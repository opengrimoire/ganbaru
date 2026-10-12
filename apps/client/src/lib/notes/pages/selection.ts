import type { NotesPage } from "$lib/notes/types";

const SELECTED_PAGE_CONFIG_KEY = "notes.selectedPageId";

export function notesSelectedPageConfigKey(): string {
  return SELECTED_PAGE_CONFIG_KEY;
}

export function parseStoredNotesPageId(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

/** Return the restored page id only when the page still exists. */
export function restoredNotesPageSelection(
  pageId: string | null,
  pages: readonly { id: string }[],
): string | null {
  const normalizedPageId = pageId?.trim();
  if (!normalizedPageId) return null;
  return pages.some((page) => page.id === normalizedPageId) ? normalizedPageId : null;
}

/** Pick the page that should become selected after a page leaves the visible list. */
export function nextSelectedNotesPageId(
  pages: readonly { id: string }[],
  removedPageId: string,
): string | null {
  const index = pages.findIndex((page) => page.id === removedPageId);
  const remaining = pages.filter((page) => page.id !== removedPageId);
  if (remaining.length === 0) return null;
  if (index < 0) return remaining[0]?.id ?? null;
  return remaining[Math.min(index, remaining.length - 1)]?.id ?? null;
}

/** Find the visible page subtree that follows a page into Trash. */
export function notesPageSubtreeIds(
  pages: readonly Pick<NotesPage, "id" | "parent">[],
  rootPageId: string,
): Set<string> {
  const childrenByParentId = new Map<string, string[]>();
  for (const page of pages) {
    if (page.parent.type !== "page_id") continue;
    const children = childrenByParentId.get(page.parent.page_id) ?? [];
    children.push(page.id);
    childrenByParentId.set(page.parent.page_id, children);
  }
  const ids = new Set([rootPageId]);
  const queue = [rootPageId];
  for (let index = 0; index < queue.length; index += 1) {
    for (const childId of childrenByParentId.get(queue[index] ?? "") ?? []) {
      if (ids.has(childId)) continue;
      ids.add(childId);
      queue.push(childId);
    }
  }
  return ids;
}

export function filterNotesPagesByTitle<Page>(
  pages: readonly Page[],
  query: string,
  titleForPage: (page: Page) => string,
): Page[] {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) return [...pages];
  return pages.filter((page) => titleForPage(page).toLowerCase().includes(normalizedQuery));
}
