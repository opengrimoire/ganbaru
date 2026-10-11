import type { NotesRichText } from "$lib/notes/contracts/core";
import type { NotesDirectionalTextSelection, NotesTextSelection } from "$lib/notes/editor/selection";

/** Plain-text span that the editor treats as one indivisible token. */
export interface NotesAtomicTextRange {
  start: number;
  end: number;
}

/** Collect the plain-text spans of every mention, which edit and move as single tokens. */
export function notesAtomicTextRanges(richText: readonly NotesRichText[]): NotesAtomicTextRange[] {
  const ranges: NotesAtomicTextRange[] = [];
  let cursor = 0;
  for (const item of richText) {
    const start = cursor;
    cursor += item.plain_text.length;
    if (item.type === "mention" && cursor > start) ranges.push({ start, end: cursor });
  }
  return ranges;
}

/** Find the token whose interior strictly contains the offset. */
export function notesAtomicRangeInside(
  ranges: readonly NotesAtomicTextRange[],
  offset: number,
): NotesAtomicTextRange | null {
  return ranges.find((range) => range.start < offset && offset < range.end) ?? null;
}

/** Move an offset inside a token to its nearest edge, preferring the end on a tie. */
export function snapNotesOffsetToAtomicEdge(
  ranges: readonly NotesAtomicTextRange[],
  offset: number,
): number {
  const range = notesAtomicRangeInside(ranges, offset);
  if (!range) return offset;
  return offset - range.start < range.end - offset ? range.start : range.end;
}

/**
 * Normalize a selection so no token is partly selected: a caret inside a token snaps to its
 * nearest edge, and a range that cuts through a token grows to cover all of it.
 */
export function normalizeNotesSelectionForAtomicRanges(
  ranges: readonly NotesAtomicTextRange[],
  selection: NotesTextSelection,
): NotesTextSelection {
  if (selection.start === selection.end) {
    const offset = snapNotesOffsetToAtomicEdge(ranges, selection.start);
    return { start: offset, end: offset };
  }
  return {
    start: notesAtomicRangeInside(ranges, selection.start)?.start ?? selection.start,
    end: notesAtomicRangeInside(ranges, selection.end)?.end ?? selection.end,
  };
}

/** Return the token that ends exactly at the caret, if any. */
export function notesAtomicRangeEndingAt(
  ranges: readonly NotesAtomicTextRange[],
  offset: number,
): NotesAtomicTextRange | null {
  return ranges.find((range) => range.end === offset) ?? null;
}

/** Return the token that starts exactly at the caret, if any. */
export function notesAtomicRangeStartingAt(
  ranges: readonly NotesAtomicTextRange[],
  offset: number,
): NotesAtomicTextRange | null {
  return ranges.find((range) => range.start === offset) ?? null;
}

/**
 * Snap a directional selection so no token is partly selected while its direction survives:
 * a caret moves to the nearest token edge, and each end of a range moves outward.
 */
export function snapNotesDirectionalSelection(
  ranges: readonly NotesAtomicTextRange[],
  selection: NotesDirectionalTextSelection,
): NotesDirectionalTextSelection {
  if (selection.anchor === selection.focus) {
    const offset = snapNotesOffsetToAtomicEdge(ranges, selection.anchor);
    return { anchor: offset, focus: offset };
  }
  const forward = selection.anchor < selection.focus;
  const outward = (offset: number, towardEnd: boolean): number => {
    const range = notesAtomicRangeInside(ranges, offset);
    if (!range) return offset;
    return towardEnd ? range.end : range.start;
  };
  return { anchor: outward(selection.anchor, !forward), focus: outward(selection.focus, forward) };
}
