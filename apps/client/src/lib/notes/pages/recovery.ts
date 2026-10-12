import type { NotesPage } from "$lib/notes/types";

/**
 * Report whether a trashed page is known to restore as a workspace page.
 */
export function notesPageRestoresToWorkspace(
  page: NotesPage,
  trashedPages: readonly NotesPage[],
): boolean {
  const parent = page.parent;
  if (parent.type !== "page_id") return false;
  return trashedPages.some((candidate) => candidate.id === parent.page_id);
}
