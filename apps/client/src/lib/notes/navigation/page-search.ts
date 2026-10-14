import { notesPageProjectId } from "$lib/notes/project-membership";
import type { NotesPage, NotesSearchResult } from "$lib/notes/types";

/** Longest query the Notes search index accepts. */
export const NOTES_SEARCH_QUERY_MAX_CHARS = 200;

/** Notes of one project that match a note search, or notes outside any project when `project` is null. */
export interface NotesPageSearchGroup<TProject extends { id: string }> {
  project: TProject | null;
  pages: NotesPage[];
}

/** Whether the Notes search index can run a query: it needs a letter or digit and fits the length limit. */
export function notesPageSearchQueryIsSearchable(query: string): boolean {
  const trimmed = query.trim();
  return /[\p{L}\p{N}]/u.test(trimmed) && [...trimmed].length <= NOTES_SEARCH_QUERY_MAX_CHARS;
}

/**
 * Group note matches by project for a note picker: one row per note in search rank order, only matches on the note
 * itself (not its blocks or comments), projects in the given order, and notes outside any project last. Notes of
 * projects that are not listed are left out.
 */
export function notesPageSearchGroups<TProject extends { id: string }>(
  results: readonly NotesSearchResult[],
  projects: readonly TProject[],
): NotesPageSearchGroup<TProject>[] {
  const pagesByProjectId = new Map<string, NotesPage[]>();
  const pagesWithoutProject: NotesPage[] = [];
  const seenPageIds = new Set<string>();
  for (const result of results) {
    if (result.type !== "page" || seenPageIds.has(result.page.id)) continue;
    seenPageIds.add(result.page.id);
    const projectId = notesPageProjectId(result.page);
    if (!projectId) {
      pagesWithoutProject.push(result.page);
      continue;
    }
    const projectPages = pagesByProjectId.get(projectId) ?? [];
    projectPages.push(result.page);
    pagesByProjectId.set(projectId, projectPages);
  }
  const groups: NotesPageSearchGroup<TProject>[] = projects.flatMap((project) => {
    const pages = pagesByProjectId.get(project.id);
    return pages ? [{ project, pages }] : [];
  });
  if (pagesWithoutProject.length > 0) groups.push({ project: null, pages: pagesWithoutProject });
  return groups;
}
