import type { NotesBlockType } from "$lib/notes/types";

const SPLITTABLE_RICH_TEXT_BLOCK_TYPES = new Set<NotesBlockType>([
  "paragraph",
  "heading_1",
  "heading_2",
  "heading_3",
  "heading_4",
  "heading_5",
  "heading_6",
  "bulleted_list_item",
  "numbered_list_item",
  "to_do",
  "toggle",
  "callout",
  "quote",
]);

const SAME_TYPE_ENTER_BLOCK_TYPES = new Set<NotesBlockType>([
  "paragraph",
  "bulleted_list_item",
  "numbered_list_item",
  "to_do",
]);

const EMPTY_ENTER_RETURNS_PARAGRAPH_TYPES = new Set<NotesBlockType>([
  "bulleted_list_item",
  "numbered_list_item",
  "to_do",
  "quote",
]);

export function notesEnterSplitsRichTextBlock(type: NotesBlockType): boolean {
  return SPLITTABLE_RICH_TEXT_BLOCK_TYPES.has(type);
}

export function notesEnterSiblingBlockType(type: NotesBlockType): NotesBlockType {
  return SAME_TYPE_ENTER_BLOCK_TYPES.has(type) ? type : "paragraph";
}

export function notesEmptyEnterReturnsParagraph(type: NotesBlockType): boolean {
  return EMPTY_ENTER_RETURNS_PARAGRAPH_TYPES.has(type);
}
