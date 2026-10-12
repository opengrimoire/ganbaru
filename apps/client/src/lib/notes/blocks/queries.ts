import type {
  NotesBlock,
  NotesBlockType,
  NotesMediaBlockPayload,
  NotesRichText,
  NotesSyncedBlockPayload,
  NotesTableRowBlockPayload,
  NotesTextBlockPayload,
} from "$lib/notes/types";
import { richTextPlainText } from "$lib/notes/rich-text/core";
import { unsupportedBlockPlainText } from "$lib/notes/block-types/unsupported";
import {
  type NotesHeadingBlockType,
  createRichText,
} from "./payloads";

export function blockPlainText(block: NotesBlock): string {
  switch (block.type) {
    case "paragraph":
      return richTextPlainText(block.paragraph.rich_text);
    case "heading_1":
      return richTextPlainText(block.heading_1.rich_text);
    case "heading_2":
      return richTextPlainText(block.heading_2.rich_text);
    case "heading_3":
      return richTextPlainText(block.heading_3.rich_text);
    case "heading_4":
      return richTextPlainText(block.heading_4.rich_text);
    case "heading_5":
      return richTextPlainText(block.heading_5.rich_text);
    case "heading_6":
      return richTextPlainText(block.heading_6.rich_text);
    case "bulleted_list_item":
      return richTextPlainText(block.bulleted_list_item.rich_text);
    case "numbered_list_item":
      return richTextPlainText(block.numbered_list_item.rich_text);
    case "to_do":
      return richTextPlainText(block.to_do.rich_text);
    case "toggle":
      return richTextPlainText(block.toggle.rich_text);
    case "callout":
      return richTextPlainText(block.callout.rich_text);
    case "quote":
      return richTextPlainText(block.quote.rich_text);
    case "child_page":
      return block.child_page.title;
    case "child_database":
      return block.child_database.title;
    case "code":
      return richTextPlainText(block.code.rich_text);
    case "bookmark":
      return richTextPlainText(block.bookmark.caption) || block.bookmark.url;
    case "link_preview":
      return block.link_preview.url;
    case "synced_block":
      return syncedBlockPlainText(block.synced_block);
    case "template":
      return richTextPlainText(block.template.rich_text);
    case "button":
      return richTextPlainText(block.button.rich_text);
    case "tab":
      return "";
    case "embed":
      return block.embed.url;
    case "equation":
      return block.equation.expression;
    case "table_row":
      return tableRowPlainText(block.table_row);
    case "image":
      return mediaPayloadPlainText(block.image);
    case "video":
      return mediaPayloadPlainText(block.video);
    case "audio":
      return mediaPayloadPlainText(block.audio);
    case "file":
      return mediaPayloadPlainText(block.file);
    case "pdf":
      return mediaPayloadPlainText(block.pdf);
    case "breadcrumb":
    case "table_of_contents":
    case "column_list":
    case "column":
    case "table":
    case "divider":
      return "";
    case "unsupported":
      return unsupportedBlockPlainText(block.unsupported);
  }
}

export function tableCellPlainText(cell: readonly NotesRichText[]): string {
  return richTextPlainText(cell);
}

export function tableRowPlainText(row: NotesTableRowBlockPayload): string {
  return row.cells.map(tableCellPlainText).join("\t");
}

function mediaPayloadPlainText(payload: NotesMediaBlockPayload): string {
  const caption = payload.caption ? richTextPlainText(payload.caption) : "";
  const source = payload.type === "external"
    ? payload.external.url
    : payload.type === "file"
      ? payload.file.url
      : payload.file_upload.id;
  return [caption, payload.name ?? "", source]
    .map((part) => part.trim())
    .filter(Boolean)
    .join(" ");
}

function syncedBlockPlainText(payload: NotesSyncedBlockPayload): string {
  return payload.synced_from?.block_id ?? "Synced block";
}

export function isTextEditableBlock(type: NotesBlockType): boolean {
  return (
    type !== "divider"
    && type !== "child_page"
    && type !== "child_database"
    && type !== "breadcrumb"
    && type !== "table_of_contents"
    && type !== "column_list"
    && type !== "column"
    && type !== "table"
    && type !== "table_row"
    && type !== "tab"
    && type !== "image"
    && type !== "video"
    && type !== "audio"
    && type !== "file"
    && type !== "pdf"
    && type !== "bookmark"
    && type !== "link_preview"
    && type !== "synced_block"
    && type !== "embed"
    && type !== "equation"
    && type !== "unsupported"
  );
}

export function blockEditableRichText(block: NotesBlock): NotesRichText[] {
  switch (block.type) {
    case "paragraph":
      return block.paragraph.rich_text;
    case "heading_1":
      return block.heading_1.rich_text;
    case "heading_2":
      return block.heading_2.rich_text;
    case "heading_3":
      return block.heading_3.rich_text;
    case "heading_4":
      return block.heading_4.rich_text;
    case "heading_5":
      return block.heading_5.rich_text;
    case "heading_6":
      return block.heading_6.rich_text;
    case "bulleted_list_item":
      return block.bulleted_list_item.rich_text;
    case "numbered_list_item":
      return block.numbered_list_item.rich_text;
    case "to_do":
      return block.to_do.rich_text;
    case "toggle":
      return block.toggle.rich_text;
    case "callout":
      return block.callout.rich_text;
    case "quote":
      return block.quote.rich_text;
    case "code":
      return block.code.rich_text;
    case "template":
      return block.template.rich_text;
    case "button":
      return block.button.rich_text;
    default:
      return [createRichText(blockPlainText(block))];
  }
}

export function isHeadingBlockType(type: NotesBlockType): type is NotesHeadingBlockType {
  return type === "heading_1"
    || type === "heading_2"
    || type === "heading_3"
    || type === "heading_4"
    || type === "heading_5"
    || type === "heading_6";
}

function headingPayload(block: NotesBlock): NotesTextBlockPayload | null {
  if (block.type === "heading_1") return block.heading_1;
  if (block.type === "heading_2") return block.heading_2;
  if (block.type === "heading_3") return block.heading_3;
  if (block.type === "heading_4") return block.heading_4;
  if (block.type === "heading_5") return block.heading_5;
  if (block.type === "heading_6") return block.heading_6;
  return null;
}

export function headingIsToggleable(block: NotesBlock): boolean {
  return headingPayload(block)?.is_toggleable === true;
}

export function headingToggleOpen(block: NotesBlock): boolean {
  const payload = headingPayload(block);
  return payload?.is_toggleable !== true || payload.ganbaru_open !== false;
}

export function canBlockHaveChildren(blockOrType: NotesBlock | NotesBlockType): boolean {
  if (typeof blockOrType !== "string") {
    if (isHeadingBlockType(blockOrType.type)) return headingIsToggleable(blockOrType);
    if (blockOrType.type === "synced_block") {
      return blockOrType.synced_block.synced_from === null;
    }
    return canBlockHaveChildren(blockOrType.type);
  }
  const type = blockOrType;
  return [
    "paragraph",
    "bulleted_list_item",
    "numbered_list_item",
    "to_do",
    "toggle",
    "callout",
    "quote",
    "child_database",
    "column_list",
    "column",
    "table",
    "tab",
    "template",
    "button",
  ].includes(type);
}

/** Return explicit indentation relative to the structural parent. */
export function blockIndent(block: NotesBlock): number {
  const record: Record<string, unknown> = { ...block };
  const payload = record[block.type];
  if (typeof payload !== "object" || payload === null || !("ganbaru_indent" in payload)
    || payload.ganbaru_indent === undefined) return 0;
  const indent = payload.ganbaru_indent;
  if (typeof indent !== "number" || !Number.isSafeInteger(indent) || indent < 0) {
    throw new Error("Invalid Notes indentation");
  }
  return indent;
}
