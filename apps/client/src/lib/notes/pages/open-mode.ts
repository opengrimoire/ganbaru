export type NotesPageOpenMode = "side" | "center" | "full";

export const NOTES_PAGE_OPEN_MODES = ["side", "center", "full"] as const;

export const DEFAULT_NOTES_PAGE_OPEN_MODE: NotesPageOpenMode = "center";

export interface NotesPageOpenModeSelectionContext {
  requestedOpenMode?: NotesPageOpenMode;
  currentOpenMode: NotesPageOpenMode;
  hasOpenPage: boolean;
}

export function isNotesPageOpenMode(value: unknown): value is NotesPageOpenMode {
  return typeof value === "string"
    && NOTES_PAGE_OPEN_MODES.includes(value as NotesPageOpenMode);
}

export function notesDefaultOpenModeForProject(
  globalDefaultOpenMode: NotesPageOpenMode,
  projectDefaultOpenMode: NotesPageOpenMode | null | undefined,
): NotesPageOpenMode {
  return projectDefaultOpenMode ?? globalDefaultOpenMode;
}

export function notesContextualPageOpenMode(
  currentOpenMode: NotesPageOpenMode,
  defaultOpenMode: NotesPageOpenMode,
): NotesPageOpenMode {
  return currentOpenMode === "full" ? defaultOpenMode : currentOpenMode;
}

/**
 * Mode for a page opened by navigation rather than from inside another note: a full page when none is open, or the
 * open page's mode otherwise. The configured default only applies to contextual opens.
 */
export function notesPageOpenModeForSelection({
  requestedOpenMode,
  currentOpenMode,
  hasOpenPage,
}: NotesPageOpenModeSelectionContext): NotesPageOpenMode {
  if (requestedOpenMode) return requestedOpenMode;
  return hasOpenPage ? currentOpenMode : "full";
}
