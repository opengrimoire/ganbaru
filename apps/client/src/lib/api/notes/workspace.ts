import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { mapNotesFolderDto } from "$lib/notes/validation/response-mappers";
import type {
  NotesFolder,
  NotesFolderCreate,
  NotesFolderUpdate,
  NotesPageSummaryWindow,
  NotesPageSummaryWindowRequest,
  NotesSidebarPageList,
  NotesSidebarPagesRequest,
  NotesWorkspaceShell,
  NotesWorkspaceShellRequest,
} from "$lib/notes/types";
import { invokeNotesMutation } from "./mutation";
import {
  mapPageSummary,
  mapNavigationDatabase,
  notesWorkspaceShellRecord,
  shellNullableString,
  shellNumber,
  shellStringArray,
} from "./workspace-parsing";

export async function listNotesFolders(): Promise<NotesFolder[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<unknown>("notes_list_folders", { dbUrl });
  if (!Array.isArray(rows)) throw new Error("notes_list_folders returned a non-array payload");
  return rows.map(mapNotesFolderDto);
}

export async function loadNotesWorkspaceShell(
  request: NotesWorkspaceShellRequest,
): Promise<NotesWorkspaceShell> {
  const dbUrl = await ensureDbUrl();
  const value = await invoke<unknown>("notes_load_workspace_shell", { dbUrl, request });
  const record = notesWorkspaceShellRecord(value, "notes_load_workspace_shell response");
  if (!Array.isArray(record.pages) || !Array.isArray(record.folders) || !Array.isArray(record.navigation_databases)) {
    throw new Error("notes_load_workspace_shell returned invalid collections");
  }
  return {
    pages: record.pages.map(mapPageSummary),
    folders: record.folders.map(mapNotesFolderDto),
    navigation_pages: Array.isArray(record.navigation_pages)
      ? record.navigation_pages.map(mapPageSummary)
      : [],
    navigation_folders: Array.isArray(record.navigation_folders)
      ? record.navigation_folders.map(mapNotesFolderDto)
      : [],
    navigation_page_ids_with_children: shellStringArray(
      record.navigation_page_ids_with_children ?? [],
      "navigation_page_ids_with_children",
    ),
    navigation_databases: record.navigation_databases.map(mapNavigationDatabase),
    page_ids_with_children: shellStringArray(record.page_ids_with_children, "page_ids_with_children"),
    missing_parent_page_ids: shellStringArray(record.missing_parent_page_ids, "missing_parent_page_ids"),
    trashed_parent_page_ids: shellStringArray(record.trashed_parent_page_ids, "trashed_parent_page_ids"),
    resolved_selected_page_id: shellNullableString(
      record.resolved_selected_page_id,
      "resolved_selected_page_id",
    ),
    total_page_count: shellNumber(record.total_page_count, "total_page_count"),
    total_folder_count: shellNumber(record.total_folder_count, "total_folder_count"),
    next_page_cursor: shellNullableString(record.next_page_cursor, "next_page_cursor"),
    next_folder_cursor: shellNullableString(record.next_folder_cursor, "next_folder_cursor"),
  };
}

/** Check whether a page is still active after an uncertain lifecycle mutation. */
export async function isNotesPageActive(pageId: string): Promise<boolean> {
  const shell = await loadNotesWorkspaceShell({
    project_id: null,
    selected_page_id: pageId,
    expanded_page_ids: [],
    seed_page_ids: [],
    page_cursor: "end",
    folder_cursor: "end",
  });
  return shell.pages.some((page) => page.id === pageId);
}

export async function listNotesDestinationCandidates(
  projectId: string | null,
  cursor: string | null = null,
  query = "",
): Promise<NotesWorkspaceShell> {
  return loadNotesWorkspaceShell({
    project_id: projectId,
    expanded_page_ids: [],
    seed_page_ids: [],
    selected_page_id: null,
    page_cursor: cursor,
    folder_cursor: "end",
    destination_candidates: true,
    page_query: query,
  });
}

export async function createNotesFolder(folder: NotesFolderCreate): Promise<NotesFolder> {
  const dbUrl = await ensureDbUrl();
  return mapNotesFolderDto(await invokeNotesMutation("notes_create_folder", { dbUrl, folder }));
}

export async function updateNotesFolder(
  folderId: string,
  update: NotesFolderUpdate,
): Promise<NotesFolder> {
  const dbUrl = await ensureDbUrl();
  return mapNotesFolderDto(
    await invokeNotesMutation("notes_update_folder", { dbUrl, folderId, update }),
  );
}

export async function deleteNotesFolder(folderId: string): Promise<string> {
  const dbUrl = await ensureDbUrl();
  const deletedFolderId = await invokeNotesMutation("notes_delete_folder", { dbUrl, folderId });
  if (typeof deletedFolderId !== "string") {
    throw new Error("notes_delete_folder returned a non-string payload");
  }
  return deletedFolderId;
}

export async function listTrashedNotesPages(
  request: NotesPageSummaryWindowRequest = {},
): Promise<NotesPageSummaryWindow> {
  const dbUrl = await ensureDbUrl();
  return mapPageSummaryWindow(
    await invoke<unknown>("notes_list_trashed_pages", { dbUrl, request }),
    "notes_list_trashed_pages",
  );
}

export async function listArchivedNotesPages(
  request: NotesPageSummaryWindowRequest = {},
): Promise<NotesPageSummaryWindow> {
  const dbUrl = await ensureDbUrl();
  return mapPageSummaryWindow(
    await invoke<unknown>("notes_list_archived_pages", { dbUrl, request }),
    "notes_list_archived_pages",
  );
}

function mapPageSummaryWindow(value: unknown, command: string): NotesPageSummaryWindow {
  const record = notesWorkspaceShellRecord(value, `${command} response`);
  if (!Array.isArray(record.pages)) throw new Error(`${command} returned invalid pages`);
  return {
    pages: record.pages.map(mapPageSummary),
    total_count: shellNumber(record.total_count, `${command}.total_count`),
    next_cursor: shellNullableString(record.next_cursor, `${command}.next_cursor`),
  };
}

export async function listNotesSidebarPages(
  request: NotesSidebarPagesRequest,
): Promise<NotesSidebarPageList> {
  const dbUrl = await ensureDbUrl();
  const record = notesWorkspaceShellRecord(
    await invoke<unknown>("notes_list_sidebar_pages", { dbUrl, request }),
    "notes_list_sidebar_pages response",
  );
  if (!Array.isArray(record.pages)) {
    throw new Error("notes_list_sidebar_pages returned invalid pages");
  }
  return {
    pages: record.pages.map(mapPageSummary),
    page_ids_with_children: shellStringArray(record.page_ids_with_children, "page_ids_with_children"),
    missing_parent_page_ids: shellStringArray(record.missing_parent_page_ids, "missing_parent_page_ids"),
    trashed_parent_page_ids: shellStringArray(record.trashed_parent_page_ids, "trashed_parent_page_ids"),
  };
}
