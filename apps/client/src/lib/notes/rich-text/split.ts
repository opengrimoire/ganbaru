import {
  createTextRichText,
  richTextPlainText,
  richTextRangeSlice,
} from "./core";
import type { NotesRichText } from "$lib/notes/types";

export interface NotesRichTextBlockSplit {
  before: NotesRichText[];
  after: NotesRichText[];
}

function richTextObjectSafeSplitRange(
  richText: readonly NotesRichText[],
  selectionStart: number,
  selectionEnd: number,
): { start: number; end: number } {
  const plainText = richTextPlainText(richText);
  let safeStart = Math.max(0, Math.min(selectionStart, selectionEnd, plainText.length));
  let safeEnd = Math.max(
    safeStart,
    Math.min(Math.max(selectionStart, selectionEnd), plainText.length),
  );
  let cursor = 0;
  for (const item of richText) {
    const itemStart = cursor;
    const itemEnd = itemStart + item.plain_text.length;
    cursor = itemEnd;
    if (item.type === "text" || itemStart === itemEnd) continue;

    if (safeStart === safeEnd && safeStart > itemStart && safeStart < itemEnd) {
      const beforeLength = safeStart - itemStart;
      const afterLength = itemEnd - safeStart;
      const snapped = beforeLength < afterLength ? itemStart : itemEnd;
      return { start: snapped, end: snapped };
    }

    if (safeStart > itemStart && safeStart < itemEnd) safeStart = itemStart;
    if (safeEnd > itemStart && safeEnd < itemEnd) safeEnd = itemEnd;
  }
  return { start: safeStart, end: safeEnd };
}

export function splitRichTextForBlock(
  richText: readonly NotesRichText[],
  selectionStart: number,
  selectionEnd: number,
): NotesRichTextBlockSplit {
  const plainText = richTextPlainText(richText);
  const range = richTextObjectSafeSplitRange(richText, selectionStart, selectionEnd);
  const before = richTextRangeSlice(richText, 0, range.start);
  const after = richTextRangeSlice(richText, range.end, plainText.length);
  return {
    before: before.length > 0 ? before : [createTextRichText("")],
    after: after.length > 0 ? after : [createTextRichText("")],
  };
}
