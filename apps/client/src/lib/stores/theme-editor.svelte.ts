import type { ThemeId } from "$lib/themes";
import { getTheme } from "./theme.svelte";
import {
  themeEditorSessionHasChanges,
  type ThemeEditorSessionState,
} from "$lib/themes/editor-session";
import type { ThemeSaveChoice, ThemeSaveOutcome } from "$lib/themes/editor-save";

/**
 * The single active theme editor session. Edits during the session live in
 * the theme store's in-memory `$state` only; SQLite is not touched until
 * `commit` calls `persistThemeToDb`. The snapshot (when present) is the
 * serialized theme at session open and lets `cancel` roll back the
 * in-memory edits without writing to disk. For themes created for this
 * session (`createdFresh` is true), cancel discards the in-memory theme
 * entirely so backing out leaves no orphan behind. While a session is open,
 * theme reloads after sync keep the edited theme's buffer and run again
 * when the session ends.
 */
type EditorSession = ThemeEditorSessionState;

let session = $state<EditorSession | undefined>(undefined);

type SavedAsCopy = Extract<ThemeSaveOutcome, { kind: "savedAsCopy" }>;

let saveNotice = $state<SavedAsCopy | undefined>(undefined);

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
      getTheme().setEditingTheme(id);
    },
    /**
     * The copy a save made instead of updating the edited theme, until
     * {@link takeSaveNotice} reports it.
     */
    get saveNotice(): SavedAsCopy | undefined {
      return saveNotice;
    },
    /** Return the pending save notice once and clear it. */
    takeSaveNotice(): SavedAsCopy | undefined {
      const notice = saveNotice;
      saveNotice = undefined;
      return notice;
    },
    /**
     * Flush the in-memory edits to SQLite and end the session. The saved
     * theme stays active. A conflict with a remote edit writes nothing and
     * keeps the session open until a later commit passes a `choice`. A save
     * that made a copy, because the person chose one or the theme was
     * deleted meanwhile, leaves a {@link saveNotice}.
     */
    async commit(choice?: ThemeSaveChoice): Promise<ThemeSaveOutcome> {
      if (!session) return { kind: "saved" };
      const committingSession = session;
      const { editingId } = committingSession;
      const outcome = await getTheme().persistThemeToDb(editingId, choice);
      if (outcome.kind === "conflict") return outcome;
      if (outcome.kind === "savedAsCopy") saveNotice = outcome;
      if (session === committingSession) {
        session = undefined;
        getTheme().setEditingTheme(undefined);
      }
      return outcome;
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
      themeStore.setEditingTheme(undefined);
    },
  };
}
