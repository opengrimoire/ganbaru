import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { parseNotesDataSourceRowPageContext } from "$lib/notes/database/row-page-context";
import type { NotesDataSourceRowPageContext } from "$lib/notes/database/row-page-context";
import { mapNotesLoadedPageDto } from "$lib/notes/validation/response-mappers";
import type { NotesDataSourceRowPageCreateRequest, NotesLoadedPage } from "$lib/notes/types";
import { invokeNotesMutation } from "./mutation";

/** Create a canonical source-owned row and its sub-item relationship atomically. */
export async function createNotesDataSourceSubitem(
  dataSourceId: string, parentRowPageId: string, request: NotesDataSourceRowPageCreateRequest,
): Promise<NotesLoadedPage> {
  const dbUrl = await ensureDbUrl();
  return mapNotesLoadedPageDto(await invokeNotesMutation("notes_create_data_source_subitem", {
    dbUrl, dataSourceId, parentRowPageId, request,
  }));
}

/** Change a sub-item's parent while retaining its existing data-source page ownership. */
export async function updateNotesDataSourceRowParent(
  dataSourceId: string, rowPageId: string, parentRowPageId: string | null,
): Promise<void> {
  const dbUrl = await ensureDbUrl();
  await invokeNotesMutation("notes_update_data_source_row_parent", {
    dbUrl, dataSourceId, rowPageId, update: { parent_row_page_id: parentRowPageId },
  });
}

/** Load one row page with computed values, its source schema, parent, sub-items, and lock state. */
export async function getNotesDataSourceRowPageContext(
  dataSourceId: string, pageId: string,
): Promise<NotesDataSourceRowPageContext> {
  const dbUrl = await ensureDbUrl();
  return parseNotesDataSourceRowPageContext(await invoke<unknown>(
    "notes_get_data_source_row_page_context", { dbUrl, dataSourceId, pageId },
  ));
}
