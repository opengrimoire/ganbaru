import type { ThemeId } from "$lib/themes";

export interface ThemeEditorSessionState {
  editingId: ThemeId;
  snapshot: string | undefined;
  createdFresh: boolean;
  previousActiveId: ThemeId;
}

/** Return whether a session contains changes that Cancel would discard. */
export function themeEditorSessionHasChanges(
  session: Readonly<ThemeEditorSessionState> | undefined,
  currentSnapshot: string | undefined,
): boolean {
  if (!session) return false;
  if (session.createdFresh) return true;
  if (session.snapshot === undefined) return false;
  return session.snapshot !== currentSnapshot;
}
