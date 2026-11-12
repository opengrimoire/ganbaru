import { describe, expect, it } from "vitest";
import type { ChatMessageRead } from "$lib/chat/contracts";
import { buildChannelFeedRows, CHANNEL_FEED_UNREAD_ROW_ID, type ChannelFeedRow } from "./channel-feed-rows";
import { unreadMessageStartIndex } from "./organizational-message-model";

/** Builds an ISO timestamp from local wall time, so day boundaries hold in every test time zone. */
function localTime(day: number, hour: number, minute = 0): string {
  return new Date(2026, 7, day, hour, minute).toISOString();
}

function message(itemId: string, authorId: string | null, createdAt: string): ChatMessageRead {
  return {
    itemId,
    conversationId: "conversation:test",
    replyThreadId: null,
    revisionId: `revision:${itemId}`,
    revision: 1,
    author: authorId === null ? null : {
      id: authorId,
      kind: authorId === "me" ? "local_user" : "human",
      displayName: authorId,
      avatar: { schemaVersion: 1, value: {} },
      revision: 1,
      archivedAt: null,
    },
    authorLabelSnapshot: authorId ?? "Removed",
    normalizedMarkdown: itemId,
    richContent: { schemaVersion: 1, value: {} },
    attachmentIds: [],
    references: [],
    replyThread: null,
    orderKey: itemId,
    editedAt: null,
    createdAt,
  };
}

function shape(rows: readonly ChannelFeedRow[]): string[] {
  return rows.map((row) => {
    if (row.kind === "message") return `${row.message.itemId}${row.grouped ? " grouped" : ""}`;
    if (row.kind === "date-divider") return `date${row.first ? " first" : ""}`;
    return "unread";
  });
}

describe("channel feed rows", () => {
  it("adds a date divider per local day and groups one author's messages within five minutes", () => {
    const messages = [
      message("a", "ana", localTime(10, 9, 0)),
      message("b", "ana", localTime(10, 9, 4)),
      message("c", "ana", localTime(10, 9, 10)),
      message("d", "ben", localTime(10, 9, 11)),
      message("e", "ben", localTime(10, 23, 59)),
      message("f", "ben", localTime(11, 0, 1)),
      message("g", null, localTime(11, 0, 2)),
      message("h", null, localTime(11, 0, 3)),
    ];

    const rows = buildChannelFeedRows(messages, messages.length, 0, "en");

    expect(shape(rows)).toEqual(["date first", "a", "b grouped", "c", "d", "e", "date", "f", "g", "h"]);
    const dates = rows.filter((row) => row.kind === "date-divider");
    expect(dates[0]?.label).toContain("2026");
    expect(new Set(rows.map((row) => row.id)).size).toBe(rows.length);
  });

  it("places the unread divider before the first unread message and before its date divider", () => {
    const messages = [
      message("seen", "ana", localTime(10, 9)),
      message("incoming", "ben", localTime(11, 9)),
      message("mine", "me", localTime(11, 9, 1)),
    ];
    const unreadStart = unreadMessageStartIndex(messages, 1);
    const rows = buildChannelFeedRows(messages, unreadStart, 1, "en");

    expect(shape(rows)).toEqual(["date first", "seen", "unread", "date", "incoming", "mine"]);
    expect(rows[2]?.id).toBe(CHANNEL_FEED_UNREAD_ROW_ID);
  });

  it("shows no unread divider when the unread count is zero", () => {
    const messages = [message("a", "ana", localTime(10, 9)), message("b", "ben", localTime(10, 10))];

    expect(shape(buildChannelFeedRows(messages, 1, 0, "en"))).toEqual(["date first", "a", "b"]);
  });

  it("keeps every existing row ID when older messages of the same day are prepended", () => {
    const newer = [message("c", "ana", localTime(10, 12)), message("d", "ben", localTime(11, 8))];
    const older = [message("a", "ben", localTime(9, 20)), message("b", "ana", localTime(10, 7))];

    const before = buildChannelFeedRows(newer, newer.length, 0, "en");
    const after = buildChannelFeedRows([...older, ...newer], older.length + newer.length, 0, "en");

    const afterIds = new Set(after.map((row) => row.id));
    for (const row of before) expect(afterIds.has(row.id)).toBe(true);
    expect(before[0]?.kind === "date-divider" && before[0].first).toBe(true);
    const sameDay = after.find((row) => row.id === before[0]?.id);
    expect(sameDay?.kind === "date-divider" && sameDay.first).toBe(false);
  });

  it("returns no rows for an empty channel", () => {
    expect(buildChannelFeedRows([], 0, 3, "en")).toEqual([]);
  });
});
