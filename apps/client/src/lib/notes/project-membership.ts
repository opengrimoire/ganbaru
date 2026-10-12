import type { NotesPage } from "./types";
import type { NotesPageOpenMode } from "$lib/notes/pages/open-mode";

export const NOTES_PAGE_PROJECT_ID_PROPERTY = "__ganbaru_project_id";

export interface NotesCreatePageOptions {
  projectId?: string | null;
  folderId?: string | null;
  openMode?: NotesPageOpenMode;
}

export function normalizeNotesProjectId(projectId: string | null | undefined): string | null {
  const trimmed = projectId?.trim() ?? "";
  return trimmed.length > 0 ? trimmed : null;
}

export function notesPageProjectId(page: Pick<NotesPage, "properties">): string | null {
  const value = page.properties[NOTES_PAGE_PROJECT_ID_PROPERTY];
  return typeof value === "string" ? normalizeNotesProjectId(value) : null;
}

export function notesPageMatchesProject(
  page: Pick<NotesPage, "properties">,
  projectId: string | null | undefined,
): boolean {
  const normalizedProjectId = normalizeNotesProjectId(projectId);
  return !normalizedProjectId || notesPageProjectId(page) === normalizedProjectId;
}

export function notesPagesForProject(
  pages: readonly NotesPage[],
  projectId: string | null | undefined,
): NotesPage[] {
  const normalizedProjectId = normalizeNotesProjectId(projectId);
  if (!normalizedProjectId) return [...pages];
  return pages.filter((page) => notesPageProjectId(page) === normalizedProjectId);
}

/** Add destination-only pages without replacing newer workspace page records. */
export function mergeNotesNavigationPages(
  workspacePages: readonly NotesPage[],
  destinationPages: readonly NotesPage[],
): NotesPage[] {
  return [...new Map(
    [...destinationPages, ...workspacePages].map((page) => [page.id, page]),
  ).values()];
}

export function notesPageProjectProperties(
  projectId: string | null | undefined,
): Record<string, unknown> | undefined {
  const normalizedProjectId = normalizeNotesProjectId(projectId);
  if (!normalizedProjectId) return undefined;
  return {
    [NOTES_PAGE_PROJECT_ID_PROPERTY]: normalizedProjectId,
  };
}
