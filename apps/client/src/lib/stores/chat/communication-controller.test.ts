import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatChannelPageRead,
  ChatMessageRead,
  ChatParticipantRead,
  ChatReplyThreadPageRead,
} from "$lib/chat/contracts";
import { ChatCommunicationController } from "./communication-controller.svelte";

const api = vi.hoisted(() => ({
  readReplyThreadPage: vi.fn<
    (replyThreadId: string, cursor?: string | null) => Promise<ChatReplyThreadPageRead>
  >(),
  readChannelPage: vi.fn<
    (channelId: string, cursor?: string | null) => Promise<ChatChannelPageRead>
  >(),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  readChatReplyThreadPage: api.readReplyThreadPage,
  readChatChannelPage: api.readChannelPage,
}));

const REPLY_THREAD_ID = "reply-thread:test";

const author: ChatParticipantRead = {
  id: "participant:local-owner",
  kind: "local_user",
  displayName: "You",
  avatar: { schemaVersion: 1, value: {} },
  revision: 1,
  archivedAt: null,
};

describe("ChatCommunicationController older reply history", () => {
  beforeEach(() => {
    api.readReplyThreadPage.mockReset();
  });

  it("reports a failed older reply page and clears the error on the next attempt", async () => {
    const controller = new ChatCommunicationController({
      selectedChannelId: () => null,
      openReplyThreadId: () => REPLY_THREAD_ID,
    });
    api.readReplyThreadPage.mockResolvedValueOnce(replyPage("cursor:older", 2));
    await controller.loadReplyThread(REPLY_THREAD_ID);

    api.readReplyThreadPage.mockRejectedValueOnce(new Error("Reply history is unavailable"));
    await expect(controller.loadOlderReplyThread()).rejects.toThrow("Reply history is unavailable");

    expect(controller.replyThreadError).not.toBeNull();
    expect(controller.replyThreadLoading).toBe(false);
    expect(controller.replyThread?.replies.map((reply) => reply.itemId)).toEqual(["reply:2"]);

    api.readReplyThreadPage.mockResolvedValueOnce(replyPage(null, 1));
    await controller.loadOlderReplyThread();

    expect(api.readReplyThreadPage).toHaveBeenLastCalledWith(REPLY_THREAD_ID, "cursor:older");
    expect(controller.replyThreadError).toBeNull();
    expect(controller.replyThread?.replies.map((reply) => reply.itemId)).toEqual(["reply:1", "reply:2"]);
  });
});


describe("ChatCommunicationController channel history ownership", () => {
  let selectedChannelId: string | null = null;

  beforeEach(() => {
    api.readChannelPage.mockReset();
    selectedChannelId = null;
  });

  function controller(): ChatCommunicationController {
    return new ChatCommunicationController({
      selectedChannelId: () => selectedChannelId,
      openReplyThreadId: () => null,
    });
  }

  it("keeps the previous channel as the owner of loaded messages until the next channel's page arrives", async () => {
    const communication = controller();
    selectedChannelId = "channel:a";
    api.readChannelPage.mockResolvedValueOnce(channelPage("channel:a", "a:1"));
    await communication.loadChannel("channel:a");

    selectedChannelId = "channel:b";
    let resolvePage: (page: ChatChannelPageRead) => void = () => undefined;
    api.readChannelPage.mockReturnValueOnce(new Promise((resolve) => { resolvePage = resolve; }));
    const loading = communication.loadChannel("channel:b");

    expect(communication.channelMessagesChannelId).toBe("channel:a");
    expect(communication.channelLoading).toBe(true);

    resolvePage(channelPage("channel:b", "b:1"));
    await loading;

    expect(communication.channelMessagesChannelId).toBe("channel:b");
    expect(communication.channelMessages.map((entry) => entry.itemId)).toEqual(["b:1"]);
  });

  it("shows a cached channel synchronously and reports a miss without changing the loaded history", async () => {
    const communication = controller();
    selectedChannelId = "channel:a";
    api.readChannelPage.mockResolvedValueOnce(channelPage("channel:a", "a:1"));
    await communication.loadChannel("channel:a");
    selectedChannelId = "channel:b";
    api.readChannelPage.mockResolvedValueOnce(channelPage("channel:b", "b:1"));
    await communication.loadChannel("channel:b");

    expect(communication.showCachedChannel("channel:a")).toBe(true);
    expect(communication.channelMessagesChannelId).toBe("channel:a");
    expect(communication.channelMessages.map((entry) => entry.itemId)).toEqual(["a:1"]);

    expect(communication.showCachedChannel("channel:missing")).toBe(false);
    expect(communication.channelMessagesChannelId).toBe("channel:a");
  });

  it("forgets the history owner on reset", async () => {
    const communication = controller();
    selectedChannelId = "channel:a";
    api.readChannelPage.mockResolvedValueOnce(channelPage("channel:a", "a:1"));
    await communication.loadChannel("channel:a");

    communication.reset();

    expect(communication.channelMessagesChannelId).toBeNull();
    expect(communication.showCachedChannel("channel:a")).toBe(false);
  });
});

function channelPage(channelId: string, itemId: string): ChatChannelPageRead {
  return { channelId, messages: [message(itemId, 1)], previousCursor: null, revision: 1 };
}

function replyPage(previousCursor: string | null, replyPosition: number): ChatReplyThreadPageRead {
  return {
    thread: {
      id: REPLY_THREAD_ID,
      replyCount: 2,
      lastActivityAt: "2026-08-04T17:02:00.000Z",
      participants: [author],
      unread: false,
      unseenFailure: false,
      workState: "ready_for_review",
    },
    rootMessage: message("root:test", 0),
    replies: [message(`reply:${replyPosition}`, replyPosition)],
    assignment: null,
    agentRuns: [],
    delegatedChildren: [],
    previousCursor,
    revision: 1,
  };
}

function message(itemId: string, position: number): ChatMessageRead {
  return {
    itemId,
    conversationId: "conversation:test",
    replyThreadId: position === 0 ? null : REPLY_THREAD_ID,
    revisionId: `revision:${itemId}`,
    revision: 1,
    author,
    authorLabelSnapshot: author.displayName,
    normalizedMarkdown: itemId,
    richContent: { schemaVersion: 1, value: { type: "message", content: [{ type: "text", text: itemId }] } },
    attachmentIds: [],
    references: [],
    replyThread: null,
    orderKey: `c${String(position).padStart(3, "0")}`,
    awaitingDelivery: false,
    editedAt: null,
    createdAt: `2026-08-04T17:0${position}:00.000Z`,
  };
}
