import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { invalidateNotesNotificationSchedule } from "$lib/notes/collaboration/notification-schedule.svelte";
import {
  mapNotesCreatedDatabaseDto,
  mapNotesDataSourceDto,
  mapNotesDataSourceListItemDto,
  mapNotesDatabaseViewDto,
  mapNotesDataSourceBoardViewDto,
  mapNotesDataSourceCalendarViewDto,
  mapNotesDataSourceCsvExportSaveDto,
  mapNotesDataSourceCsvImportDto,
  mapNotesDataSourceGalleryViewDto,
  mapNotesDataSourceListViewDto,
  mapNotesDataSourceSchemaDto,
  mapNotesDataSourceTableViewDto,
  mapNotesDataSourceTemplateDto,
  mapNotesDataSourceTimelineViewDto,
  mapNotesLoadedPageDto,
  mapNotesPageDto,
} from "$lib/notes/validation/response-mappers";
import type {
  NotesCreatedDatabase,
  NotesDatabaseCreateRequest,
  NotesDataSourceBoardRowMove,
  NotesDataSourceBoardView,
  NotesDataSourceBoardViewUpdate,
  NotesDataSourceButtonClickRequest,
  NotesDataSourceCalendarView,
  NotesDataSourceCalendarViewUpdate,
  NotesDataSourceCsvExportRequest,
  NotesDataSourceCsvExportSaveResult,
  NotesDataSourceCsvImportRequest,
  NotesDataSourceCsvImportResult,
  NotesDataSourceGalleryView,
  NotesDataSourceGalleryViewUpdate,
  NotesDataSourceListView,
  NotesDataSourceListViewUpdate,
  NotesDatabaseViewScope,
  NotesDatabaseView,
  NotesDatabaseViewDuplicateRequest,
  NotesDataSource,
  NotesDataSourceListItem,
  NotesDataSourceRowPageCreateRequest,
  NotesDataSourceRowPropertyUpdate,
  NotesDataSourceSchema,
  NotesDataSourceSchemaUpdate,
  NotesDataSourceCreateRequest,
  NotesDataSourceAttachRequest,
  NotesDataSourcePropertyAction,
  NotesDataSourcePropertyActionResult,
  NotesDataSourceTableView,
  NotesDataSourceTableViewUpdate,
  NotesDataSourceViewWindowRequest,
  NotesDataSourceTemplate,
  NotesDataSourceTemplateApplyRequest,
  NotesDataSourceTemplateCreateFromRowRequest,
  NotesDataSourceTimelineView,
  NotesDataSourceTimelineViewUpdate,
  NotesLinkedDatabaseCreateRequest,
  NotesLoadedPage,
  NotesPage,
} from "$lib/notes/types";
import { invokeNotesMutation } from "./mutation";
import { isNotesUuid } from "$lib/notes/links/block-link";
import type { NotesDatabaseReference } from "$lib/notes/types";

/** Read a database reference's destination and ownership without fetching its rows. */
export async function getNotesDatabaseReference(blockId: string): Promise<NotesDatabaseReference> {
  const dbUrl = await ensureDbUrl();
  const value: unknown = await invoke("notes_database_reference", { dbUrl, blockId });
  return parseDatabaseReference(value);
}

/** Lock or unlock structural edits for one database shell. */
export async function setNotesDatabaseEditingLock(databaseId: string, locked: boolean): Promise<NotesDatabaseReference> {
  const dbUrl = await ensureDbUrl();
  return parseDatabaseReference(await invokeNotesMutation("notes_set_database_editing_lock", { dbUrl, databaseId, locked }));
}

/** Create an independent source in an existing database shell. */
export async function createNotesDataSource(request: NotesDataSourceCreateRequest): Promise<NotesDataSourceSchema> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceSchemaDto(await invokeNotesMutation("notes_create_data_source", { dbUrl, request }));
}

/** Show an existing source through a new table view while retaining its owner and rows. */
export async function attachNotesDataSource(request: NotesDataSourceAttachRequest): Promise<NotesDataSourceSchema> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceSchemaDto(await invokeNotesMutation("notes_attach_data_source", { dbUrl, request }));
}

/** Apply contextual property creation and table placement as one native transaction. */
export async function applyNotesDataSourcePropertyAction(dataSourceId: string, databaseId: string, viewId: string, action: NotesDataSourcePropertyAction): Promise<NotesDataSourcePropertyActionResult> {
  const dbUrl = await ensureDbUrl();
  const value: unknown = await invokeNotesMutation("notes_apply_data_source_property_action", { dbUrl, dataSourceId, databaseId, viewId, action });
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("Invalid database property action response");
  const record = value as Record<string, unknown>;
  if (typeof record.property_id !== "string" || !record.property_id.trim()) throw new Error("Database property action returned no property identity");
  return { property_id: record.property_id, schema: mapNotesDataSourceSchemaDto(record.schema) };
}

function parseDatabaseReference(value: unknown): NotesDatabaseReference {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid Notes database reference");
  const dto = value as Record<string, unknown>;
  if (!isNotesUuid(dto.source_block_id) || !isNotesUuid(dto.page_id)
    || !isNotesUuid(dto.canonical_source_block_id) || !isNotesUuid(dto.canonical_source_page_id)
    || typeof dto.title !== "string" || typeof dto.is_linked !== "boolean" || typeof dto.editing_locked !== "boolean"
    || typeof dto.owned_data_source_count !== "number" || !Number.isSafeInteger(dto.owned_data_source_count)
    || dto.owned_data_source_count < 0) throw new Error("Invalid Notes database reference metadata");
  return { block_id: dto.source_block_id, page_id: dto.page_id,
    source_block_id: dto.canonical_source_block_id, source_page_id: dto.canonical_source_page_id,
    title: dto.title, is_linked: dto.is_linked, editing_locked: dto.editing_locked, owned_data_source_count: dto.owned_data_source_count };
}

function databaseViewScopeArgs(scope?: NotesDatabaseViewScope | null): {
  databaseId: string | null;
  viewId: string | null;
} {
  return {
    databaseId: scope?.databaseId ?? null,
    viewId: scope?.viewId ?? null,
  };
}

export async function createNotesDatabase(
  request: NotesDatabaseCreateRequest,
): Promise<NotesCreatedDatabase> {
  const dbUrl = await ensureDbUrl();
  return mapNotesCreatedDatabaseDto(
    await invokeNotesMutation("notes_create_database", { dbUrl, request }),
  );
}

export async function createNotesLinkedDatabaseView(
  request: NotesLinkedDatabaseCreateRequest,
): Promise<NotesCreatedDatabase> {
  const dbUrl = await ensureDbUrl();
  return mapNotesCreatedDatabaseDto(
    await invokeNotesMutation("notes_create_linked_database_view", { dbUrl, request }),
  );
}

/** Rename an inline database and its owned data source. */
export async function renameNotesDatabase(databaseId: string, title: string): Promise<string> {
  const dbUrl = await ensureDbUrl();
  const renamed = await invokeNotesMutation("notes_rename_database", {
    dbUrl,
    databaseId,
    update: { title },
  });
  if (typeof renamed !== "string") throw new Error("notes_rename_database returned an invalid title");
  return renamed;
}

/** List only views saved in this database shell. */
export async function listNotesDatabaseViews(databaseId: string): Promise<NotesDatabaseView[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<unknown>("notes_list_database_views", { dbUrl, databaseId });
  if (!Array.isArray(rows)) throw new Error("notes_list_database_views returned a non-array payload");
  return rows.map(mapNotesDatabaseViewDto);
}

/** Copy a view's independent layout, filters, and sorts. */
export async function duplicateNotesDatabaseView(
  request: NotesDatabaseViewDuplicateRequest,
): Promise<NotesDatabaseView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDatabaseViewDto(await invokeNotesMutation("notes_duplicate_database_view", { dbUrl, request }));
}

/** Save the toolbar order of a database's views; the list must name every view exactly once. */
export async function reorderNotesDatabaseViews(databaseId: string, viewIds: string[]): Promise<NotesDatabaseView[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invokeNotesMutation("notes_reorder_database_views", { dbUrl, databaseId, viewIds });
  if (!Array.isArray(rows)) throw new Error("notes_reorder_database_views returned a non-array payload");
  return rows.map(mapNotesDatabaseViewDto);
}

/** Rename one saved view without changing its row data. */
export async function renameNotesDatabaseView(
  databaseId: string,
  viewId: string,
  name: string,
): Promise<NotesDatabaseView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDatabaseViewDto(await invokeNotesMutation("notes_rename_database_view", {
    dbUrl,
    databaseId,
    viewId,
    update: { name },
  }));
}

/** Remove a saved view while keeping its shared data source and row pages. */
export async function deleteNotesDatabaseView(databaseId: string, viewId: string): Promise<void> {
  const dbUrl = await ensureDbUrl();
  const deleted = await invokeNotesMutation("notes_delete_database_view", { dbUrl, databaseId, viewId });
  if (deleted !== viewId) throw new Error("notes_delete_database_view returned an unexpected view id");
}

export async function getNotesDataSourceSchema(
  dataSourceId: string,
  scope?: NotesDatabaseViewScope | null,
): Promise<NotesDataSourceSchema> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceSchemaDto(
    await invoke<unknown>("notes_data_source_schema", {
      dbUrl,
      dataSourceId,
      ...databaseViewScopeArgs(scope),
    }),
  );
}

export async function updateNotesDataSourceSchema(
  dataSourceId: string,
  update: NotesDataSourceSchemaUpdate,
  scope?: NotesDatabaseViewScope | null,
): Promise<NotesDataSourceSchema> {
  const dbUrl = await ensureDbUrl();
  const schema = mapNotesDataSourceSchemaDto(
    await invokeNotesMutation("notes_update_data_source_schema", {
      dbUrl,
      dataSourceId,
      update,
      ...databaseViewScopeArgs(scope),
    }),
  );
  invalidateNotesNotificationSchedule();
  return schema;
}

export async function listNotesDataSources(): Promise<NotesDataSourceListItem[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<unknown>("notes_list_data_sources", { dbUrl });
  if (!Array.isArray(rows)) {
    throw new Error("notes_list_data_sources returned a non-array payload");
  }
  return rows.map(mapNotesDataSourceListItemDto);
}

export async function listNotesDataSourceRowPages(
  dataSourceId: string,
): Promise<NotesPage[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<unknown>("notes_list_data_source_row_pages", { dbUrl, dataSourceId });
  if (!Array.isArray(rows)) {
    throw new Error("notes_list_data_source_row_pages returned a non-array payload");
  }
  return rows.map(mapNotesPageDto);
}

export async function createNotesDataSourceRowPage(
  dataSourceId: string,
  request: NotesDataSourceRowPageCreateRequest,
): Promise<NotesLoadedPage> {
  const dbUrl = await ensureDbUrl();
  return mapNotesLoadedPageDto(
    await invokeNotesMutation("notes_create_data_source_row_page", {
      dbUrl,
      dataSourceId,
      request,
    }),
  );
}

export async function importNotesDataSourceCsv(
  dataSourceId: string,
  request: NotesDataSourceCsvImportRequest,
): Promise<NotesDataSourceCsvImportResult> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceCsvImportDto(
    await invokeNotesMutation(
      "notes_import_data_source_csv",
      { dbUrl, dataSourceId, request },
    ),
  );
}

export async function saveNotesDataSourceCsv(
  dataSourceId: string,
  request: NotesDataSourceCsvExportRequest,
): Promise<NotesDataSourceCsvExportSaveResult> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceCsvExportSaveDto(
    await invoke<unknown>("notes_pick_and_write_data_source_csv", { dbUrl, dataSourceId, request }),
  );
}

export async function listNotesDataSourceTemplates(
  dataSourceId: string,
): Promise<NotesDataSourceTemplate[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<unknown>("notes_list_data_source_templates", { dbUrl, dataSourceId });
  if (!Array.isArray(rows)) {
    throw new Error("notes_list_data_source_templates returned a non-array payload");
  }
  return rows.map(mapNotesDataSourceTemplateDto);
}

export async function createNotesDataSourceTemplateFromRow(
  dataSourceId: string,
  request: NotesDataSourceTemplateCreateFromRowRequest,
): Promise<NotesDataSourceTemplate> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceTemplateDto(
    await invokeNotesMutation("notes_create_data_source_template_from_row", {
      dbUrl,
      dataSourceId,
      request,
    }),
  );
}

export async function applyNotesDataSourceTemplate(
  dataSourceId: string,
  templateId: string,
  request: NotesDataSourceTemplateApplyRequest,
): Promise<NotesLoadedPage> {
  const dbUrl = await ensureDbUrl();
  return mapNotesLoadedPageDto(
    await invokeNotesMutation("notes_apply_data_source_template", {
      dbUrl,
      dataSourceId,
      templateId,
      request,
    }),
  );
}

export async function deleteNotesDataSourceTemplate(
  dataSourceId: string,
  templateId: string,
): Promise<string> {
  const dbUrl = await ensureDbUrl();
  const deletedTemplateId = await invokeNotesMutation("notes_delete_data_source_template", {
    dbUrl,
    dataSourceId,
    templateId,
  });
  if (typeof deletedTemplateId !== "string") {
    throw new Error("notes_delete_data_source_template returned an invalid template id");
  }
  return deletedTemplateId;
}

export async function getNotesDataSourceTableView(
  dataSourceId: string,
  scope?: NotesDatabaseViewScope | null,
  window?: NotesDataSourceViewWindowRequest,
): Promise<NotesDataSourceTableView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceTableViewDto(
    await invoke<unknown>("notes_data_source_table_view", {
      dbUrl,
      dataSourceId,
      ...databaseViewScopeArgs(scope),
      window: window ?? null,
    }),
  );
}

export async function updateNotesDataSourceTableView(
  dataSourceId: string,
  update: NotesDataSourceTableViewUpdate,
  scope?: NotesDatabaseViewScope | null,
): Promise<NotesDataSourceTableView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceTableViewDto(
    await invokeNotesMutation("notes_update_data_source_table_view", {
      dbUrl,
      dataSourceId,
      update,
      ...databaseViewScopeArgs(scope),
    }),
  );
}
export async function getNotesDataSourceBoardView(
  dataSourceId: string,
  scope?: NotesDatabaseViewScope | null,
  window?: NotesDataSourceViewWindowRequest,
): Promise<NotesDataSourceBoardView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceBoardViewDto(
    await invoke<unknown>("notes_data_source_board_view", {
      dbUrl,
      dataSourceId,
      ...databaseViewScopeArgs(scope),
      window: window ?? null,
    }),
  );
}

export async function updateNotesDataSourceBoardView(
  dataSourceId: string,
  update: NotesDataSourceBoardViewUpdate,
  scope?: NotesDatabaseViewScope | null,
): Promise<NotesDataSourceBoardView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceBoardViewDto(
    await invokeNotesMutation("notes_update_data_source_board_view", {
      dbUrl,
      dataSourceId,
      update,
      ...databaseViewScopeArgs(scope),
    }),
  );
}

export async function moveNotesDataSourceBoardRow(
  dataSourceId: string,
  request: NotesDataSourceBoardRowMove,
  scope?: NotesDatabaseViewScope | null,
): Promise<NotesDataSourceBoardView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceBoardViewDto(
    await invokeNotesMutation("notes_move_data_source_board_row", {
      dbUrl,
      dataSourceId,
      request,
      ...databaseViewScopeArgs(scope),
    }),
  );
}

export async function getNotesDataSourceGalleryView(
  dataSourceId: string,
  scope?: NotesDatabaseViewScope | null,
  window?: NotesDataSourceViewWindowRequest,
): Promise<NotesDataSourceGalleryView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceGalleryViewDto(
    await invoke<unknown>("notes_data_source_gallery_view", {
      dbUrl,
      dataSourceId,
      ...databaseViewScopeArgs(scope),
      window: window ?? null,
    }),
  );
}

export async function updateNotesDataSourceGalleryView(
  dataSourceId: string,
  update: NotesDataSourceGalleryViewUpdate,
  scope?: NotesDatabaseViewScope | null,
): Promise<NotesDataSourceGalleryView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceGalleryViewDto(
    await invokeNotesMutation("notes_update_data_source_gallery_view", {
      dbUrl,
      dataSourceId,
      update,
      ...databaseViewScopeArgs(scope),
    }),
  );
}

export async function updateNotesDataSourceRowProperty(
  dataSourceId: string,
  pageId: string,
  update: NotesDataSourceRowPropertyUpdate,
): Promise<NotesPage> {
  const dbUrl = await ensureDbUrl();
  const page = mapNotesPageDto(
    await invokeNotesMutation("notes_update_data_source_row_property", {
      dbUrl,
      dataSourceId,
      pageId,
      update,
    }),
  );
  invalidateNotesNotificationSchedule();
  return page;
}

/**
 * Set one property to the same value on several rows in one transaction; either every row changes or none does.
 *
 * @param dataSourceId The data source that owns every row.
 * @param pageIds The row pages to change, without duplicates.
 * @param update The property and the value every row receives.
 * @returns The updated row page ids in request order.
 */
export async function updateNotesDataSourceRowsProperty(
  dataSourceId: string,
  pageIds: readonly string[],
  update: NotesDataSourceRowPropertyUpdate,
): Promise<string[]> {
  const dbUrl = await ensureDbUrl();
  const updated = await invokeNotesMutation("notes_update_data_source_rows_property", {
    dbUrl,
    dataSourceId,
    pageIds: [...pageIds],
    update,
  });
  if (!Array.isArray(updated) || !updated.every((id): id is string => typeof id === "string")) {
    throw new Error("notes_update_data_source_rows_property returned invalid page ids");
  }
  invalidateNotesNotificationSchedule();
  return updated;
}

export async function clickNotesDataSourceButton(
  dataSourceId: string,
  pageId: string,
  request: NotesDataSourceButtonClickRequest,
): Promise<NotesPage> {
  const dbUrl = await ensureDbUrl();
  return mapNotesPageDto(
    await invokeNotesMutation("notes_click_data_source_button", {
      dbUrl,
      dataSourceId,
      pageId,
      request,
    }),
  );
}

export async function getNotesDataSourceListView(
  dataSourceId: string,
  scope?: NotesDatabaseViewScope | null,
  window?: NotesDataSourceViewWindowRequest,
): Promise<NotesDataSourceListView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceListViewDto(
    await invoke<unknown>("notes_data_source_list_view", {
      dbUrl,
      dataSourceId,
      ...databaseViewScopeArgs(scope),
      window: window ?? null,
    }),
  );
}

export async function updateNotesDataSourceListView(
  dataSourceId: string,
  update: NotesDataSourceListViewUpdate,
  scope?: NotesDatabaseViewScope | null,
): Promise<NotesDataSourceListView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceListViewDto(
    await invokeNotesMutation("notes_update_data_source_list_view", {
      dbUrl,
      dataSourceId,
      update,
      ...databaseViewScopeArgs(scope),
    }),
  );
}

export async function getNotesDataSourceCalendarView(
  dataSourceId: string,
  scope?: NotesDatabaseViewScope | null,
  window?: NotesDataSourceViewWindowRequest,
): Promise<NotesDataSourceCalendarView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceCalendarViewDto(
    await invoke<unknown>("notes_data_source_calendar_view", {
      dbUrl,
      dataSourceId,
      ...databaseViewScopeArgs(scope),
      window: window ?? null,
    }),
  );
}

export async function updateNotesDataSourceCalendarView(
  dataSourceId: string,
  update: NotesDataSourceCalendarViewUpdate,
  scope?: NotesDatabaseViewScope | null,
): Promise<NotesDataSourceCalendarView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceCalendarViewDto(
    await invokeNotesMutation("notes_update_data_source_calendar_view", {
      dbUrl,
      dataSourceId,
      update,
      ...databaseViewScopeArgs(scope),
    }),
  );
}

export async function getNotesDataSourceTimelineView(
  dataSourceId: string,
  scope?: NotesDatabaseViewScope | null,
  window?: NotesDataSourceViewWindowRequest,
): Promise<NotesDataSourceTimelineView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceTimelineViewDto(
    await invoke<unknown>("notes_data_source_timeline_view", {
      dbUrl,
      dataSourceId,
      ...databaseViewScopeArgs(scope),
      window: window ?? null,
    }),
  );
}

export async function updateNotesDataSourceTimelineView(
  dataSourceId: string,
  update: NotesDataSourceTimelineViewUpdate,
  scope?: NotesDatabaseViewScope | null,
): Promise<NotesDataSourceTimelineView> {
  const dbUrl = await ensureDbUrl();
  return mapNotesDataSourceTimelineViewDto(
    await invokeNotesMutation("notes_update_data_source_timeline_view", {
      dbUrl,
      dataSourceId,
      update,
      ...databaseViewScopeArgs(scope),
    }),
  );
}
