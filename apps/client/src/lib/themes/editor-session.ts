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

/** An open editor session: editing in memory, or waiting for its save to land. */
export type ThemeEditorSession = ThemeEditorSessionState & { phase: "editing" | "committing" };

export type ThemeEditorSessionEvent =
  | { type: "commitStarted" }
  /** The save landed and closed the session. */
  | { type: "committed" }
  /** The save stopped at a conflict or failed, so the session stays open for another choice. */
  | { type: "commitStopped" }
  | { type: "cancelled" };

/** Open a session in the editing phase. */
export function openThemeEditorSession(state: ThemeEditorSessionState): ThemeEditorSession {
  return { ...state, phase: "editing" };
}

/**
 * Apply one editor session event; events that do not apply return the same session. A save in flight is the only
 * owner of the session until it settles, so a second save or a cancel waits for it instead of acting.
 */
export function transitionThemeEditorSession(
  session: ThemeEditorSession | undefined,
  event: ThemeEditorSessionEvent,
): ThemeEditorSession | undefined {
  if (!session) return session;
  switch (event.type) {
    case "commitStarted":
      return session.phase === "editing" ? { ...session, phase: "committing" } : session;
    case "committed":
      return session.phase === "committing" ? undefined : session;
    case "commitStopped":
      return session.phase === "committing" ? { ...session, phase: "editing" } : session;
    case "cancelled":
      return session.phase === "editing" ? undefined : session;
  }
}
