import {
  applyRichTextAnnotations,
  replaceRichTextRange,
  richTextPlainText,
  richTextRangeSlice,
  type NotesRichTextAnnotationPatch,
} from "./core";
import type { NotesRichText } from "$lib/notes/types";

export type NotesMarkdownInlineShortcutName =
  | "bold"
  | "italic"
  | "code"
  | "strikethrough";

export interface NotesMarkdownInlineShortcutPlan {
  shortcut: NotesMarkdownInlineShortcutName;
  richText: NotesRichText[];
  cursor: number;
  range: {
    start: number;
    end: number;
  };
}

interface NotesMarkdownInlineShortcutDefinition {
  name: NotesMarkdownInlineShortcutName;
  delimiter: string;
  annotation: NotesRichTextAnnotationPatch;
}

interface NotesMarkdownInlineShortcutMatch {
  definition: NotesMarkdownInlineShortcutDefinition;
  openingStart: number;
  openingEnd: number;
  contentStart: number;
  contentEnd: number;
  closingStart: number;
  closingEnd: number;
}

const INLINE_SHORTCUTS: readonly NotesMarkdownInlineShortcutDefinition[] = [
  { name: "bold", delimiter: "**", annotation: { bold: true } },
  { name: "italic", delimiter: "*", annotation: { italic: true } },
  { name: "code", delimiter: "`", annotation: { code: true } },
  { name: "strikethrough", delimiter: "~", annotation: { strikethrough: true } },
];

function clampSelectionOffset(offset: number, textLength: number): number {
  return Math.max(0, Math.min(offset, textLength));
}

function delimiterTouchesCharacter(
  text: string,
  start: number,
  end: number,
  character: string,
): boolean {
  return text.at(start - 1) === character || text.at(end) === character;
}

function rangeContainsOnlyText(
  richText: readonly NotesRichText[],
  start: number,
  end: number,
): boolean {
  if (start === end) return true;
  let coveredLength = 0;
  let cursor = 0;
  for (const item of richText) {
    const itemStart = cursor;
    const itemEnd = itemStart + item.plain_text.length;
    cursor = itemEnd;
    if (itemEnd <= start || itemStart >= end) continue;
    if (item.type !== "text") return false;
    coveredLength += Math.min(end, itemEnd) - Math.max(start, itemStart);
  }
  return coveredLength === end - start;
}

function findInlineShortcutMatch(
  text: string,
  cursor: number,
  definition: NotesMarkdownInlineShortcutDefinition,
): NotesMarkdownInlineShortcutMatch | null {
  const { delimiter } = definition;
  const closingStart = cursor - delimiter.length;
  if (closingStart < delimiter.length || text.slice(closingStart, cursor) !== delimiter) {
    return null;
  }
  const openingStart = text.lastIndexOf(delimiter, closingStart - 1);
  if (openingStart < 0) return null;
  const openingEnd = openingStart + delimiter.length;
  const contentStart = openingEnd;
  const contentEnd = closingStart;
  if (contentStart >= contentEnd) return null;
  const content = text.slice(contentStart, contentEnd);
  if (!content.trim() || content.includes("\n")) return null;
  if (
    delimiter.includes("*")
    && (
      delimiterTouchesCharacter(text, openingStart, openingEnd, "*")
      || delimiterTouchesCharacter(text, closingStart, cursor, "*")
    )
  ) {
    return null;
  }
  return {
    definition,
    openingStart,
    openingEnd,
    contentStart,
    contentEnd,
    closingStart,
    closingEnd: cursor,
  };
}

/**
 * Plan a Notion-style inline Markdown shortcut conversion after normal typing.
 */
export function planNotesMarkdownInlineShortcutConversion(
  richText: readonly NotesRichText[],
  selectionStart: number,
  selectionEnd: number,
): NotesMarkdownInlineShortcutPlan | null {
  const text = richTextPlainText(richText);
  const start = clampSelectionOffset(selectionStart, text.length);
  const end = clampSelectionOffset(selectionEnd, text.length);
  if (start !== end) return null;

  for (const definition of INLINE_SHORTCUTS) {
    const match = findInlineShortcutMatch(text, end, definition);
    if (!match) continue;
    if (
      !rangeContainsOnlyText(richText, match.openingStart, match.openingEnd)
      || !rangeContainsOnlyText(richText, match.contentStart, match.contentEnd)
      || !rangeContainsOnlyText(richText, match.closingStart, match.closingEnd)
    ) {
      continue;
    }
    const contentLength = match.contentEnd - match.contentStart;
    const content = richTextRangeSlice(richText, match.contentStart, match.contentEnd);
    const annotatedContent = applyRichTextAnnotations(
      content,
      0,
      contentLength,
      match.definition.annotation,
    );
    return {
      shortcut: match.definition.name,
      richText: replaceRichTextRange(
        richText,
        match.openingStart,
        match.closingEnd,
        annotatedContent,
      ),
      cursor: match.openingStart + contentLength,
      range: {
        start: match.openingStart,
        end: match.closingEnd,
      },
    };
  }

  return null;
}
