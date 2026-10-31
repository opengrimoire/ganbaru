import { describe, expect, it } from "vitest";
import { createTextRichText } from "$lib/notes/rich-text/core";
import type { NotesBlockUpdate, NotesRichText } from "$lib/notes/types";
import { notesBlockContentEqual, notesCaretAfterRefresh, rebaseNotesBlockEdit } from "./block-rebase";

function text(content: string): NotesRichText[] {
  return [createTextRichText(content)];
}

function paragraph(content: string, color: "default" | "red" = "default"): NotesBlockUpdate {
  return { type: "paragraph", paragraph: { rich_text: text(content), color } };
}

function heading(content: string): NotesBlockUpdate {
  return { type: "heading_1", heading_1: { rich_text: text(content), color: "default" } };
}

function toggle(content: string, open: boolean): NotesBlockUpdate {
  return { type: "toggle", toggle: { rich_text: text(content), color: "default", ganbaru_open: open } };
}

describe("rebaseNotesBlockEdit", () => {
  it("keeps the local text when a remote edit to the block changed only its kind", () => {
    const result = rebaseNotesBlockEdit(paragraph("Draft"), paragraph("Draft and more"), heading("Draft"));
    expect(result).toEqual({ kind: "rebased", update: heading("Draft and more") });
  });

  it("takes the remote text when the editor changed only the kind", () => {
    const result = rebaseNotesBlockEdit(paragraph("Draft"), paragraph("Draft", "red"), paragraph("Remote draft"));
    expect(result).toEqual({ kind: "rebased", update: paragraph("Remote draft", "red") });
  });

  it("reports a conflict when both sides changed the text differently", () => {
    expect(rebaseNotesBlockEdit(paragraph("Draft"), paragraph("Mine"), paragraph("Theirs"))).toEqual({ kind: "conflict" });
  });

  it("does not report a conflict when both sides typed the same text", () => {
    expect(rebaseNotesBlockEdit(paragraph("Draft"), paragraph("Same"), paragraph("Same"))).toEqual({
      kind: "rebased",
      update: paragraph("Same"),
    });
  });

  it("reports a removed block when the canonical block is gone", () => {
    expect(rebaseNotesBlockEdit(paragraph("Draft"), paragraph("Mine"), null)).toEqual({ kind: "removed" });
  });

  it("keeps the local content when the block was only moved elsewhere", () => {
    // A move changes the parent and order key, which are not content, so the canonical content equals the base.
    expect(rebaseNotesBlockEdit(paragraph("Draft"), paragraph("Draft typed"), paragraph("Draft"))).toEqual({
      kind: "rebased",
      update: paragraph("Draft typed"),
    });
  });

  it("treats any text difference as a conflict when the base is unknown", () => {
    expect(rebaseNotesBlockEdit(undefined, paragraph("Mine"), paragraph("Theirs"))).toEqual({ kind: "conflict" });
    expect(rebaseNotesBlockEdit(undefined, paragraph("Same", "red"), paragraph("Same"))).toEqual({
      kind: "rebased",
      update: paragraph("Same", "red"),
    });
  });

  it("keeps the device-local view state of the editor", () => {
    const result = rebaseNotesBlockEdit(toggle("Draft", true), toggle("Draft typed", false), toggle("Draft", true));
    expect(result).toEqual({ kind: "rebased", update: toggle("Draft typed", false) });
  });

  it("gives the same update when reapplied on the same canonical content", () => {
    const canonical = heading("Draft");
    const first = rebaseNotesBlockEdit(paragraph("Draft"), paragraph("Draft and more"), canonical);
    if (first.kind !== "rebased") throw new Error("expected a rebased update");
    expect(rebaseNotesBlockEdit(paragraph("Draft"), first.update, canonical)).toEqual(first);
    expect(rebaseNotesBlockEdit(canonical, first.update, canonical)).toEqual(first);
  });

  it("compares content regardless of key order and view state", () => {
    const reordered: NotesBlockUpdate = { type: "paragraph", paragraph: { color: "default", rich_text: text("Same") } };
    expect(notesBlockContentEqual(paragraph("Same"), reordered)).toBe(true);
    expect(notesBlockContentEqual(toggle("Same", true), toggle("Same", false))).toBe(true);
    expect(notesBlockContentEqual(paragraph("Same"), paragraph("Other"))).toBe(false);
  });
});

describe("notesCaretAfterRefresh", () => {
  const length = (id: string) => id.length * 10;

  it("keeps the caret when blocks are inserted above it", () => {
    const caret = { blockId: "b", start: 3, end: 3 };
    expect(notesCaretAfterRefresh(caret, ["a", "b", "c"], ["new", "a", "b", "c"], length)).toEqual(caret);
  });

  it("keeps the caret when blocks are inserted below it", () => {
    const caret = { blockId: "b", start: 3, end: 3 };
    expect(notesCaretAfterRefresh(caret, ["a", "b", "c"], ["a", "b", "new", "c"], length)).toEqual(caret);
  });

  it("keeps a selected range when its block remains", () => {
    const range = { blockId: "b", start: 2, end: 7 };
    expect(notesCaretAfterRefresh(range, ["a", "b"], ["a", "b"], length)).toEqual(range);
  });

  it("clamps a selected range to the block's shorter text", () => {
    expect(notesCaretAfterRefresh({ blockId: "b", start: 4, end: 15 }, ["b"], ["b"], () => 6)).toEqual({ blockId: "b", start: 4, end: 6 });
  });

  it("collapses at the end of the nearest earlier block when its block was deleted", () => {
    expect(notesCaretAfterRefresh({ blockId: "c", start: 1, end: 5 }, ["a", "bb", "c"], ["a", "bb"], length)).toEqual({
      blockId: "bb",
      start: 20,
      end: 20,
    });
  });

  it("moves to the start of the next block when nothing before it remains", () => {
    expect(notesCaretAfterRefresh({ blockId: "a", start: 1, end: 1 }, ["a", "b"], ["b"], length)).toEqual({ blockId: "b", start: 0, end: 0 });
  });

  it("follows the block when a remote move reorders the page", () => {
    const caret = { blockId: "a", start: 2, end: 2 };
    expect(notesCaretAfterRefresh(caret, ["a", "b", "c"], ["b", "c", "a"], length)).toEqual(caret);
  });

  it("is null when no block remains or there was no caret", () => {
    expect(notesCaretAfterRefresh({ blockId: "a", start: 0, end: 0 }, ["a"], [], length)).toBeNull();
    expect(notesCaretAfterRefresh(null, ["a"], ["a"], length)).toBeNull();
  });
});
