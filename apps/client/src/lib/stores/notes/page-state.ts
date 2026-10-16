import {
  notesSelectedPageConfigKey,
  parseStoredNotesPageId,
} from "$lib/notes/pages/selection";
import {
  notesCustomOrderByProjectConfigKey,
  notesFavoriteFolderIdsConfigKey,
  notesFavoritePageIdsConfigKey,
  notesRecentPageIdsConfigKey,
  notesSidebarOrderByProjectConfigKey,
  parseStoredNotesPageIdList,
} from "$lib/notes/pages/navigation";
import {
  notesSidebarCollapsedFolderIdsConfigKey,
  parseStoredNotesSidebarCollapsedFolderIds,
} from "$lib/notes/navigation/tree";
import { parseCustomOrders, parseSidebarSortOrderOverrides, type SidebarSortOrder } from "$lib/utils/sidebar-order";
import { getConfigKey, setConfigKey } from "$lib/vault/config";

export const notesSelectedPageKey = notesSelectedPageConfigKey();
export const notesFavoritePageIdsKey = notesFavoritePageIdsConfigKey();
export const notesFavoriteFolderIdsKey = notesFavoriteFolderIdsConfigKey();
export const notesRecentPageIdsKey = notesRecentPageIdsConfigKey();
export const notesSidebarCollapsedFolderIdsKey = notesSidebarCollapsedFolderIdsConfigKey();
export const notesSidebarOrderByProjectKey = notesSidebarOrderByProjectConfigKey();
export const notesCustomOrderByProjectKey = notesCustomOrderByProjectConfigKey();

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
 * Load the persisted favorite Notes folder ids.
 */
export function initialNotesFavoriteFolderIds(): string[] {
  return parseStoredNotesPageIdList(getConfigKey<unknown>(notesFavoriteFolderIdsKey, undefined));
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
 * Persist the favorite Notes folder ids.
 */
export function saveNotesFavoriteFolderIds(folderIds: readonly string[]): void {
  setConfigKey(notesFavoriteFolderIdsKey, folderIds.length > 0 ? [...folderIds] : undefined);
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

/**
 * Load the per-project Notes sidebar order overrides.
 */
export function initialNotesSidebarOrderOverrides(): Record<string, SidebarSortOrder> {
  return parseSidebarSortOrderOverrides(getConfigKey<unknown>(notesSidebarOrderByProjectKey, undefined));
}

/**
 * Persist the per-project Notes sidebar order overrides.
 */
export function saveNotesSidebarOrderOverrides(overrides: Readonly<Record<string, SidebarSortOrder>>): void {
  setConfigKey(notesSidebarOrderByProjectKey, Object.keys(overrides).length > 0 ? { ...overrides } : undefined);
}

/**
 * Load the custom order of each project's Notes folders and notes.
 */
export function initialNotesCustomOrders(): Record<string, string[]> {
  return parseCustomOrders(getConfigKey<unknown>(notesCustomOrderByProjectKey, undefined));
}

/**
 * Persist the custom order of each project's Notes folders and notes.
 */
export function saveNotesCustomOrders(orders: Readonly<Record<string, readonly string[]>>): void {
  setConfigKey(notesCustomOrderByProjectKey, Object.keys(orders).length > 0 ? { ...orders } : undefined);
}
