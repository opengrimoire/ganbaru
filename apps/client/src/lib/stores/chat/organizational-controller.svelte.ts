import * as chatApi from "$lib/api/chat";
import type {
  ChatChannelId,
  ChatChannelRead,
  ChatFollowupRouting,
  ChatMessageReference,
  ChatRecurrenceSpec,
  ChatReplyThreadId,
  ChatScheduledMessageDispatchRead,
  ChatScheduledMessageId,
  ChatScheduledMessageRead,
  PostChatMessageResult,
  UpdateScheduledChatMessageRequest,
  UtcTimestamp,
  VersionedJson,
} from "$lib/chat/contracts";
import { withWorkingFolderTrust } from "$lib/api/working-folder-trust";
import { cloneData } from "$lib/utils/clone";

export interface ChatOrganizationalDraft {
  normalizedMarkdown: string;
  richContent: VersionedJson;
  attachmentIds: string[];
  references: ChatMessageReference[];
  selectionStart: number;
  selectionEnd: number;
  scheduledFor: UtcTimestamp | null;
  /** Repeat rule of the scheduled delivery; `null` sends it once. */
  recurrence: ChatRecurrenceSpec | null;
}

/** Options a post carries beyond its draft. */
export interface ChatOrganizationalPostOptions {
  alsoSendToChannel?: boolean;
  /** How a reply reaches active work in its thread. */
  followupRouting?: ChatFollowupRouting;
}

export interface ChatOrganizationalControllerOptions {
  selectedChannel: () => ChatChannelRead | null;
  selectedChannelId: () => ChatChannelId | null;
  openReplyThreadId: () => ChatReplyThreadId | null;
  upsertChannel: (channel: ChatChannelRead) => void;
  loadChannelMessages: (channelId: ChatChannelId, force?: boolean) => Promise<void>;
  loadReplyThread: (replyThreadId: ChatReplyThreadId, force?: boolean) => Promise<void>;
  openReplyThread: (replyThreadId: ChatReplyThreadId) => Promise<void>;
}

/** Owns channel and reply-thread drafts plus scheduled-message workflows. */
export class ChatOrganizationalController {
  drafts = $state<Record<string, ChatOrganizationalDraft>>({});
  scrollPositions = $state<Record<string, number>>({});
  scheduledMessagesVersion = $state(0);
  /** The last scheduled messages read for each destination, so a composer opens with its summary instead of adding it later. */
  scheduledMessagesByDestination = $state<Record<string, ChatScheduledMessageRead[]>>({});
  /** Bumped when channel notes or note proposal states may have changed, so open note views reread. */
  channelNotesVersion = $state(0);
  /** Bumped when push request states may have changed, including pushes that ran without a person, so open cards reread. */
  pushRequestsVersion = $state(0);

  constructor(private readonly options: ChatOrganizationalControllerOptions) {}

  reset(): void {
    this.drafts = {};
    this.scrollPositions = {};
    this.scheduledMessagesByDestination = {};
    this.scheduledMessagesVersion += 1;
    this.channelNotesVersion += 1;
    this.pushRequestsVersion += 1;
  }

  draft(destination: string): ChatOrganizationalDraft {
    return this.drafts[destination] ?? emptyOrganizationalDraft();
  }

  setDraft(destination: string, draft: ChatOrganizationalDraft): void {
    this.drafts = { ...this.drafts, [destination]: cloneData(draft) };
  }

  setScrollPosition(destination: string, scrollTop: number): void {
    this.scrollPositions = { ...this.scrollPositions, [destination]: scrollTop };
  }

  async post(
    destination: string,
    options: ChatOrganizationalPostOptions = {},
  ): Promise<PostChatMessageResult> {
    const channel = this.requiredChannel("posting");
    const draft = this.requiredDraft(destination, "posting");
    const replyThreadId = destinationReplyThreadId(destination);
    const result = await chatApi.postChatMessage({
      clientCommandId: crypto.randomUUID(),
      channelId: channel.id,
      replyThreadId,
      normalizedMarkdown: draft.normalizedMarkdown,
      richContent: draft.richContent,
      attachmentIds: [...draft.attachmentIds],
      references: cloneData(draft.references),
      alsoSendToChannel: options.alsoSendToChannel ?? false,
      ...(options.followupRouting ? { followupRouting: options.followupRouting } : {}),
    });
    this.setDraft(destination, emptyOrganizationalDraft());
    await this.options.loadChannelMessages(channel.id, true);
    if ((replyThreadId || result.assignment) && result.replyThreadId) {
      await this.options.openReplyThread(result.replyThreadId);
    }
    this.options.upsertChannel(await chatApi.readChatChannel(channel.id));
    return result;
  }

  async schedule(
    destination: string,
    scheduledFor: UtcTimestamp,
    options: { alsoSendToChannel?: boolean } = {},
  ): Promise<ChatScheduledMessageRead> {
    const channel = this.requiredChannel("scheduling");
    const draft = this.requiredDraft(destination, "scheduling");
    const scheduled = await chatApi.scheduleChatMessage({
      scheduledMessageId: crypto.randomUUID(),
      scheduledFor,
      message: {
        clientCommandId: crypto.randomUUID(),
        channelId: channel.id,
        replyThreadId: destinationReplyThreadId(destination),
        normalizedMarkdown: draft.normalizedMarkdown,
        richContent: draft.richContent,
        attachmentIds: [...draft.attachmentIds],
        references: cloneData(draft.references),
        alsoSendToChannel: options.alsoSendToChannel ?? false,
      },
      recurrence: draft.recurrence ? cloneData(draft.recurrence) : null,
    });
    this.setDraft(destination, emptyOrganizationalDraft());
    this.scheduledMessagesVersion += 1;
    return scheduled;
  }

  /**
   * Reads a destination's scheduled messages and remembers them as its summary.
   *
   * @param destination - The channel or reply thread destination.
   * @returns The scheduled messages; a failed read rejects and records an empty summary only when none is known.
   */
  async listScheduled(destination: string): Promise<ChatScheduledMessageRead[]> {
    const channel = this.options.selectedChannel();
    if (!channel) return [];
    try {
      const messages = await chatApi.listScheduledChatMessages(channel.id, destinationReplyThreadId(destination));
      this.rememberScheduled(destination, messages);
      return messages;
    } catch (error: unknown) {
      if (!(destination in this.scheduledMessagesByDestination)) this.rememberScheduled(destination, []);
      throw error;
    }
  }

  /** Records the scheduled messages a destination shows in its summary. */
  rememberScheduled(destination: string, messages: ChatScheduledMessageRead[]): void {
    this.scheduledMessagesByDestination = { ...this.scheduledMessagesByDestination, [destination]: messages };
  }

  async cancelScheduled(id: ChatScheduledMessageId): Promise<void> {
    await chatApi.cancelScheduledChatMessage(id);
    this.scheduledMessagesVersion += 1;
  }

  /** Schedules the message a person confirmed from an agent's schedule proposal. */
  async confirmScheduleProposal(
    request: import("$lib/chat/contracts").ConfirmScheduleProposalRequest,
  ): Promise<ChatScheduledMessageRead> {
    const scheduled = await chatApi.confirmChatScheduleProposal(request);
    this.scheduledMessagesVersion += 1;
    return scheduled;
  }

  /**
   * Pushes the commit of an agent's push request in `workingFolderId`, asking for a trust decision
   * about the folder's Git configuration when this device has none, then rereads the open thread so
   * the confirming reply shows.
   */
  async confirmPushRequest(
    request: import("$lib/chat/contracts").ConfirmPushRequestRequest,
    workingFolderId: string,
  ): Promise<PostChatMessageResult> {
    const result = await withWorkingFolderTrust(workingFolderId, () => chatApi.confirmChatPushRequest(request));
    const channelId = this.options.selectedChannelId();
    if (channelId) await this.options.loadChannelMessages(channelId, true);
    const replyThreadId = this.options.openReplyThreadId();
    if (replyThreadId && replyThreadId === result.replyThreadId) {
      await this.options.loadReplyThread(replyThreadId, true);
    }
    return result;
  }

  /** Replaces the content, time, and repeat rule of a scheduled message in place. */
  async updateScheduled(request: UpdateScheduledChatMessageRequest): Promise<ChatScheduledMessageRead> {
    const scheduled = await chatApi.updateScheduledChatMessage(request);
    this.scheduledMessagesVersion += 1;
    return scheduled;
  }

  async retryScheduled(id: ChatScheduledMessageId): Promise<ChatScheduledMessageRead> {
    const scheduled = await chatApi.retryScheduledChatMessage(id);
    this.scheduledMessagesVersion += 1;
    return scheduled;
  }

  async sendScheduledNow(id: ChatScheduledMessageId): Promise<PostChatMessageResult> {
    const channelId = this.options.selectedChannelId();
    const replyThreadId = this.options.openReplyThreadId();
    try {
      const result = await chatApi.sendScheduledChatMessageNow(id);
      if (channelId) {
        this.options.upsertChannel(await chatApi.readChatChannel(channelId));
        if (this.options.selectedChannelId() === channelId) {
          await this.options.loadChannelMessages(channelId, true);
        }
        if (replyThreadId
          && this.options.openReplyThreadId() === replyThreadId
          && result.message.replyThreadId === replyThreadId) {
          await this.options.loadReplyThread(replyThreadId, true);
        }
      }
      return result;
    } finally {
      this.scheduledMessagesVersion += 1;
    }
  }

  async dispatchDue(): Promise<ChatScheduledMessageDispatchRead> {
    const result = await chatApi.dispatchDueScheduledChatMessages();
    if (result.processedCount > 0) this.scheduledMessagesVersion += 1;
    if (result.dispatchedChannelIds.length === 0) return result;
    const channelReads = await Promise.allSettled(
      result.dispatchedChannelIds.map((channelId) => chatApi.readChatChannel(channelId)),
    );
    for (const channelRead of channelReads) {
      if (channelRead.status === "fulfilled") this.options.upsertChannel(channelRead.value);
      else console.error("Scheduled message channel refresh failed", channelRead.reason);
    }
    const selectedChannelId = this.options.selectedChannelId();
    if (selectedChannelId && result.dispatchedChannelIds.includes(selectedChannelId)) {
      await this.options.loadChannelMessages(selectedChannelId, true);
      const replyThreadId = this.options.openReplyThreadId();
      if (replyThreadId) await this.options.loadReplyThread(replyThreadId, true);
    }
    return result;
  }

  private requiredChannel(action: string): ChatChannelRead {
    const channel = this.options.selectedChannel();
    if (!channel) throw new Error(`Choose a channel before ${action}`);
    return channel;
  }

  private requiredDraft(destination: string, action: string): ChatOrganizationalDraft {
    const draft = this.draft(destination);
    if (!draft.normalizedMarkdown.trim()
      && draft.attachmentIds.length === 0
      && draft.references.length === 0) {
      throw new Error(`Write a message or attach context before ${action}`);
    }
    return draft;
  }
}

function destinationReplyThreadId(destination: string): ChatReplyThreadId | null {
  return destination.startsWith("reply-thread:")
    ? destination.slice("reply-thread:".length)
    : null;
}

function emptyOrganizationalDraft(): ChatOrganizationalDraft {
  return {
    normalizedMarkdown: "",
    richContent: { schemaVersion: 1, value: { type: "document", children: [] } },
    attachmentIds: [],
    references: [],
    selectionStart: 0,
    selectionEnd: 0,
    scheduledFor: null,
    recurrence: null,
  };
}
