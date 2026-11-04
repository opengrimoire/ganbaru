import type { NotesPage } from "./types";
import type { NotesPageOpenMode } from "$lib/notes/pages/open-mode";

export interface NotesCreatePageOptions {
  projectId?: string | null;
  folderId?: string | null;
  openMode?: NotesPageOpenMode;
}

export function normalizeNotesProjectId(projectId: string | null | undefined): string | null {
  const trimmed = projectId?.trim() ?? "";
  return trimmed.length > 0 ? trimmed : null;
}

export function notesPageProjectId(page: Pick<NotesPage, "project_id">): string | null {
  return normalizeNotesProjectId(page.project_id);
}

export function notesPageMatchesProject(
  page: Pick<NotesPage, "project_id">,
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
