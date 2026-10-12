import {
  notesSelectedPageConfigKey,
  parseStoredNotesPageId,
} from "$lib/notes/pages/selection";
import {
  notesFavoritePageIdsConfigKey,
  notesRecentPageIdsConfigKey,
  parseStoredNotesPageIdList,
} from "$lib/notes/pages/navigation";
import {
  notesSidebarCollapsedFolderIdsConfigKey,
  parseStoredNotesSidebarCollapsedFolderIds,
} from "$lib/notes/navigation/tree";
import { getConfigKey, setConfigKey } from "$lib/vault/config";

export const notesSelectedPageKey = notesSelectedPageConfigKey();
export const notesFavoritePageIdsKey = notesFavoritePageIdsConfigKey();
export const notesRecentPageIdsKey = notesRecentPageIdsConfigKey();
export const notesSidebarCollapsedFolderIdsKey = notesSidebarCollapsedFolderIdsConfigKey();

/**
 * Load the persisted Notes page selection id.
 */
export function initialNotesSelectedPageId(): string | null {
  return parseStoredNotesPageId(getConfigKey<unknown>(notesSelectedPageKey, undefined));
}

/**
 * Load the persisted favorite Notes page ids.
 */
export function initialNotesFavoritePageIds(): string[] {
  return parseStoredNotesPageIdList(getConfigKey<unknown>(notesFavoritePageIdsKey, undefined));
}

/**
 * Load the persisted recent Notes page ids.
 */
export function initialNotesRecentPageIds(): string[] {
  return parseStoredNotesPageIdList(getConfigKey<unknown>(notesRecentPageIdsKey, undefined));
}

/**
 * Load the persisted collapsed Notes folder ids.
 */
export function initialNotesSidebarCollapsedFolderIds(): string[] {
  return parseStoredNotesSidebarCollapsedFolderIds(
    getConfigKey<unknown>(notesSidebarCollapsedFolderIdsKey, undefined),
  );
}

/**
 * Persist the selected Notes page id.
 */
export function saveNotesSelectedPageId(pageId: string | null): void {
  setConfigKey(notesSelectedPageKey, pageId ?? undefined);
}

/**
 * Persist the favorite Notes page ids.
 */
export function saveNotesFavoritePageIds(pageIds: readonly string[]): void {
  setConfigKey(notesFavoritePageIdsKey, pageIds.length > 0 ? [...pageIds] : undefined);
}

/**
 * Persist the recent Notes page ids.
 */
export function saveNotesRecentPageIds(pageIds: readonly string[]): void {
  setConfigKey(notesRecentPageIdsKey, pageIds.length > 0 ? [...pageIds] : undefined);
}

/**
 * Persist the collapsed Notes folder ids.
 */
export function saveNotesSidebarCollapsedFolderIds(folderIds: readonly string[]): void {
  setConfigKey(
    notesSidebarCollapsedFolderIdsKey,
    folderIds.length > 0 ? [...folderIds] : undefined,
  );
}
