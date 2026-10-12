import type { NotesPage } from "$lib/notes/types";

const FAVORITE_PAGE_IDS_CONFIG_KEY = "notes.favoritePageIds";
const RECENT_PAGE_IDS_CONFIG_KEY = "notes.recentPageIds";
const DEFAULT_RECENT_PAGE_LIMIT = 8;

/** Return the config key that stores favorited Notes page ids. */
export function notesFavoritePageIdsConfigKey(): string {
  return FAVORITE_PAGE_IDS_CONFIG_KEY;
}

/** Return the config key that stores recently opened Notes page ids. */
export function notesRecentPageIdsConfigKey(): string {
  return RECENT_PAGE_IDS_CONFIG_KEY;
}

/** Parse a persisted page id list from config defensively. */
export function parseStoredNotesPageIdList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return [...new Set(value.filter((item): item is string => typeof item === "string"))];
}

/** Add or remove a page id from the favorite list without changing unrelated ids. */
export function setNotesPageFavoriteId(
  pageIds: readonly string[],
  pageId: string,
  favorited: boolean,
): string[] {
  const normalizedPageId = pageId.trim();
  if (!normalizedPageId) return [...pageIds];
  if (!favorited) return pageIds.filter((candidate) => candidate !== normalizedPageId);
  return [normalizedPageId, ...pageIds.filter((candidate) => candidate !== normalizedPageId)];
}

/** Record a recently opened page id at the front of the list. */
export function recordRecentNotesPageId(
  pageIds: readonly string[],
  pageId: string,
  limit = DEFAULT_RECENT_PAGE_LIMIT,
): string[] {
  const normalizedPageId = pageId.trim();
  if (!normalizedPageId || limit <= 0) return [];
  return [normalizedPageId, ...pageIds.filter((candidate) => candidate !== normalizedPageId)]
    .slice(0, limit);
}

/** Return pages in stored id order, dropping stale ids. */
export function orderedNotesPagesById(
  pages: readonly NotesPage[],
  pageIds: readonly string[],
): NotesPage[] {
  const pageById = new Map(pages.map((page) => [page.id, page]));
  return pageIds.flatMap((pageId) => {
    const page = pageById.get(pageId);
    return page ? [page] : [];
  });
}
