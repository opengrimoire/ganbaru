import type {
  NotesChildDatabaseBlock,
  NotesDatabaseViewScope,
  NotesLinkedDatabaseCreateRequest,
} from "$lib/notes/types";

export interface NotesLinkedDatabaseRequestIds {
  databaseId: string;
  viewId: string;
}

/** Return the scoped view identity stored on a local child database block. */
export function notesChildDatabaseViewScope(
  block: NotesChildDatabaseBlock,
): NotesDatabaseViewScope | null {
  const { database_id: databaseId, data_source_id: dataSourceId, view_id: viewId } = block.child_database;
  if (!databaseId || !dataSourceId || !viewId) return null;
  return { databaseId, viewId };
}

/** Build the Tauri request for a linked database view below an existing local database block. */
export function createNotesLinkedDatabaseViewRequest(
  sourceBlock: NotesChildDatabaseBlock,
  ids: NotesLinkedDatabaseRequestIds,
  title: string,
): NotesLinkedDatabaseCreateRequest {
  if (!notesChildDatabaseViewScope(sourceBlock)) {
    throw new Error("source block must be a local database");
  }
  const trimmedTitle = title.trim();
  return {
    id: ids.databaseId,
    view_id: ids.viewId,
    source_block_id: sourceBlock.id,
    title: trimmedTitle || null,
  };
}
