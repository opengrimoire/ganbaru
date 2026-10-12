import { describe, expect, it } from "vitest";
import {
  nextNotesFocusRequest,
  notesBackgroundPointerTargetsDocumentEnd,
  notesDocumentEndFocusIsCurrent,
  planNotesDeletedBlockFocus,
  planNotesInsertedBlockFocus,
  planNotesPageLoadFocus,
} from "./focus";

describe("notes editor focus helpers", () => {
  it("preserves scrolling for pointer focus without disabling the next keyboard reveal", () => {
    const clicked = nextNotesFocusRequest({ blockId: "a", requestId: 2, selection: null }, "b", null, true);
    expect(clicked).toEqual({ blockId: "b", requestId: 3, selection: null, preventScroll: true });
    const next = nextNotesFocusRequest(clicked, "c", { start: 0, end: 0 });
    expect(next).toEqual({ blockId: "c", requestId: 4, selection: { start: 0, end: 0 } });
  });

  it("treats only background pointers below the final row as the document end", () => {
    expect(notesBackgroundPointerTargetsDocumentEnd({
      pointerY: 240,
      lastRowBottom: 220,
      targetInsideRow: false,
    })).toBe(true);
    expect(notesBackgroundPointerTargetsDocumentEnd({
      pointerY: 200,
      lastRowBottom: 220,
      targetInsideRow: false,
    })).toBe(false);
    expect(notesBackgroundPointerTargetsDocumentEnd({
      pointerY: 240,
      lastRowBottom: 220,
      targetInsideRow: true,
    })).toBe(false);
  });

  it("does not refocus a caret that is already at the document end", () => {
    expect(notesDocumentEndFocusIsCurrent({
      activeBlockId: "last",
      lastBlockId: "last",
      selection: { start: 4, end: 4 },
      textLength: 4,
    })).toBe(true);
    expect(notesDocumentEndFocusIsCurrent({
      activeBlockId: "last",
      lastBlockId: "last",
      selection: { start: 2, end: 2 },
      textLength: 4,
    })).toBe(false);
    expect(notesDocumentEndFocusIsCurrent({
      activeBlockId: "other",
      lastBlockId: "last",
      selection: { start: 4, end: 4 },
      textLength: 4,
    })).toBe(false);
  });

  it("increments the focus request token for each requested block", () => {
    expect(nextNotesFocusRequest({ blockId: "a", requestId: 2, selection: null }, "b")).toEqual({
      blockId: "b",
      requestId: 3,
      selection: null,
    });
  });

  it("keeps focus clearing observable through the request token", () => {
    expect(nextNotesFocusRequest({ blockId: "a", requestId: 2, selection: null }, null)).toEqual({
      blockId: null,
      requestId: 3,
      selection: null,
    });
  });

  it("carries an optional requested text selection", () => {
    expect(nextNotesFocusRequest(
      { blockId: "a", requestId: 2, selection: null },
      "b",
      { start: 3, end: 3 },
    )).toEqual({
      blockId: "b",
      requestId: 3,
      selection: { start: 3, end: 3 },
    });
  });

  it("focuses a requested visible block when loading a link", () => {
    expect(planNotesPageLoadFocus(["a", "b", "c"], "b")).toBe("b");
  });

  it("focuses the first visible block when switching pages without a block target", () => {
    expect(planNotesPageLoadFocus(["first", "second"])).toBe("first");
  });

  it("falls back to the first visible block when a requested link target is not rendered", () => {
    expect(planNotesPageLoadFocus(["first", "second"], "hidden")).toBe("first");
  });

  it("returns no page focus when no block is visible", () => {
    expect(planNotesPageLoadFocus([], "missing")).toBeNull();
  });

  it("focuses the first inserted block for create, split, duplicate, template, and button flows", () => {
    expect(planNotesInsertedBlockFocus([null, "created", "next"], "trigger")).toBe("created");
  });

  it("falls back to the trigger when template or button insertion creates no focusable block", () => {
    expect(planNotesInsertedBlockFocus([], "trigger")).toBe("trigger");
  });

  it("focuses the previous surviving block after deletion or move away", () => {
    expect(
      planNotesDeletedBlockFocus({
        visibleBlockIds: ["a", "b", "child", "c"],
        removedBlockIds: ["b", "child"],
        firstRemovedBlockId: "b",
      }),
    ).toBe("a");
  });

  it("focuses the next surviving block when the first visible block is deleted or moved away", () => {
    expect(
      planNotesDeletedBlockFocus({
        visibleBlockIds: ["a", "child", "b"],
        removedBlockIds: ["a", "child"],
        firstRemovedBlockId: "a",
      }),
    ).toBe("b");
  });

  it("clears focus when every visible block is deleted or moved away", () => {
    expect(
      planNotesDeletedBlockFocus({
        visibleBlockIds: ["a", "b"],
        removedBlockIds: ["a", "b"],
        firstRemovedBlockId: "a",
      }),
    ).toBeNull();
  });
});
