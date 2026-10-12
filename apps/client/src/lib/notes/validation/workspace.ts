import { parseNotesIndent } from "./blocks";
import type { NotesChildPageBlockPayload, NotesFolder, NotesPage, NotesPageTemplate, NotesTemplateBlockPayload } from "../contracts/core";
import type { NotesBlockFrontier, NotesBlockOutline, NotesLoadedPage, NotesPageOpenResponse, NotesSidebarPageList } from "../contracts/workspace";
import { parseNullableNotesIcon, parseNullablePageCover } from "./assets";
import { isNotesBlockType, parseNotesBlock, parseNotesPaginatedBlockList, parseNotesParent } from "./blocks";
import { parseNotesPageBreadcrumbItem } from "./knowledge";
import { readBoolean, readDisplayString, readFiniteNumber, readInteger, readNullableString, readRecord, readString, readStringArray } from "./readers";
import { parseNotesRichTextArray } from "./rich-text";

export function parseTemplatePayload(value: unknown, label: string): NotesTemplateBlockPayload {
  const record = readRecord(value, label);
  if (record.color !== undefined) throw new Error(`${label}.color is not supported`);
  if (record.children !== undefined) {
    throw new Error(`${label}.children must be stored as child blocks`);
  }
  return {
    rich_text: parseNotesRichTextArray(record.rich_text, `${label}.rich_text`),
  };
}

export function parseChildPagePayload(value: unknown, label: string): NotesChildPageBlockPayload {
  const record = readRecord(value, label);
  return {
    title: readString(record.title, `${label}.title`),
  };
}

export function parseNotesPage(value: unknown): NotesPage {
  const record = readRecord(value, "page");
  if (record.object !== "page") throw new Error("page.object must be page");
  return {
    object: "page",
    id: readString(record.id, "page.id"),
    created_time: readString(record.created_time, "page.created_time"),
    last_edited_time: readString(record.last_edited_time, "page.last_edited_time"),
    parent: parseNotesParent(record.parent),
    folder_id: readNullableString(record.folder_id, "page.folder_id"),
    in_trash: readBoolean(record.in_trash, "page.in_trash"),
    archived: typeof record.archived === "boolean" ? record.archived : undefined,
    icon: parseNullableNotesIcon(record.icon, "page.icon"),
    cover: parseNullablePageCover(record.cover, "page.cover"),
    properties: readRecord(record.properties, "page.properties"),
    url: readNullableString(record.url, "page.url"),
    public_url: readNullableString(record.public_url, "page.public_url"),
    source_provider: readNullableString(record.source_provider, "page.source_provider"),
    source_object_id: readNullableString(record.source_object_id, "page.source_object_id"),
    source_workspace_id: readNullableString(record.source_workspace_id, "page.source_workspace_id"),
    source_last_edited_time: readNullableString(
      record.source_last_edited_time,
      "page.source_last_edited_time",
    ),
  };
}

export function parseNotesFolder(value: unknown): NotesFolder {
  const record = readRecord(value, "folder");
  if (record.object !== "folder") throw new Error("folder.object must be folder");
  return {
    object: "folder",
    id: readString(record.id, "folder.id"),
    project_id: readString(record.project_id, "folder.project_id"),
    parent_folder_id: readNullableString(
      record.parent_folder_id,
      "folder.parent_folder_id",
    ),
    name: readDisplayString(record.name, "folder.name"),
    created_time: readString(record.created_time, "folder.created_time"),
    last_edited_time: readString(record.last_edited_time, "folder.last_edited_time"),
  };
}

export function parseNotesLoadedPage(value: unknown): NotesLoadedPage {
  const record = readRecord(value, "loaded page");
  return {
    page: parseNotesPage(record.page),
    blocks: parseNotesPaginatedBlockList(record.blocks),
  };
}

export function parseNotesPageOpenResponse(value: unknown): NotesPageOpenResponse {
  const record = readRecord(value, "page open response");
  if (!Array.isArray(record.breadcrumb)) {
    throw new Error("page open response.breadcrumb must be an array");
  }
  if (!Array.isArray(record.outlines)) {
    throw new Error("page open response.outlines must be an array");
  }
  return {
    page: parseNotesPage(record.page),
    breadcrumb: record.breadcrumb.map(parseNotesPageBreadcrumbItem),
    blocks: parseNotesPaginatedBlockList(record.blocks),
    outlines: record.outlines.map(parseNotesBlockOutline),
  };
}

export function parseNotesBlockOutline(value: unknown): NotesBlockOutline {
  const record = readRecord(value, "block outline");
  const blockType = readString(record.type, "block outline.type");
  if (!isNotesBlockType(blockType)) throw new Error("block outline.type is unsupported");
  const parent = parseNotesParent(record.parent);
  if (parent.type !== "page_id" && parent.type !== "block_id") {
    throw new Error("block outline.parent must identify a page or block");
  }
  const retainedHeight = readInteger(record.retained_height, "block outline.retained_height");
  if (retainedHeight <= 0) throw new Error("block outline.retained_height must be positive");
  return {
    id: readString(record.id, "block outline.id"),
    page_id: readString(record.page_id, "block outline.page_id"),
    parent,
    type: blockType,
    sort_order: readFiniteNumber(record.sort_order, "block outline.sort_order"),
    has_children: readBoolean(record.has_children, "block outline.has_children"),
    retained_height: retainedHeight,
    ganbaru_indent: parseNotesIndent(record.ganbaru_indent ?? 0),
  };
}

export function parseNotesBlockFrontier(value: unknown): NotesBlockFrontier {
  const record = readRecord(value, "block frontier");
  if (!Array.isArray(record.blocks)) {
    throw new Error("block frontier.blocks must be an array");
  }
  return { blocks: record.blocks.map(parseNotesBlock) };
}

export function parseNotesSidebarPageList(value: unknown): NotesSidebarPageList {
  const record = readRecord(value, "sidebar page list");
  if (!Array.isArray(record.pages)) throw new Error("sidebar page list.pages must be an array");
  return {
    pages: record.pages.map(parseNotesPage),
    page_ids_with_children: readStringArray(
      record.page_ids_with_children,
      "sidebar page list.page_ids_with_children",
    ),
    missing_parent_page_ids: readStringArray(
      record.missing_parent_page_ids,
      "sidebar page list.missing_parent_page_ids",
    ),
    trashed_parent_page_ids: readStringArray(
      record.trashed_parent_page_ids,
      "sidebar page list.trashed_parent_page_ids",
    ),
  };
}

export function parseNotesPageTemplate(value: unknown): NotesPageTemplate {
  const record = readRecord(value, "page template");
  if (record.object !== "page_template") {
    throw new Error("page template.object must be page_template");
  }
  const blockCount = readInteger(record.block_count, "page template.block_count");
  if (blockCount < 0) throw new Error("page template.block_count must not be negative");
  return {
    object: "page_template",
    id: readString(record.id, "page template.id"),
    name: readDisplayString(record.name, "page template.name"),
    source_page_id: readNullableString(record.source_page_id, "page template.source_page_id"),
    properties: readRecord(record.properties, "page template.properties"),
    icon: parseNullableNotesIcon(record.icon, "page template.icon"),
    cover: parseNullablePageCover(record.cover, "page template.cover"),
    block_count: blockCount,
    created_time: readString(record.created_time, "page template.created_time"),
    last_edited_time: readString(record.last_edited_time, "page template.last_edited_time"),
  };
}
