import * as chatApi from "$lib/api/chat";
import { ChatConversationPageCache } from "$lib/chat/timeline/conversation-page-cache";
import type {
  ChatChannelId,
  ChatChannelPageRead,
  ChatMessageRead,
  ChatReplyThreadId,
  ChatReplyThreadPageRead,
  ChatReplyThreadSummaryRead,
} from "$lib/chat/contracts";
import { chatErrorMessage } from "$lib/chat/error-presentation";
import { applyReplyThreadSummary } from "$lib/chat/timeline/organizational-message-model";

const CACHE_MAX_ENTRIES = 6;
const CACHE_MAX_BYTES = 8 * 1024 * 1024;
const MAX_PAGES_PER_CONVERSATION = 8;

export interface ChatCommunicationControllerOptions {
  selectedChannelId: () => ChatChannelId | null;
  openReplyThreadId: () => ChatReplyThreadId | null;
}

/** Owns bounded loading and caching for channel and reply-thread messages. */
export class ChatCommunicationController {
  channelPages = $state<ChatChannelPageRead[]>([]);
  channelMessages = $state<ChatMessageRead[]>([]);
  channelLoading = $state(false);
  channelError = $state<string | null>(null);
  replyThreadPages = $state<ChatReplyThreadPageRead[]>([]);
  replyThread = $state<ChatReplyThreadPageRead | null>(null);
  replyThreadLoading = $state(false);
  replyThreadError = $state<string | null>(null);

  private channelRequest = 0;
  private replyThreadRequest = 0;
  private readonly channelCache = new ChatConversationPageCache<ChatChannelPageRead>(
    CACHE_MAX_ENTRIES,
    CACHE_MAX_BYTES,
    MAX_PAGES_PER_CONVERSATION,
  );
  private readonly replyThreadCache = new ChatConversationPageCache<ChatReplyThreadPageRead>(
    CACHE_MAX_ENTRIES,
    CACHE_MAX_BYTES,
    MAX_PAGES_PER_CONVERSATION,
  );

  constructor(private readonly options: ChatCommunicationControllerOptions) {}

  reset(): void {
    this.channelRequest += 1;
    this.replyThreadRequest += 1;
    this.channelCache.clear();
    this.replyThreadCache.clear();
    this.channelPages = [];
    this.channelMessages = [];
    this.channelLoading = false;
    this.channelError = null;
    this.replyThreadPages = [];
    this.replyThread = null;
    this.replyThreadLoading = false;
    this.replyThreadError = null;
  }

  clearReplyThread(): void {
    this.replyThreadRequest += 1;
    this.replyThreadPages = [];
    this.replyThread = null;
    this.replyThreadLoading = false;
    this.replyThreadError = null;
  }

  hasCachedChannel(channelId: ChatChannelId): boolean {
    return this.channelCache.get(channelId) !== null;
  }

  async prefetchChannel(
    channelId: ChatChannelId,
    shouldCache: () => boolean = () => true,
  ): Promise<void> {
    if (this.hasCachedChannel(channelId)) return;
    const page = await chatApi.readChatChannelPage(channelId);
    if (shouldCache()) this.channelCache.set(channelId, [page]);
  }

  async loadChannel(channelId: ChatChannelId, force = false): Promise<void> {
    if (!force) {
      const cached = this.channelCache.get(channelId);
      if (cached) {
        this.channelLoading = false;
        this.channelError = null;
        this.channelPages = cached;
        this.channelMessages = mergeMessages(cached.flatMap((page) => page.messages));
        return;
      }
    }
    const request = ++this.channelRequest;
    this.channelLoading = true;
    this.channelError = null;
    try {
      const page = await chatApi.readChatChannelPage(channelId);
      if (!this.isCurrentChannelRequest(request, channelId)) return;
      this.channelPages = [page];
      this.channelMessages = page.messages;
      this.channelCache.set(channelId, this.channelPages);
    } catch (error: unknown) {
      if (request !== this.channelRequest) return;
      this.channelError = chatErrorMessage(error, "Chat channel could not be loaded");
      throw error;
    } finally {
      if (request === this.channelRequest) this.channelLoading = false;
    }
  }

  async loadOlderChannel(): Promise<void> {
    const channelId = this.options.selectedChannelId();
    const currentPage = this.channelPages[0];
    if (!channelId || this.channelLoading || !currentPage?.previousCursor) return;
    const request = ++this.channelRequest;
    this.channelLoading = true;
    this.channelError = null;
    try {
      const page = await chatApi.readChatChannelPage(channelId, currentPage.previousCursor);
      if (!this.isCurrentChannelRequest(request, channelId)) return;
      this.channelPages = [page, ...this.channelPages].slice(0, MAX_PAGES_PER_CONVERSATION);
      this.channelMessages = mergeMessages(this.channelPages.flatMap((entry) => entry.messages));
      this.channelCache.set(channelId, this.channelPages);
    } catch (error: unknown) {
      if (request !== this.channelRequest) return;
      this.channelError = chatErrorMessage(error, "Older channel history could not be loaded");
      throw error;
    } finally {
      if (request === this.channelRequest) this.channelLoading = false;
    }
  }

  async loadChannelAtCursor(channelId: ChatChannelId, cursor: string): Promise<void> {
    const request = ++this.channelRequest;
    this.channelLoading = true;
    this.channelError = null;
    try {
      const page = await chatApi.readChatChannelPage(channelId, cursor);
      if (!this.isCurrentChannelRequest(request, channelId)) return;
      this.channelPages = [page];
      this.channelMessages = page.messages;
    } catch (error: unknown) {
      if (request !== this.channelRequest) return;
      this.channelError = chatErrorMessage(error, "Search result could not be loaded");
      throw error;
    } finally {
      if (request === this.channelRequest) this.channelLoading = false;
    }
  }

  async loadReplyThread(replyThreadId: ChatReplyThreadId, force = false): Promise<void> {
    if (!force) {
      const cached = this.replyThreadCache.get(replyThreadId);
      if (cached) {
        this.replyThreadPages = cached;
        this.replyThread = mergeReplyThreadPages(cached);
        this.replyThreadLoading = false;
        this.replyThreadError = null;
        return;
      }
    }
    const request = ++this.replyThreadRequest;
    this.replyThreadLoading = true;
    this.replyThreadError = null;
    try {
      const page = await chatApi.readChatReplyThreadPage(replyThreadId);
      if (!this.isCurrentReplyThreadRequest(request, replyThreadId)) return;
      this.updateLoadedReplyThreadSummary(page.thread);
      this.replyThreadPages = [page];
      this.replyThread = page;
      this.replyThreadCache.set(replyThreadId, [page]);
    } catch (error: unknown) {
      if (request !== this.replyThreadRequest) return;
      this.replyThreadError = chatErrorMessage(error, "Reply thread could not be loaded");
      throw error;
    } finally {
      if (request === this.replyThreadRequest) this.replyThreadLoading = false;
    }
  }

  async loadOlderReplyThread(): Promise<void> {
    const replyThreadId = this.options.openReplyThreadId();
    const currentPage = this.replyThreadPages[0];
    if (!replyThreadId || this.replyThreadLoading || !currentPage?.previousCursor) return;
    const request = ++this.replyThreadRequest;
    this.replyThreadLoading = true;
    this.replyThreadError = null;
    try {
      const page = await chatApi.readChatReplyThreadPage(replyThreadId, currentPage.previousCursor);
      if (!this.isCurrentReplyThreadRequest(request, replyThreadId)) return;
      this.updateLoadedReplyThreadSummary(page.thread);
      this.replyThreadPages = [page, ...this.replyThreadPages].slice(0, MAX_PAGES_PER_CONVERSATION);
      this.replyThread = mergeReplyThreadPages(this.replyThreadPages);
      this.replyThreadCache.set(replyThreadId, this.replyThreadPages);
    } catch (error: unknown) {
      if (request !== this.replyThreadRequest) return;
      this.replyThreadError = chatErrorMessage(error, "Older reply thread history could not be loaded");
      throw error;
    } finally {
      if (request === this.replyThreadRequest) this.replyThreadLoading = false;
    }
  }

  async loadReplyThreadAtCursor(replyThreadId: ChatReplyThreadId, cursor: string): Promise<void> {
    const request = ++this.replyThreadRequest;
    this.replyThreadLoading = true;
    this.replyThreadError = null;
    try {
      const page = await chatApi.readChatReplyThreadPage(replyThreadId, cursor);
      if (!this.isCurrentReplyThreadRequest(request, replyThreadId)) return;
      this.updateLoadedReplyThreadSummary(page.thread);
      this.replyThreadPages = [page];
      this.replyThread = page;
    } catch (error: unknown) {
      if (request !== this.replyThreadRequest) return;
      this.replyThreadError = chatErrorMessage(error, "Search result thread could not be loaded");
      throw error;
    } finally {
      if (request === this.replyThreadRequest) this.replyThreadLoading = false;
    }
  }

  private isCurrentChannelRequest(request: number, channelId: ChatChannelId): boolean {
    return request === this.channelRequest && this.options.selectedChannelId() === channelId;
  }

  private updateLoadedReplyThreadSummary(summary: ChatReplyThreadSummaryRead): void {
    const updatedPages = this.channelPages.map((page) => ({
      ...page,
      messages: applyReplyThreadSummary(page.messages, summary),
    }));
    if (!updatedPages.some((page, index) => page.messages.some(
      (message, messageIndex) => message !== this.channelPages[index]?.messages[messageIndex],
    ))) return;
    this.channelPages = updatedPages;
    this.channelMessages = mergeMessages(updatedPages.flatMap((page) => page.messages));
    const channelId = this.options.selectedChannelId();
    if (channelId) this.channelCache.set(channelId, updatedPages);
  }

  private isCurrentReplyThreadRequest(request: number, replyThreadId: ChatReplyThreadId): boolean {
    return request === this.replyThreadRequest && this.options.openReplyThreadId() === replyThreadId;
  }
}

function mergeMessages(messages: readonly ChatMessageRead[]): ChatMessageRead[] {
  const byId = new Map(messages.map((message) => [message.itemId, message]));
  return [...byId.values()].sort((left, right) => (
    left.ordinal - right.ordinal || left.itemId.localeCompare(right.itemId)
  ));
}

function mergeReplyThreadPages(pages: readonly ChatReplyThreadPageRead[]): ChatReplyThreadPageRead | null {
  const latest = pages.at(-1);
  if (!latest) return null;
  return {
    ...latest,
    replies: mergeMessages(pages.flatMap((page) => page.replies)),
    previousCursor: pages[0]?.previousCursor ?? null,
    agentRuns: pages.flatMap((page) => page.agentRuns).filter((run, index, runs) => (
      runs.findIndex((candidate) => candidate.id === run.id) === index
    )),
  };
}
