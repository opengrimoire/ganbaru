import { describe, expect, it } from "vitest";
import type {
  ChatMessageRead,
  ChatParticipantKind,
  ChatReplyThreadSummaryRead,
} from "$lib/chat/contracts";
import {
  applyReplyThreadSummary,
  isChatMessageReactionValue,
  toggleChatMessageReactionParticipant,
  unreadMessageStartIndex,
} from "./organizational-message-model";

function message(itemId: string, kind: ChatParticipantKind): ChatMessageRead {
  return {
    itemId,
    conversationId: "conversation:test",
    replyThreadId: null,
    revisionId: `revision:${itemId}`,
    revision: 1,
    author: {
      id: `participant:${itemId}`,
      kind,
      displayName: itemId,
      avatar: { schemaVersion: 1, value: {} },
      revision: 1,
      archivedAt: null,
    },
    authorLabelSnapshot: itemId,
    normalizedMarkdown: itemId,
    richContent: { schemaVersion: 1, value: {} },
    attachmentIds: [],
    references: [],
    replyThread: null,
    ordinal: 1,
    editedAt: null,
    createdAt: "2026-08-04T12:00:00.000Z",
  };
}

describe("organizational unread messages", () => {
  it("places the divider before unread incoming messages rather than later local messages", () => {
    const messages = [
      message("read", "human"),
      message("unread-agent", "ai_teammate"),
      message("local-reply", "local_user"),
    ];

    expect(unreadMessageStartIndex(messages, 1)).toBe(1);
  });

  it("returns the loaded-page start when the unread count exceeds loaded incoming messages", () => {
    expect(unreadMessageStartIndex([message("local", "local_user")], 2)).toBe(0);
  });

  it("clears a root message unread dot from a freshly read thread summary", () => {
    const root = message("root", "local_user");
    const summary: ChatReplyThreadSummaryRead = {
      id: "reply-thread:test",
      replyCount: 1,
      lastActivityAt: "2026-08-04T12:00:00.000Z",
      participants: [],
      unread: true,
      workState: null,
    };
    root.replyThread = summary;
    const readSummary = { ...summary, unread: false };

    const updated = applyReplyThreadSummary([root, message("other", "human")], readSummary);

    expect(updated[0]?.replyThread).toEqual(readSummary);
    expect(updated[1]?.replyThread).toBeNull();
  });
});

describe("local message reactions", () => {
  const localParticipant = { participantId: "participant:local-owner", displayName: "Alice" };
  const teammate = { participantId: "participant:teammate", displayName: "Atlas" };

  it("accepts only Unicode and custom emoji picker values", () => {
    expect(isChatMessageReactionValue("emoji:✅")).toBe(true);
    expect(isChatMessageReactionValue("custom-emoji:focus")).toBe(true);
    expect(isChatMessageReactionValue("lucide:smile")).toBe(false);
    expect(isChatMessageReactionValue("emoji:  ")).toBe(false);
  });

  it("adds multiple reactions and removes only the local participant when toggled", () => {
    const existing = [{ value: "emoji:✅", participants: [teammate] }];
    const selected = toggleChatMessageReactionParticipant(existing, "emoji:✅", localParticipant);
    const withAnother = toggleChatMessageReactionParticipant(selected, "emoji:🙌", localParticipant);

    expect(withAnother).toEqual([
      { value: "emoji:✅", participants: [teammate, localParticipant] },
      { value: "emoji:🙌", participants: [localParticipant] },
    ]);
    expect(toggleChatMessageReactionParticipant(withAnother, "emoji:✅", localParticipant)).toEqual([
      { value: "emoji:✅", participants: [teammate] },
      { value: "emoji:🙌", participants: [localParticipant] },
    ]);
  });

  it("removes a reaction when its final participant toggles it off", () => {
    const selected = toggleChatMessageReactionParticipant([], "custom-emoji:focus", localParticipant);
    expect(toggleChatMessageReactionParticipant(selected, "custom-emoji:focus", localParticipant)).toEqual([]);
  });
});
