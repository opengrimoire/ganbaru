import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatMessageRead,
  ChatParticipantRead,
  ChatReplyThreadPageRead,
} from "$lib/chat/contracts";
import { ChatCommunicationController } from "./communication-controller.svelte";

const api = vi.hoisted(() => ({
  readReplyThreadPage: vi.fn<
    (replyThreadId: string, cursor?: string | null) => Promise<ChatReplyThreadPageRead>
  >(),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  readChatReplyThreadPage: api.readReplyThreadPage,
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

function replyPage(previousCursor: string | null, replyOrdinal: number): ChatReplyThreadPageRead {
  return {
    thread: {
      id: REPLY_THREAD_ID,
      replyCount: 2,
      lastActivityAt: "2026-08-04T17:02:00.000Z",
      participants: [author],
      unread: false,
      workState: "ready_for_review",
    },
    rootMessage: message("root:test", 0),
    replies: [message(`reply:${replyOrdinal}`, replyOrdinal)],
    assignment: null,
    agentRuns: [],
    previousCursor,
    revision: 1,
  };
}

function message(itemId: string, ordinal: number): ChatMessageRead {
  return {
    itemId,
    conversationId: "conversation:test",
    replyThreadId: ordinal === 0 ? null : REPLY_THREAD_ID,
    revisionId: `revision:${itemId}`,
    revision: 1,
    author,
    authorLabelSnapshot: author.displayName,
    normalizedMarkdown: itemId,
    richContent: { schemaVersion: 1, value: { type: "message", content: [{ type: "text", text: itemId }] } },
    attachmentIds: [],
    references: [],
    replyThread: null,
    ordinal,
    editedAt: null,
    createdAt: `2026-08-04T17:0${ordinal}:00.000Z`,
  };
}
