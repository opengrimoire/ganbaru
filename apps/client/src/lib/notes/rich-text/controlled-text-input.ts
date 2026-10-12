import type { NotesTextSelection } from "$lib/notes/editor/selection";
import {
  normalizeNotesSelectionForAtomicRanges,
  notesAtomicRangeEndingAt,
  notesAtomicRangeStartingAt,
  type NotesAtomicTextRange,
} from "./atomic-ranges";

export interface NotesControlledTextEditInput {
  inputType: string;
  data: string | null;
  text: string;
  selectionStart: number;
  selectionEnd: number;
  /** Mention spans that are inserted around and deleted whole, never split. */
  atomicRanges?: readonly NotesAtomicTextRange[];
}

export interface NotesControlledTextEdit {
  text: string;
  selection: NotesTextSelection;
}

function clampSelection(
  text: string,
  selectionStart: number,
  selectionEnd: number,
): NotesTextSelection {
  const max = text.length;
  const start = Math.min(Math.max(0, selectionStart), max);
  const end = Math.min(Math.max(0, selectionEnd), max);
  return {
    start: Math.min(start, end),
    end: Math.max(start, end),
  };
}

function previousTextOffset(text: string, offset: number): number {
  if (offset <= 0) return 0;
  const before = text.slice(0, offset);
  const previous = Array.from(before).at(-1);
  return Math.max(0, offset - (previous?.length ?? 1));
}

function nextTextOffset(text: string, offset: number): number {
  if (offset >= text.length) return text.length;
  const next = Array.from(text.slice(offset)).at(0);
  return Math.min(text.length, offset + (next?.length ?? 1));
}

function previousWordOffset(text: string, offset: number): number {
  const before = text.slice(0, offset);
  const withoutTrailingSpace = before.replace(/\s+$/u, "");
  const wordStart = withoutTrailingSpace.search(/[^\s]*$/u);
  return Math.max(0, wordStart);
}

function nextWordOffset(text: string, offset: number): number {
  const after = text.slice(offset);
  const leadingSpaceLength = after.match(/^\s*/u)?.[0].length ?? 0;
  const rest = after.slice(leadingSpaceLength);
  const wordLength = rest.match(/^[^\s]*/u)?.[0].length ?? 0;
  return Math.min(text.length, offset + leadingSpaceLength + wordLength);
}

function replaceRange(
  text: string,
  selection: NotesTextSelection,
  replacement: string,
): NotesControlledTextEdit {
  const nextText = `${text.slice(0, selection.start)}${replacement}${text.slice(selection.end)}`;
  const cursor = selection.start + replacement.length;
  return {
    text: nextText,
    selection: { start: cursor, end: cursor },
  };
}

function deleteRange(
  text: string,
  start: number,
  end: number,
): NotesControlledTextEdit {
  const selection = clampSelection(text, start, end);
  return replaceRange(text, selection, "");
}

function expandToAtomicRanges(
  ranges: readonly NotesAtomicTextRange[],
  start: number,
  end: number,
): NotesTextSelection {
  return normalizeNotesSelectionForAtomicRanges(ranges, { start, end });
}

/** Plan a browser beforeinput text edit without letting contenteditable mutate unmanaged DOM. */
export function planNotesControlledTextEdit(
  input: NotesControlledTextEditInput,
): NotesControlledTextEdit | null {
  const ranges = input.atomicRanges ?? [];
  const selection = normalizeNotesSelectionForAtomicRanges(
    ranges,
    clampSelection(input.text, input.selectionStart, input.selectionEnd),
  );
  const collapsed = selection.start === selection.end;

  if (input.inputType === "insertText") {
    if (input.data === null) return null;
    return replaceRange(input.text, selection, input.data);
  }

  if (input.inputType === "insertLineBreak" || input.inputType === "insertParagraph") {
    return replaceRange(input.text, selection, "\n");
  }

  if (input.inputType === "deleteContentBackward" || input.inputType === "deleteWordBackward") {
    if (!collapsed) return deleteRange(input.text, selection.start, selection.end);
    const token = notesAtomicRangeEndingAt(ranges, selection.start);
    if (token) return deleteRange(input.text, token.start, token.end);
    const from = input.inputType === "deleteWordBackward"
      ? previousWordOffset(input.text, selection.start)
      : previousTextOffset(input.text, selection.start);
    const range = expandToAtomicRanges(ranges, from, selection.start);
    return deleteRange(input.text, range.start, range.end);
  }

  if (input.inputType === "deleteContentForward" || input.inputType === "deleteWordForward") {
    if (!collapsed) return deleteRange(input.text, selection.start, selection.end);
    const token = notesAtomicRangeStartingAt(ranges, selection.start);
    if (token) return deleteRange(input.text, token.start, token.end);
    const to = input.inputType === "deleteWordForward"
      ? nextWordOffset(input.text, selection.start)
      : nextTextOffset(input.text, selection.start);
    const range = expandToAtomicRanges(ranges, selection.start, to);
    return deleteRange(input.text, range.start, range.end);
  }

  if (input.inputType === "deleteByCut" || input.inputType === "deleteContent") {
    if (collapsed) return null;
    return deleteRange(input.text, selection.start, selection.end);
  }

  return null;
}
