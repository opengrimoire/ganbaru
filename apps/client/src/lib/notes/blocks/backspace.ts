import {
  canBlockHaveChildren,
  isTextEditableBlock,
} from "./factory";
import {
  replaceRichTextRange,
  richTextPlainText,
} from "$lib/notes/rich-text/core";
import type { NotesBlock, NotesBlockType, NotesRichText } from "$lib/notes/types";

export function notesBackspaceCanMergeBlockTypes(
  sourceType: NotesBlockType,
  targetType: NotesBlockType,
): boolean {
  return isTextEditableBlock(sourceType) && isTextEditableBlock(targetType);
}

export function mergeRichTextForBackspace(
  targetRichText: readonly NotesRichText[],
  sourceRichText: readonly NotesRichText[],
): NotesRichText[] {
  const targetText = richTextPlainText(targetRichText);
  return replaceRichTextRange(targetRichText, targetText.length, targetText.length, sourceRichText);
}

export function notesParentCanAcceptBlockType(
  parentBlock: NotesBlock | null,
  childType: NotesBlockType,
): boolean {
  if (parentBlock?.type === "column_list") return childType === "column";
  if (parentBlock?.type === "table") return childType === "table_row";
  if (parentBlock?.type === "tab") return childType === "paragraph";
  if (childType === "column" || childType === "table_row") return false;
  return parentBlock === null || canBlockHaveChildren(parentBlock);
}

/** Remove ordinary text-block formatting before Backspace can merge or delete the item. */
export function notesBackspaceRemovesBlockFormat(type: NotesBlockType): boolean {
  return [
    "heading_1", "heading_2", "heading_3", "heading_4", "heading_5", "heading_6",
    "bulleted_list_item", "numbered_list_item", "to_do", "toggle", "quote", "callout",
  ].includes(type);
}
