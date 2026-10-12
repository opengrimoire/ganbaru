import type { NotesChildDatabaseBlockPayload, NotesCreatedDatabase } from "../../contracts/core";
import { NOTES_DATABASE_VIEW_TYPES } from "$lib/notes/contracts/database";
import type { NotesDataSource, NotesDataSourceListItem, NotesDataSourceSchema, NotesDataSourceTemplate, NotesDatabase, NotesDatabaseDataSourceSummary, NotesDatabaseView, NotesDatabaseViewType } from "$lib/notes/contracts/database";
import { parseNullableNotesIcon, parseNullablePageCover } from ".././assets";
import { parseNotesBlock, parseNotesParent } from ".././blocks";
import { containsControlCharacters, readBoolean, readDisplayString, readInteger, readNotesDatabaseViewType, readNullableRecord, readNullableString, readOptionalUuidString, readRecord, readRecordArray, readString, readStringArray } from ".././readers";
import type { UnknownRecord } from ".././readers";
import { parseNotesRichTextArray } from ".././rich-text";

export function parseChildDatabasePayload(
  value: unknown,
  label: string,
): NotesChildDatabaseBlockPayload {
  const record = readRecord(value, label);
  const title = readString(record.title, `${label}.title`);
  if (containsControlCharacters(title)) {
    throw new Error(`${label}.title must not contain control characters`);
  }
  const databaseId = readOptionalUuidString(record.database_id, `${label}.database_id`);
  const dataSourceId = readOptionalUuidString(record.data_source_id, `${label}.data_source_id`);
  const viewId = readOptionalUuidString(record.view_id, `${label}.view_id`);
  return {
    title,
    ...(record.editing_locked === undefined ? {} : { editing_locked: readBoolean(record.editing_locked, `${label}.editing_locked`) }),
    ...(databaseId === undefined ? {} : { database_id: databaseId }),
    ...(dataSourceId === undefined ? {} : { data_source_id: dataSourceId }),
    ...(viewId === undefined ? {} : { view_id: viewId }),
  };
}

function parseNotesDatabaseDataSourceSummary(
  value: unknown,
  label: string,
): NotesDatabaseDataSourceSummary {
  const record = readRecord(value, label);
  return {
    id: readString(record.id, `${label}.id`),
    name: readString(record.name, `${label}.name`),
  };
}

function parseNotesDatabaseDataSources(
  value: unknown,
  label: string,
): NotesDatabaseDataSourceSummary[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  return value.map((item, index) => parseNotesDatabaseDataSourceSummary(item, `${label}[${index}]`));
}

export function parseNotesDatabase(value: unknown): NotesDatabase {
  const record = readRecord(value, "database");
  if (record.object !== "database") throw new Error("database.object must be database");
  return {
    object: "database",
    id: readString(record.id, "database.id"),
    parent: parseNotesParent(record.parent),
    title: readString(record.title, "database.title"),
    title_rich_text: parseNotesRichTextArray(record.title_rich_text, "database.title_rich_text"),
    description: parseNotesRichTextArray(record.description, "database.description"),
    icon: parseNullableNotesIcon(record.icon, "database.icon"),
    cover: parseNullablePageCover(record.cover, "database.cover"),
    in_trash: readBoolean(record.in_trash, "database.in_trash"),
    is_inline: readBoolean(record.is_inline, "database.is_inline"),
    data_sources: parseNotesDatabaseDataSources(record.data_sources, "database.data_sources"),
    url: readNullableString(record.url, "database.url"),
    public_url: readNullableString(record.public_url, "database.public_url"),
    source_provider: readNullableString(record.source_provider, "database.source_provider"),
    source_object_id: readNullableString(record.source_object_id, "database.source_object_id"),
    source_workspace_id: readNullableString(
      record.source_workspace_id,
      "database.source_workspace_id",
    ),
    source_last_edited_time: readNullableString(
      record.source_last_edited_time,
      "database.source_last_edited_time",
    ),
    created_time: readString(record.created_time, "database.created_time"),
    last_edited_time: readString(record.last_edited_time, "database.last_edited_time"),
  };
}

export function parseNotesDataSource(value: unknown): NotesDataSource {
  const record = readRecord(value, "data source");
  if (record.object !== "data_source") {
    throw new Error("data source.object must be data_source");
  }
  const parent = readRecord(record.parent, "data source.parent");
  if (parent.type !== "database_id") {
    throw new Error("data source.parent.type must be database_id");
  }
  return {
    object: "data_source",
    id: readString(record.id, "data source.id"),
    parent: {
      type: "database_id",
      database_id: readString(parent.database_id, "data source.parent.database_id"),
    },
    database_parent: parseNotesParent(record.database_parent),
    title: readString(record.title, "data source.title"),
    title_rich_text: parseNotesRichTextArray(
      record.title_rich_text,
      "data source.title_rich_text",
    ),
    description: parseNotesRichTextArray(record.description, "data source.description"),
    icon: parseNullableNotesIcon(record.icon, "data source.icon"),
    properties: readRecord(record.properties, "data source.properties"),
    in_trash: readBoolean(record.in_trash, "data source.in_trash"),
    source_provider: readNullableString(record.source_provider, "data source.source_provider"),
    source_object_id: readNullableString(record.source_object_id, "data source.source_object_id"),
    source_workspace_id: readNullableString(
      record.source_workspace_id,
      "data source.source_workspace_id",
    ),
    source_last_edited_time: readNullableString(
      record.source_last_edited_time,
      "data source.source_last_edited_time",
    ),
    created_time: readString(record.created_time, "data source.created_time"),
    last_edited_time: readString(record.last_edited_time, "data source.last_edited_time"),
  };
}

/** Validate a data source from the vault-wide list, including its page location. */
export function parseNotesDataSourceListItem(value: unknown): NotesDataSourceListItem {
  const record = readRecord(value, "data source");
  return { ...parseNotesDataSource(record), location: readStringArray(record.location, "data source.location") };
}

export function isNotesDatabaseViewType(value: unknown): value is NotesDatabaseViewType {
  return (
    typeof value === "string"
    && NOTES_DATABASE_VIEW_TYPES.includes(value as NotesDatabaseViewType)
  );
}

export function parseNotesDatabaseView(value: unknown): NotesDatabaseView {
  const record = readRecord(value, "database view");
  if (record.object !== "view") throw new Error("database view.object must be view");
  const parent = readRecord(record.parent, "database view.parent");
  if (parent.type !== "database_id") {
    throw new Error("database view.parent.type must be database_id");
  }
  return {
    object: "view",
    id: readString(record.id, "database view.id"),
    parent: {
      type: "database_id",
      database_id: readString(parent.database_id, "database view.parent.database_id"),
    },
    data_source_id: readString(record.data_source_id, "database view.data_source_id"),
    name: readString(record.name, "database view.name"),
    type: readNotesDatabaseViewType(record.type, "database view.type"),
    filter: readNullableRecord(record.filter, "database view.filter"),
    sorts: readRecordArray(record.sorts, "database view.sorts"),
    configuration: readNullableRecord(record.configuration, "database view.configuration"),
    url: readNullableString(record.url, "database view.url"),
    source_provider: readNullableString(record.source_provider, "database view.source_provider"),
    source_object_id: readNullableString(record.source_object_id, "database view.source_object_id"),
    source_workspace_id: readNullableString(
      record.source_workspace_id,
      "database view.source_workspace_id",
    ),
    source_last_edited_time: readNullableString(
      record.source_last_edited_time,
      "database view.source_last_edited_time",
    ),
    created_time: readString(record.created_time, "database view.created_time"),
    last_edited_time: readString(record.last_edited_time, "database view.last_edited_time"),
  };
}

export function parseNotesCreatedDatabase(value: unknown): NotesCreatedDatabase {
  const record = readRecord(value, "created database");
  const block = parseNotesBlock(record.block);
  if (block.type !== "child_database") {
    throw new Error("created database.block must be a child_database block");
  }
  return {
    database: parseNotesDatabase(record.database),
    data_source: parseNotesDataSource(record.data_source),
    view: parseNotesDatabaseView(record.view),
    block,
  };
}

export function parseNotesDataSourceSchema(value: unknown): NotesDataSourceSchema {
  const record = readRecord(value, "data source schema");
  return {
    data_source: parseNotesDataSource(record.data_source),
    view: parseNotesDatabaseView(record.view),
  };
}

export function parseDataSourceWindowMetadata(record: UnknownRecord, label: string) {
  return {
    total_row_count: readInteger(record.total_row_count, `${label}.total_row_count`),
    next_cursor: readNullableString(record.next_cursor, `${label}.next_cursor`),
    has_more: readBoolean(record.has_more, `${label}.has_more`),
  };
}

export function parseDataSourceGroupCounts(value: unknown, label: string): Record<string, number> {
  const record = readRecord(value, label);
  return Object.fromEntries(Object.entries(record).map(([id, count]) => {
    const parsed = readInteger(count, `${label}.${id}`);
    if (parsed < 0) throw new Error(`${label}.${id} must not be negative`);
    return [id, parsed];
  }));
}

export function parseNotesDataSourceTemplate(value: unknown): NotesDataSourceTemplate {
  const record = readRecord(value, "data source template");
  if (record.object !== "data_source_template") {
    throw new Error("data source template.object must be data_source_template");
  }
  const blockCount = readInteger(record.block_count, "data source template.block_count");
  if (blockCount < 0) {
    throw new Error("data source template.block_count must not be negative");
  }
  return {
    object: "data_source_template",
    id: readString(record.id, "data source template.id"),
    data_source_id: readString(record.data_source_id, "data source template.data_source_id"),
    source_page_id: readNullableString(
      record.source_page_id,
      "data source template.source_page_id",
    ),
    name: readDisplayString(record.name, "data source template.name"),
    properties: readRecord(record.properties, "data source template.properties"),
    is_default: readBoolean(record.is_default, "data source template.is_default"),
    block_count: blockCount,
    created_time: readString(record.created_time, "data source template.created_time"),
    last_edited_time: readString(
      record.last_edited_time,
      "data source template.last_edited_time",
    ),
  };
}
