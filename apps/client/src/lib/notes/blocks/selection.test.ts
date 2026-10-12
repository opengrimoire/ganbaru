import { describe, expect, it } from "vitest";
import {
  notesBlockSelectionAfterClick,
  notesBlockSelectionAfterKeyboard,
  notesBlockSelectionForBlock,
  notesBlockSelectionPrunedToVisible,
  notesBlockSelectionRange,
  normalizeNotesSelectableBlockIds,
} from "./selection";

describe("notes block selection ranges", () => {
  it("selects an inclusive range in visible order", () => {
    expect(notesBlockSelectionRange(["a", "b", "c", "d"], "b", "d")).toEqual({
      anchorBlockId: "b",
      focusBlockId: "d",
      selectedBlockIds: ["b", "c", "d"],
    });
  });

  it("keeps anchor and focus while selecting reversed ranges", () => {
    expect(notesBlockSelectionRange(["a", "b", "c", "d"], "d", "b")).toEqual({
      anchorBlockId: "d",
      focusBlockId: "b",
      selectedBlockIds: ["b", "c", "d"],
    });
  });

  it("extends click selection from the existing anchor", () => {
    const current = notesBlockSelectionForBlock(["a", "b", "c", "d"], "b");

    expect(
      notesBlockSelectionAfterClick({
        blockIds: ["a", "b", "c", "d"],
        current,
        blockId: "d",
        extend: true,
      }),
    ).toEqual({
      anchorBlockId: "b",
      focusBlockId: "d",
      selectedBlockIds: ["b", "c", "d"],
    });
  });

  it("expands and shrinks ranges with keyboard focus movement", () => {
    const expanded = notesBlockSelectionAfterKeyboard({
      blockIds: ["a", "b", "c", "d"],
      current: notesBlockSelectionForBlock(["a", "b", "c", "d"], "b"),
      focusedBlockId: "b",
      direction: "next",
    });

    expect(expanded).toEqual({
      anchorBlockId: "b",
      focusBlockId: "c",
      selectedBlockIds: ["b", "c"],
    });
    expect(
      notesBlockSelectionAfterKeyboard({
        blockIds: ["a", "b", "c", "d"],
        current: expanded,
        focusedBlockId: "c",
        direction: "previous",
      }),
    ).toEqual({
      anchorBlockId: "b",
      focusBlockId: "b",
      selectedBlockIds: ["b"],
    });
  });

  it("starts keyboard selection from the focused block when no range exists", () => {
    expect(
      notesBlockSelectionAfterKeyboard({
        blockIds: ["a", "b", "c"],
        current: null,
        focusedBlockId: "b",
        direction: "next",
      }),
    ).toEqual({
      anchorBlockId: "b",
      focusBlockId: "c",
      selectedBlockIds: ["b", "c"],
    });
  });

  it("only selects blocks that are present in the visible render order", () => {
    const visibleIds = ["page-block", "callout", "callout-child", "table", "tab"];
    expect(notesBlockSelectionRange(visibleIds, "callout", "tab")).toEqual({
      anchorBlockId: "callout",
      focusBlockId: "tab",
      selectedBlockIds: ["callout", "callout-child", "table", "tab"],
    });
    expect(notesBlockSelectionRange(visibleIds, "hidden-table-row", "tab")).toBeNull();
    expect(notesBlockSelectionRange(visibleIds, "hidden-tab-label", "tab")).toBeNull();
  });

  it("drops stale selections when anchor or focus is no longer visible", () => {
    expect(
      notesBlockSelectionPrunedToVisible(
        ["a", "c"],
        {
          anchorBlockId: "a",
          focusBlockId: "b",
          selectedBlockIds: ["a", "b"],
        },
      ),
    ).toBeNull();
  });

  it("deduplicates visible ids while preserving first render order", () => {
    expect(normalizeNotesSelectableBlockIds(["a", "b", "a", "", "c"])).toEqual([
      "a",
      "b",
      "c",
    ]);
  });
});
