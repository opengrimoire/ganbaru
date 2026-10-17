import { createRichText } from "$lib/notes/blocks/factory";
import { NOTES_PAGE_PROJECT_ID_PROPERTY } from "$lib/notes/project-membership";
import { mapNotesPageDto } from "$lib/notes/validation/response-mappers";
import type { NotesNavigationDatabase, NotesPage } from "$lib/notes/types";
import { isNotesUuid } from "$lib/notes/links/block-link";

/**
 * Require an object payload from a Notes workspace or summary command.
 *
 * @param value Untrusted command payload or nested row.
 * @param context Command or field name reported when the payload is not an object.
 * @returns The payload as a string-keyed record.
 */
export function notesWorkspaceShellRecord(value: unknown, context: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${context} must be an object`);
  }
  return value as Record<string, unknown>;
}

function shellString(value: unknown, field: string): string {
  if (typeof value !== "string") throw new Error(`${field} must be a string`);
  return value;
}

/** Validate the compact identities used to open database hierarchy branches. */
export function mapNavigationDatabase(value: unknown): NotesNavigationDatabase {
  const record = notesWorkspaceShellRecord(value, "navigation database");
  if (!isNotesUuid(record.id) || !isNotesUuid(record.page_id) || !isNotesUuid(record.data_source_id)) {
    throw new Error("navigation database identities must be UUIDs");
  }
  return {
    id: record.id,
    page_id: record.page_id,
    title: shellString(record.title, "navigation database.title"),
    data_source_id: record.data_source_id,
  };
}

/** Require a nullable string field from a Notes workspace payload. */
export function shellNullableString(value: unknown, field: string): string | null {
  if (value === null) return null;
  return shellString(value, field);
}

/** Require a string array field from a Notes workspace payload. */
export function shellStringArray(value: unknown, field: string): string[] {
  if (!Array.isArray(value)) throw new Error(`${field} must be an array`);
  return value.map((item, index) => shellString(item, `${field}[${index}]`));
}

/** Require a non-negative safe integer from a Notes workspace payload. */
export function shellNumber(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${field} must be a non-negative integer`);
  }
  return value;
}

function shellBoolean(value: unknown, field: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${field} must be a boolean`);
  return value;
}

function parsePageSummaryIcon(value: string | null, pageId: string): unknown {
  if (value === null) return null;
  try {
    return JSON.parse(value) as unknown;
  } catch {
    throw new Error(`page.icon for page "${pageId}" must contain valid JSON`);
  }
}

/** Parse the compact persisted page row returned by Notes list commands. */
export function mapPageSummary(value: unknown): NotesPage {
  const row = notesWorkspaceShellRecord(value, "page");
  const parentType = shellString(row.parent_type, "page.parent_type");
  let parent: NotesPage["parent"];
  if (parentType === "workspace") {
    parent = { type: "workspace", workspace: true };
  } else if (parentType === "page_id") {
    parent = { type: "page_id", page_id: shellString(row.parent_page_id, "page.parent_page_id") };
  } else if (parentType === "block_id") {
    parent = { type: "block_id", block_id: shellString(row.parent_block_id, "page.parent_block_id") };
  } else if (parentType === "data_source_id") {
    parent = {
      type: "data_source_id",
      data_source_id: shellString(row.parent_data_source_id, "page.parent_data_source_id"),
    };
  } else {
    throw new Error("page.parent_type is invalid");
  }
  const title = shellString(row.title, "page.title");
  const projectId = shellNullableString(row.project_id, "page.project_id");
  const rawIcon = shellNullableString(row.icon, "page.icon");
  const pageId = shellString(row.id, "page.id");
  return mapNotesPageDto({
    object: "page",
    id: pageId,
    created_time: shellString(row.created_time, "page.created_time"),
    last_edited_time: shellString(row.last_edited_time, "page.last_edited_time"),
    parent,
    folder_id: shellNullableString(row.folder_id, "page.folder_id"),
    in_trash: row.in_trash === undefined ? false : shellBoolean(row.in_trash, "page.in_trash"),
    archived: row.archived === undefined ? false : shellBoolean(row.archived, "page.archived"),
    icon: parsePageSummaryIcon(rawIcon, pageId),
    cover: null,
    properties: {
      title: { id: "title", type: "title", title: title ? [createRichText(title)] : [] },
      ...(projectId ? { [NOTES_PAGE_PROJECT_ID_PROPERTY]: projectId } : {}),
    },
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  });
}
