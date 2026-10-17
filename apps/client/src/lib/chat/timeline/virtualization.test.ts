import { describe, expect, it } from "vitest";
import type { ChatTimelineItemRead, ChatTimelinePageRead } from "$lib/chat/contracts";
import type { TimelineMessageRow } from "./model";
import {
  computeTimelineVirtualWindow,
  evictTimelinePages,
  mergeTimelineItems,
  nextTimelineUnreadCount,
  scrollTopForPreservedAnchor,
  timelineMinimapRows,
  timelineScrollOverflowState,
  timelineScrollbarThumbGeometry,
  timelineScrollIntent,
} from "./virtualization";

function row(index: number): TimelineMessageRow {
  return { id: `row-${index}`, kind: "message", role: "assistant", turnId: null, sequence: index, createdAt: "2026-07-21T14:00:00.000Z", markdown: String(index), state: "complete", phase: null, userContext: null, metadata: null };
}

function item(sequence: number): ChatTimelineItemRead {
  return { activityId: `item-${sequence}`, turnId: null, sequenceAnchor: sequence, kind: "message", data: { schemaVersion: 1, value: {} } };
}

function page(start: number, end: number): ChatTimelinePageRead {
  return { threadId: "thread", items: Array.from({ length: end - start + 1 }, (_, offset) => item(start + offset)), turns: [], previousCursor: null, nextCursor: null, threadRevision: 1 };
}

describe("timeline virtualization", () => {
  it("keeps a 10,000-row conversation inside a bounded render window", () => {
    const rows = Array.from({ length: 10_000 }, (_, index) => row(index));
    const measurements = new Map([["row-5000", 160]]);
    const result = computeTimelineVirtualWindow(rows, measurements, 440_000, 800, 88, 440);
    expect(result.items.length).toBeLessThan(30);
    expect(result.startIndex).toBeGreaterThan(4_900);
    expect(result.endIndex).toBeLessThan(5_100);
    expect(result.totalHeight).toBe(880_072);
    expect(result.paddingTop + result.paddingBottom).toBeLessThan(result.totalHeight);
    const markers = timelineMinimapRows(rows);
    expect(markers).toHaveLength(120);
    expect(markers[0]?.id).toBe("row-0");
    expect(markers.at(-1)?.id).toBe("row-9999");
  });

  it("preserves an anchor when rows are prepended or expanded", () => {
    expect(scrollTopForPreservedAnchor(320, 48, 488)).toBe(760);
    expect(scrollTopForPreservedAnchor(640, 520, 420)).toBe(540);
  });

  it("keeps scrollbar thumb geometry bounded while disclosure height changes", () => {
    expect(timelineScrollbarThumbGeometry(1_600, 800, 0)).toEqual({ offset: 0, size: 400 });
    const expanded = timelineScrollbarThumbGeometry(2_400, 800, 1_600);
    expect(expanded?.offset).toBeCloseTo(1600 / 3);
    expect(expanded?.size).toBeCloseTo(800 / 3);
    expect(timelineScrollbarThumbGeometry(10_000, 100, 20_000)).toEqual({ offset: 76, size: 24 });
    expect(timelineScrollbarThumbGeometry(800, 800, 0)).toBeNull();
  });

  it("shows disclosure fades only toward remaining scrollable content", () => {
    expect(timelineScrollOverflowState(0, 600, 300)).toEqual({
      scrollable: true,
      canScrollUp: false,
      canScrollDown: true,
    });
    expect(timelineScrollOverflowState(150, 600, 300)).toEqual({
      scrollable: true,
      canScrollUp: true,
      canScrollDown: true,
    });
    expect(timelineScrollOverflowState(300, 600, 300)).toEqual({
      scrollable: true,
      canScrollUp: true,
      canScrollDown: false,
    });
    expect(timelineScrollOverflowState(0, 300, 300)).toEqual({
      scrollable: false,
      canScrollUp: false,
      canScrollDown: false,
    });
  });

  it("deduplicates page rows and retains the selected page during eviction", () => {
    expect(mergeTimelineItems([item(1), item(2)], [item(2), item(3)]).map((entry) => entry.sequenceAnchor)).toEqual([1, 2, 3]);
    expect(evictTimelinePages([page(1, 10), page(11, 20), page(21, 30), page(31, 40), page(41, 50)], 25, 3).map((entry) => entry.items[0]?.sequenceAnchor)).toEqual([11, 21, 31]);
  });

  it("separates follow, near-end, and anchored intent and counts unread events", () => {
    expect(timelineScrollIntent(10, "anchored")).toBe("following");
    expect(timelineScrollIntent(100, "anchored")).toBe("near_end");
    expect(timelineScrollIntent(100, "following")).toBe("following");
    expect(timelineScrollIntent(300, "near_end")).toBe("anchored");
    expect(nextTimelineUnreadCount(4, 3, "anchored", false)).toBe(7);
    expect(nextTimelineUnreadCount(4, 3, "following", false)).toBe(0);
    expect(nextTimelineUnreadCount(4, 3, "anchored", true)).toBe(0);
  });
});
