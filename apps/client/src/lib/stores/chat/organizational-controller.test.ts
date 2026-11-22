import { describe, expect, it, vi } from "vitest";
import type {
  ChatChannelRead,
  ChatMessageRead,
  ChatParticipantRead,
  PostChatMessageResult,
} from "$lib/chat/contracts";
import { ChatOrganizationalController } from "./organizational-controller.svelte";

const api = vi.hoisted(() => ({
  postMessage: vi.fn<() => Promise<PostChatMessageResult>>(),
  readChannel: vi.fn<(channelId: string) => Promise<ChatChannelRead>>(),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  postChatMessage: api.postMessage,
  readChatChannel: api.readChannel,
}));

const REPLY_THREAD_ID = "reply-thread:a";

describe("ChatOrganizationalController posting", () => {
  it("keeps the posted reply thread out of a channel selected while the reply posted", async () => {
    let selectedChannelId = "channel:a";
    const openReplyThread = vi.fn(async () => undefined);
    const controller = new ChatOrganizationalController({
      selectedChannel: () => channel(selectedChannelId),
      selectedChannelId: () => selectedChannelId,
      openReplyThreadId: () => null,
      upsertChannel: () => undefined,
      loadChannelMessages: async () => undefined,
      loadReplyThread: async () => undefined,
      openReplyThread,
    });
    let resolvePost: (result: PostChatMessageResult) => void = () => undefined;
    api.postMessage.mockReturnValueOnce(new Promise((resolve) => { resolvePost = resolve; }));
    api.readChannel.mockImplementation(async (channelId) => channel(channelId));
    const destination = `reply-thread:${REPLY_THREAD_ID}`;
    controller.setDraft(destination, { ...controller.draft(destination), normalizedMarkdown: "Ship it" });

    const posting = controller.post(destination);
    selectedChannelId = "channel:b";
    resolvePost(postResult());
    await posting;

    expect(openReplyThread).not.toHaveBeenCalled();
  });

  it("opens the posted reply thread while its channel stays selected", async () => {
    const openReplyThread = vi.fn(async () => undefined);
    const controller = new ChatOrganizationalController({
      selectedChannel: () => channel("channel:a"),
      selectedChannelId: () => "channel:a",
      openReplyThreadId: () => null,
      upsertChannel: () => undefined,
      loadChannelMessages: async () => undefined,
      loadReplyThread: async () => undefined,
      openReplyThread,
    });
    api.postMessage.mockResolvedValueOnce(postResult());
    api.readChannel.mockImplementation(async (channelId) => channel(channelId));
    const destination = `reply-thread:${REPLY_THREAD_ID}`;
    controller.setDraft(destination, { ...controller.draft(destination), normalizedMarkdown: "Ship it" });

    await controller.post(destination);

    expect(openReplyThread).toHaveBeenCalledWith(REPLY_THREAD_ID);
  });
});

const author: ChatParticipantRead = {
  id: "participant:local-owner",
  kind: "local_user",
  displayName: "You",
  avatar: { schemaVersion: 1, value: {} },
  revision: 1,
  archivedAt: null,
};

function channel(id: string): ChatChannelRead {
  return {
    id,
    conversationId: `conversation-${id}`,
    projectId: "project-1",
    name: id,
    topic: "",
    isDefault: false,
    memberships: [],
    messageCount: 0,
    unreadCount: 0,
    unreadReplyCount: 0,
    unseenFailure: false,
    latestPreview: null,
    lastActivityAt: "2026-01-01T00:00:00Z",
    attentionState: null,
    revision: 1,
    archivedAt: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  };
}

function postResult(): PostChatMessageResult {
  const message: ChatMessageRead = {
    itemId: "item:reply",
    conversationId: "conversation-channel:a",
    replyThreadId: REPLY_THREAD_ID,
    revisionId: "revision:reply",
    revision: 1,
    author,
    authorLabelSnapshot: author.displayName,
    normalizedMarkdown: "Ship it",
    richContent: { schemaVersion: 1, value: { type: "message", content: [{ type: "text", text: "Ship it" }] } },
    attachmentIds: [],
    references: [],
    replyThread: null,
    orderKey: "c001",
    awaitingDelivery: false,
    editedAt: null,
    createdAt: "2026-01-01T00:00:00Z",
  };
  return { message, replyThreadId: REPLY_THREAD_ID, assignment: null, assignmentInputQueued: false, routedAs: null };
}
