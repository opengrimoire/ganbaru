import { describe, expect, it } from "vitest";
import type { ChatMessageRead } from "$lib/chat/contracts";
import {
  buildChannelFeedRows,
  CHANNEL_FEED_UNREAD_ROW_ID,
  channelFeedDateDividerRowId,
  channelFeedMessageRowId,
  channelFeedScrollAnchorRowId,
  type ChannelFeedRow,
} from "./channel-feed-rows";
import { unreadMessageStartIndex } from "./organizational-message-model";
import { computeTimelineVirtualWindow, scrollTopForPreservedAnchor, virtualRowTop } from "./virtualization";

const ROW_ESTIMATE_PX = 72;
const OVERSCAN_PX = 600;

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

  it("keeps row IDs unique when order keys put one local day before and after another", () => {
    const messages = [
      message("a", "ana", localTime(11, 9)),
      message("b", "ben", localTime(10, 22)),
      message("c", "ana", localTime(11, 9, 1)),
    ];

    const rows = buildChannelFeedRows(messages, messages.length, 0, "en");

    expect(shape(rows)).toEqual(["date first", "a", "date", "b", "date", "c"]);
    expect(new Set(rows.map((row) => row.id)).size).toBe(rows.length);
    expect(rows.filter((row) => row.kind === "date-divider").map((row) => row.id)).toEqual(
      ["a", "b", "c"].map(channelFeedDateDividerRowId),
    );
  });

  it("keeps every message row ID when older messages of the same day are prepended", () => {
    const newer = [message("c", "ana", localTime(10, 12)), message("d", "ben", localTime(11, 8))];
    const older = [message("a", "ben", localTime(9, 20)), message("b", "ana", localTime(10, 7))];

    const before = buildChannelFeedRows(newer, newer.length, 0, "en");
    const after = buildChannelFeedRows([...older, ...newer], older.length + newer.length, 0, "en");

    const afterIds = new Set(after.map((row) => row.id));
    for (const row of before) {
      if (row.kind === "message") expect(afterIds.has(row.id)).toBe(true);
    }
    expect(shape(after)).toEqual(["date first", "a", "date", "b", "c", "date", "d"]);
    expect(afterIds.has(channelFeedDateDividerRowId("c"))).toBe(false);
    expect(after[2]?.id).toBe(channelFeedDateDividerRowId("b"));
    expect(after.at(-1)?.id).toBe(channelFeedMessageRowId("d"));
  });

  it("anchors a load older on the first message row below the viewport top, skipping dividers", () => {
    const messages = [message("a", "ana", localTime(10, 9)), message("b", "ben", localTime(10, 10))];
    const rows = buildChannelFeedRows(messages, 1, 1, "en");
    const window = computeTimelineVirtualWindow(rows, new Map(), 0, 400, ROW_ESTIMATE_PX, OVERSCAN_PX);

    expect(shape(rows)).toEqual(["date first", "a", "unread", "b"]);
    expect(channelFeedScrollAnchorRowId(window.items, -40)).toBe(channelFeedMessageRowId("a"));
    expect(channelFeedScrollAnchorRowId(window.items, ROW_ESTIMATE_PX * 2)).toBe(channelFeedMessageRowId("b"));
    expect(channelFeedScrollAnchorRowId(window.items, ROW_ESTIMATE_PX * 4)).toBeNull();
    expect(channelFeedScrollAnchorRowId([], 0)).toBeNull();
  });

  it("keeps the anchored message at its offset when a same-day older page is prepended", () => {
    const newer = [message("c", "ana", localTime(10, 12)), message("d", "ben", localTime(10, 13))];
    const older = [message("a", "ben", localTime(10, 8)), message("b", "ana", localTime(10, 9))];
    const heights = new Map([[channelFeedDateDividerRowId("c"), 30], [channelFeedMessageRowId("c"), 90]]);
    const buttonOffset = 40;
    const scrollTop = 0;
    const before = buildChannelFeedRows(newer, newer.length, 0, "en");
    const window = computeTimelineVirtualWindow(before, heights, scrollTop - buttonOffset, 400, ROW_ESTIMATE_PX, OVERSCAN_PX);

    const anchorId = channelFeedScrollAnchorRowId(window.items, scrollTop - buttonOffset);
    expect(anchorId).toBe(channelFeedMessageRowId("c"));
    if (!anchorId) return;
    const anchorTopBefore = buttonOffset + (virtualRowTop(before, anchorId, heights, ROW_ESTIMATE_PX) ?? Number.NaN);

    const after = buildChannelFeedRows([...older, ...newer], older.length + newer.length, 0, "en");
    const buttonOffsetAfter = 0;
    const anchorTopAfter = buttonOffsetAfter + (virtualRowTop(after, anchorId, heights, ROW_ESTIMATE_PX) ?? Number.NaN);
    const scrollTopAfter = scrollTopForPreservedAnchor(scrollTop, anchorTopBefore, anchorTopAfter);

    expect(shape(after)).toEqual(["date first", "a", "b", "c", "d"]);
    expect(scrollTopAfter).toBe(ROW_ESTIMATE_PX * 3 - buttonOffset - 30);
    expect(anchorTopAfter - scrollTopAfter).toBe(anchorTopBefore - scrollTop);
  });

  it("returns no rows for an empty channel", () => {
    expect(buildChannelFeedRows([], 0, 3, "en")).toEqual([]);
  });
});
