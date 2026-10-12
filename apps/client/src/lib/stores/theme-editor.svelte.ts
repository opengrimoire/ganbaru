import type { ThemeId } from "$lib/themes";
import { getTheme } from "./theme.svelte";
import {
  themeEditorSessionHasChanges,
  type ThemeEditorSessionState,
} from "$lib/themes/editor-session";

/**
 * The single active theme editor session. Edits during the session live in
 * the theme store's in-memory `$state` only; SQLite is not touched until
 * `commit` calls `persistThemeToDb`. The snapshot (when present) is the
 * serialized theme at session open and lets `cancel` roll back the
 * in-memory edits without writing to disk. For themes created for this
 * session (`createdFresh` is true), cancel discards the in-memory theme
 * entirely so backing out leaves no orphan behind.
 */
type EditorSession = ThemeEditorSessionState;

let session = $state<EditorSession | undefined>(undefined);

export interface OpenEditorOptions {
  snapshot?: string;
  createdFresh?: boolean;
  previousActiveId: ThemeId;
}

export function getThemeEditor() {
  return {
    get editingId(): ThemeId | undefined {
      return session?.editingId;
    },
    get isActive(): boolean {
      return session !== undefined;
    },
    get hasUnsavedChanges(): boolean {
      return themeEditorSessionHasChanges(
        session,
        session ? getTheme().exportTheme(session.editingId) : undefined,
      );
    },
    open(id: ThemeId, options: OpenEditorOptions): void {
      session = {
        editingId: id,
        snapshot: options.snapshot,
        createdFresh: options.createdFresh ?? false,
        previousActiveId: options.previousActiveId,
      };
    },
    /** Flush the in-memory edits to SQLite and end the session. The edited theme stays active. */
    async commit(): Promise<void> {
      if (!session) return;
      const committingSession = session;
      const { editingId } = committingSession;
      await getTheme().persistThemeToDb(editingId);
      if (session === committingSession) session = undefined;
    },
    /**
     * Roll the session back and reinstate the previously active theme. Fresh
     * themes are dropped from memory. Existing themes get their pre-edit JSON
     * snapshot replayed in memory only; built-in previews pass no snapshot, so
     * there is nothing to restore.
     */
    async cancel(): Promise<void> {
      if (!session) return;
      const themeStore = getTheme();
      const { editingId, snapshot, createdFresh, previousActiveId } = session;
      session = undefined;
      if (createdFresh) {
        if (themeStore.id === editingId) themeStore.setTheme(previousActiveId);
        themeStore.discardFreshTheme(editingId);
      } else {
        if (snapshot !== undefined) {
          themeStore.restoreThemeFromSnapshot(editingId, snapshot);
        }
        if (themeStore.id !== previousActiveId) {
          themeStore.setTheme(previousActiveId);
        }
      }
    },
  };
}
