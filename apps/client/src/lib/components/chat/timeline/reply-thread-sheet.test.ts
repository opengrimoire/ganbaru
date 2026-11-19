import { describe, expect, it } from "vitest";
import {
  REPLY_THREAD_SHEET_PEEK_RATIO,
  replyThreadSheetClaimsDrag,
  replyThreadSheetHeights,
  type ReplyThreadSheetDragStart,
} from "./reply-thread-sheet";

describe("replyThreadSheetHeights", () => {
  it("opens nearly full and keeps no summary strip", () => {
    const heights = replyThreadSheetHeights(800);
    expect(heights).toEqual({ dip: 0, peek: Math.round(800 * REPLY_THREAD_SHEET_PEEK_RATIO), full: 800 });
    expect(heights.peek).toBeLessThan(heights.full);
  });

  it("collapses to zero heights before the sheet is measured", () => {
    expect(replyThreadSheetHeights(0)).toEqual({ dip: 0, peek: 0, full: 0 });
  });
});

describe("replyThreadSheetClaimsDrag", () => {
  const content: ReplyThreadSheetDragStart = { onHandle: false, deltaY: 24, scrollTop: 0, innerControlKeeps: false };

  it("moves the sheet from the handle in either direction", () => {
    expect(replyThreadSheetClaimsDrag({ ...content, onHandle: true, deltaY: -24, scrollTop: 300 })).toBe(true);
    expect(replyThreadSheetClaimsDrag({ ...content, onHandle: true, innerControlKeeps: true })).toBe(true);
  });

  it("moves the sheet on a drag down from the top of the entries", () => {
    expect(replyThreadSheetClaimsDrag(content)).toBe(true);
  });

  it("leaves a drag up on the entries to scrolling", () => {
    expect(replyThreadSheetClaimsDrag({ ...content, deltaY: -24 })).toBe(false);
  });

  it("leaves a drag down to scrolling while the entries are scrolled", () => {
    expect(replyThreadSheetClaimsDrag({ ...content, scrollTop: 120 })).toBe(false);
  });

  it("leaves the drag to a nested control that keeps it", () => {
    expect(replyThreadSheetClaimsDrag({ ...content, innerControlKeeps: true })).toBe(false);
  });
});
