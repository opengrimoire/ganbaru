import { isNotesCatalogRegisteredBlockType } from "$lib/notes/blocks/catalog";
import { NOTES_BUTTON_INSERT_POSITIONS, NOTES_COLORS } from "../contracts/core";
import type { NotesBlock, NotesBlockType, NotesButtonAction, NotesButtonBlockPayload, NotesButtonInsertPosition, NotesCalloutBlockPayload, NotesCodeBlockPayload, NotesColor, NotesColumnBlockPayload, NotesEquationBlockPayload, NotesParent, NotesSyncedBlockPayload, NotesTabBlockPayload, NotesTableBlockPayload, NotesTableOfContentsBlockPayload, NotesTableRowBlockPayload, NotesTextBlockPayload, NotesTodoBlockPayload, NotesToggleBlockPayload, NotesUnsupportedBlockPayload } from "../contracts/core";
import type { NotesDatabaseBoardRowOpenMode, NotesDatabaseCalendarRowOpenMode, NotesDatabaseGalleryCardSize, NotesDatabaseGalleryRowOpenMode, NotesDatabaseListRowOpenMode, NotesDatabaseTableRowOpenMode, NotesDatabaseTimelineRowOpenMode } from "$lib/notes/contracts/database";
import type { NotesPaginatedBlockList } from "../contracts/workspace";
import { parseBookmarkPayload, parseEmbedPayload, parseLinkPreviewPayload, parseMediaPayload, parseNullableNotesIcon } from "./assets";
import { parseChildDatabasePayload } from "./database";
import { UUID_PATTERN, readBoolean, readDisplayString, readInteger, readNotesColor, readNullableString, readRecord, readString } from "./readers";
import { parseNotesRichTextArray } from "./rich-text";
import { parseChildPagePayload, parseTemplatePayload } from "./workspace";

export function isNotesBlockType(value: unknown): value is NotesBlockType {
  return isNotesCatalogRegisteredBlockType(value);
}

export function isNotesColor(value: unknown): value is NotesColor {
  return typeof value === "string" && NOTES_COLORS.includes(value as NotesColor);
}

export function parseNotesParent(value: unknown): NotesParent {
  const record = readRecord(value, "parent");
  const type = readString(record.type, "parent.type");
  if (type === "workspace") {
    if (record.workspace !== true) throw new Error("parent.workspace must be true");
    return { type: "workspace", workspace: true };
  }
  if (type === "page_id") {
    return { type, page_id: readString(record.page_id, "parent.page_id") };
  }
  if (type === "block_id") {
    return { type, block_id: readString(record.block_id, "parent.block_id") };
  }
  if (type === "data_source_id") {
    return {
      type,
      data_source_id: readString(record.data_source_id, "parent.data_source_id"),
    };
  }
  throw new Error(`unsupported parent type: ${type}`);
}

function parseTextPayload(value: unknown, label: string): NotesTextBlockPayload {
  const record = readRecord(value, label);
  return {
    rich_text: parseNotesRichTextArray(record.rich_text, `${label}.rich_text`),
    ...(record.color === undefined ? {} : { color: readNotesColor(record.color, `${label}.color`) }),
    ...(record.is_toggleable === undefined
      ? {}
      : { is_toggleable: readBoolean(record.is_toggleable, `${label}.is_toggleable`) }),
    ...(record.ganbaru_open === undefined
      ? {}
      : { ganbaru_open: readBoolean(record.ganbaru_open, `${label}.ganbaru_open`) }),
    ...(record.icon === undefined ? {} : { icon: parseNullableNotesIcon(record.icon, `${label}.icon`) }),
  };
}

function parseEmptyObjectPayload(value: unknown, label: string): Record<string, never> {
  const record = readRecord(value, label);
  if (Object.keys(record).length > 0) throw new Error(`${label} must be an empty object`);
  return {};
}

function isNotesButtonInsertPosition(value: unknown): value is NotesButtonInsertPosition {
  return (
    typeof value === "string"
    && NOTES_BUTTON_INSERT_POSITIONS.includes(value as NotesButtonInsertPosition)
  );
}

function parseButtonAction(value: unknown, label: string): NotesButtonAction {
  const record = readRecord(value, label);
  const type = readString(record.type, `${label}.type`);
  if (type !== "insert_blocks") throw new Error(`${label}.type must be insert_blocks`);
  const source = readString(record.source, `${label}.source`);
  if (source !== "children") throw new Error(`${label}.source must be children`);
  const position = readString(record.position, `${label}.position`);
  if (!isNotesButtonInsertPosition(position)) {
    throw new Error(`${label}.position must be a supported button insert position`);
  }
  return { type, source, position };
}

function parseButtonActions(value: unknown, label: string): NotesButtonAction[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  if (value.length < 1 || value.length > 10) {
    throw new Error(`${label} must include between 1 and 10 actions`);
  }
  return value.map((item, index) => parseButtonAction(item, `${label}[${index}]`));
}

function parseButtonPayload(value: unknown, label: string): NotesButtonBlockPayload {
  const record = readRecord(value, label);
  if (record.children !== undefined) {
    throw new Error(`${label}.children must be stored as child blocks`);
  }
  return {
    rich_text: parseNotesRichTextArray(record.rich_text, `${label}.rich_text`),
    icon: parseNullableNotesIcon(record.icon, `${label}.icon`),
    actions: parseButtonActions(record.actions, `${label}.actions`),
  };
}

function parseTodoPayload(value: unknown, label: string): NotesTodoBlockPayload {
  const record = readRecord(value, label);
  return {
    ...parseTextPayload(record, label),
    checked: readBoolean(record.checked, `${label}.checked`),
  };
}

function parseTogglePayload(value: unknown, label: string): NotesToggleBlockPayload {
  const record = readRecord(value, label);
  return {
    ...parseTextPayload(record, label),
    ...(record.ganbaru_open === undefined
      ? {}
      : { ganbaru_open: readBoolean(record.ganbaru_open, `${label}.ganbaru_open`) }),
  };
}

function parseCalloutPayload(value: unknown, label: string): NotesCalloutBlockPayload {
  const record = readRecord(value, label);
  return {
    ...parseTextPayload(record, label),
    icon: parseNullableNotesIcon(record.icon, `${label}.icon`),
  };
}

function parseTableOfContentsPayload(
  value: unknown,
  label: string,
): NotesTableOfContentsBlockPayload {
  const record = readRecord(value, label);
  return {
    ...(record.color === undefined ? {} : { color: readNotesColor(record.color, `${label}.color`) }),
  };
}

function parseColumnPayload(value: unknown, label: string): NotesColumnBlockPayload {
  const record = readRecord(value, label);
  if (record.width_ratio === undefined) return {};
  if (typeof record.width_ratio !== "number") {
    throw new Error(`${label}.width_ratio must be a number`);
  }
  if (record.width_ratio <= 0 || record.width_ratio > 1) {
    throw new Error(`${label}.width_ratio must be greater than 0 and no more than 1`);
  }
  return { width_ratio: record.width_ratio };
}

function parseTablePayload(value: unknown, label: string): NotesTableBlockPayload {
  const record = readRecord(value, label);
  const tableWidth = readInteger(record.table_width, `${label}.table_width`);
  if (tableWidth < 1 || tableWidth > 100) {
    throw new Error(`${label}.table_width must be between 1 and 100`);
  }
  return {
    table_width: tableWidth,
    has_column_header: readBoolean(record.has_column_header, `${label}.has_column_header`),
    has_row_header: readBoolean(record.has_row_header, `${label}.has_row_header`),
  };
}

function parseTableRowPayload(value: unknown, label: string): NotesTableRowBlockPayload {
  const record = readRecord(value, label);
  if (!Array.isArray(record.cells)) throw new Error(`${label}.cells must be an array`);
  if (record.cells.length < 1 || record.cells.length > 100) {
    throw new Error(`${label}.cells must include between 1 and 100 cells`);
  }
  return {
    cells: record.cells.map((cell, index) =>
      parseNotesRichTextArray(cell, `${label}.cells[${index}]`)
    ),
  };
}

function parseTabPayload(value: unknown, label: string): NotesTabBlockPayload {
  const record = { ...readRecord(value, label) };
  delete record.ganbaru_indent;
  return parseEmptyObjectPayload(record, label);
}

function parseSyncedBlockPayload(value: unknown, label: string): NotesSyncedBlockPayload {
  const record = readRecord(value, label);
  if (!("synced_from" in record)) {
    throw new Error(`${label}.synced_from is required`);
  }
  if (record.synced_from === null) return { synced_from: null };
  const syncedFrom = readRecord(record.synced_from, `${label}.synced_from`);
  const type = readString(syncedFrom.type, `${label}.synced_from.type`);
  if (type !== "block_id") {
    throw new Error(`${label}.synced_from.type must be block_id`);
  }
  const blockId = readString(syncedFrom.block_id, `${label}.synced_from.block_id`);
  if (!UUID_PATTERN.test(blockId)) {
    throw new Error(`${label}.synced_from.block_id must be a UUID`);
  }
  return {
    synced_from: {
      type,
      block_id: blockId,
    },
  };
}

function parseEquationPayload(value: unknown, label: string): NotesEquationBlockPayload {
  const record = readRecord(value, label);
  return {
    expression: readString(record.expression, `${label}.expression`),
  };
}

function parseUnsupportedPayload(value: unknown, label: string): NotesUnsupportedBlockPayload {
  const record = readRecord(value, label);
  const payload: NotesUnsupportedBlockPayload = { ...record };
  if (record.block_type !== undefined) {
    payload.block_type = readDisplayString(record.block_type, `${label}.block_type`);
  }
  if (record.source_type !== undefined) {
    payload.source_type = readDisplayString(record.source_type, `${label}.source_type`);
  }
  if (record.raw !== undefined) {
    payload.raw = readRecord(record.raw, `${label}.raw`);
  }
  if (record.warnings !== undefined) {
    if (!Array.isArray(record.warnings)) {
      throw new Error(`${label}.warnings must be an array`);
    }
    payload.warnings = record.warnings.map((warning, index) =>
      readDisplayString(warning, `${label}.warnings[${index}]`)
    );
  }
  return payload;
}

function parseCodePayload(value: unknown, label: string): NotesCodeBlockPayload {
  const record = readRecord(value, label);
  return {
    rich_text: parseNotesRichTextArray(record.rich_text, `${label}.rich_text`),
    caption: parseNotesRichTextArray(record.caption, `${label}.caption`),
    language: readString(record.language, `${label}.language`),
  };
}

export function isNotesTableRowOpenMode(value: unknown): value is NotesDatabaseTableRowOpenMode {
  return value === "full_page" || value === "side_panel";
}

export function isNotesBoardRowOpenMode(value: unknown): value is NotesDatabaseBoardRowOpenMode {
  return value === "full_page" || value === "side_panel";
}

export function isNotesGalleryRowOpenMode(value: unknown): value is NotesDatabaseGalleryRowOpenMode {
  return value === "full_page" || value === "side_panel";
}

export function isNotesGalleryCardSize(value: unknown): value is NotesDatabaseGalleryCardSize {
  return value === "small" || value === "medium" || value === "large";
}

export function isNotesListRowOpenMode(value: unknown): value is NotesDatabaseListRowOpenMode {
  return value === "full_page" || value === "side_panel";
}

export function isNotesCalendarRowOpenMode(value: unknown): value is NotesDatabaseCalendarRowOpenMode {
  return value === "full_page" || value === "side_panel";
}

export function isNotesTimelineRowOpenMode(value: unknown): value is NotesDatabaseTimelineRowOpenMode {
  return value === "full_page" || value === "side_panel";
}

/** Validate content and retain shared indentation for text and embedded blocks. */
export function parseNotesBlock(value: unknown): NotesBlock {
  const block = parseNotesBlockContent(value);
  const record = readRecord(value, "block");
  const payload = readRecord(record[block.type], `block.${block.type}`);
  if (payload.ganbaru_indent === undefined) return block;
  const indent = parseNotesIndent(payload.ganbaru_indent);
  const parsed: Record<string, unknown> = { ...block };
  return { ...block, [block.type]: { ...readRecord(parsed[block.type], `block.${block.type}`), ganbaru_indent: indent } };
}

/** Validate block content before attaching its shared layout metadata. */
function parseNotesBlockContent(value: unknown): NotesBlock {
  const record = readRecord(value, "block");
  if (record.object !== "block") throw new Error("block.object must be block");
  const type = readString(record.type, "block.type");
  if (!isNotesBlockType(type)) throw new Error(`unsupported block type: ${type}`);
  const revision = record.edit_revision;
  if (revision !== undefined && (typeof revision !== "string" || !/^[a-f0-9]{64}$/u.test(revision))) {
    throw new Error("block.edit_revision must be a canonical revision");
  }
  const base = {
    object: "block" as const,
    id: readString(record.id, "block.id"),
    edit_revision: revision as string | undefined,
    parent: parseNotesParent(record.parent),
    created_time: readString(record.created_time, "block.created_time"),
    last_edited_time: readString(record.last_edited_time, "block.last_edited_time"),
    has_children: readBoolean(record.has_children, "block.has_children"),
    in_trash: readBoolean(record.in_trash, "block.in_trash"),
    archived: typeof record.archived === "boolean" ? record.archived : undefined,
    source_provider: readNullableString(record.source_provider, "block.source_provider"),
    source_object_id: readNullableString(record.source_object_id, "block.source_object_id"),
    source_last_edited_time: readNullableString(
      record.source_last_edited_time,
      "block.source_last_edited_time",
    ),
  };
  switch (type) {
    case "paragraph":
      return { ...base, type, paragraph: parseTextPayload(record.paragraph, "block.paragraph") };
    case "heading_1":
      return { ...base, type, heading_1: parseTextPayload(record.heading_1, "block.heading_1") };
    case "heading_2":
      return { ...base, type, heading_2: parseTextPayload(record.heading_2, "block.heading_2") };
    case "heading_3":
      return { ...base, type, heading_3: parseTextPayload(record.heading_3, "block.heading_3") };
    case "heading_4":
      return { ...base, type, heading_4: parseTextPayload(record.heading_4, "block.heading_4") };
    case "heading_5":
      return { ...base, type, heading_5: parseTextPayload(record.heading_5, "block.heading_5") };
    case "heading_6":
      return { ...base, type, heading_6: parseTextPayload(record.heading_6, "block.heading_6") };
    case "bulleted_list_item":
      return {
        ...base,
        type,
        bulleted_list_item: parseTextPayload(
          record.bulleted_list_item,
          "block.bulleted_list_item",
        ),
      };
    case "numbered_list_item":
      return {
        ...base,
        type,
        numbered_list_item: parseTextPayload(
          record.numbered_list_item,
          "block.numbered_list_item",
        ),
      };
    case "to_do":
      return { ...base, type, to_do: parseTodoPayload(record.to_do, "block.to_do") };
    case "toggle":
      return { ...base, type, toggle: parseTogglePayload(record.toggle, "block.toggle") };
    case "callout":
      return { ...base, type, callout: parseCalloutPayload(record.callout, "block.callout") };
    case "quote":
      return { ...base, type, quote: parseTextPayload(record.quote, "block.quote") };
    case "child_page":
      return {
        ...base,
        type,
        child_page: parseChildPagePayload(record.child_page, "block.child_page"),
      };
    case "child_database":
      return {
        ...base,
        type,
        child_database: parseChildDatabasePayload(
          record.child_database,
          "block.child_database",
        ),
      };
    case "breadcrumb":
      return { ...base, type, breadcrumb: readRecord(record.breadcrumb, "block.breadcrumb") };
    case "table_of_contents":
      return {
        ...base,
        type,
        table_of_contents: parseTableOfContentsPayload(
          record.table_of_contents,
          "block.table_of_contents",
        ),
      };
    case "column_list":
      return { ...base, type, column_list: readRecord(record.column_list, "block.column_list") };
    case "column":
      return { ...base, type, column: parseColumnPayload(record.column, "block.column") };
    case "table":
      return { ...base, type, table: parseTablePayload(record.table, "block.table") };
    case "table_row":
      return {
        ...base,
        type,
        table_row: parseTableRowPayload(record.table_row, "block.table_row"),
      };
    case "tab":
      return { ...base, type, tab: parseTabPayload(record.tab, "block.tab") };
    case "image":
      return { ...base, type, image: parseMediaPayload(record.image, "block.image", type) };
    case "video":
      return { ...base, type, video: parseMediaPayload(record.video, "block.video", type) };
    case "audio":
      return { ...base, type, audio: parseMediaPayload(record.audio, "block.audio", type) };
    case "file":
      return { ...base, type, file: parseMediaPayload(record.file, "block.file", type) };
    case "pdf":
      return { ...base, type, pdf: parseMediaPayload(record.pdf, "block.pdf", type) };
    case "bookmark":
      return { ...base, type, bookmark: parseBookmarkPayload(record.bookmark, "block.bookmark") };
    case "link_preview":
      return {
        ...base,
        type,
        link_preview: parseLinkPreviewPayload(record.link_preview, "block.link_preview"),
      };
    case "synced_block":
      return {
        ...base,
        type,
        synced_block: parseSyncedBlockPayload(record.synced_block, "block.synced_block"),
      };
    case "template":
      return { ...base, type, template: parseTemplatePayload(record.template, "block.template") };
    case "button":
      return { ...base, type, button: parseButtonPayload(record.button, "block.button") };
    case "embed":
      return { ...base, type, embed: parseEmbedPayload(record.embed, "block.embed") };
    case "equation":
      return { ...base, type, equation: parseEquationPayload(record.equation, "block.equation") };
    case "divider":
      return { ...base, type, divider: readRecord(record.divider, "block.divider") };
    case "code":
      return { ...base, type, code: parseCodePayload(record.code, "block.code") };
    case "unsupported":
      return {
        ...base,
        type,
        unsupported: parseUnsupportedPayload(record.unsupported, "block.unsupported"),
      };
  }
}

export function parseNotesPaginatedBlockList(value: unknown): NotesPaginatedBlockList {
  const record = readRecord(value, "block list");
  if (record.object !== "list") throw new Error("block list.object must be list");
  if (record.type !== "block") throw new Error("block list.type must be block");
  if (!Array.isArray(record.results)) throw new Error("block list.results must be an array");
  return {
    object: "list",
    type: "block",
    block: readRecord(record.block, "block list.block"),
    results: record.results.map(parseNotesBlock),
    next_cursor: readNullableString(record.next_cursor, "block list.next_cursor"),
    has_more: readBoolean(record.has_more, "block list.has_more"),
  };
}

/** Validate persisted indentation independently of any editor nesting limit. */
export function parseNotesIndent(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) throw new Error("Notes indentation must be a nonnegative safe integer");
  return value;
}
