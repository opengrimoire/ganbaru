import type { ThemeId } from "$lib/themes";
import { getTheme } from "./theme.svelte";
import {
  openThemeEditorSession,
  themeEditorSessionHasChanges,
  transitionThemeEditorSession,
  type ThemeEditorSession,
  type ThemeEditorSessionEvent,
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
let session = $state.raw<ThemeEditorSession | undefined>(undefined);
/** The save of the committing session, which a second commit or a cancel waits for. */
let commitInFlight: Promise<ThemeSaveOutcome> | undefined;

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
      session = openThemeEditorSession({
        editingId: id,
        snapshot: options.snapshot,
        createdFresh: options.createdFresh ?? false,
        previousActiveId: options.previousActiveId,
      });
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
     * deleted meanwhile, leaves a {@link saveNotice}. A commit while a save
     * is in flight returns that save instead of starting another.
     */
    commit(choice?: ThemeSaveChoice): Promise<ThemeSaveOutcome> {
      if (commitInFlight) return commitInFlight;
      if (!session) return Promise.resolve({ kind: "saved" });
      const committing = transitionThemeEditorSession(session, { type: "commitStarted" });
      if (!committing) return Promise.resolve({ kind: "saved" });
      session = committing;
      const save = runCommit(committing, choice);
      commitInFlight = save;
      void save.then(
        () => { if (commitInFlight === save) commitInFlight = undefined; },
        () => { if (commitInFlight === save) commitInFlight = undefined; },
      );
      return save;
    },
    /**
     * Roll the session back and reinstate the previously active theme. Fresh
     * themes are dropped from memory. Existing themes get their pre-edit JSON
     * snapshot replayed in memory only; built-in previews pass no snapshot, so
     * there is nothing to restore. A save in flight settles first, so a theme
     * it stored is kept and a save that stopped is rolled back.
     */
    async cancel(): Promise<void> {
      if (commitInFlight) await commitInFlight.catch(() => undefined);
      const cancelled = session;
      if (!cancelled) return;
      session = transitionThemeEditorSession(cancelled, { type: "cancelled" });
      if (session) return;
      const themeStore = getTheme();
      const { editingId, snapshot, createdFresh, previousActiveId } = cancelled;
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

/** Save one committing session and settle it: closed when the save lands, editing again when it stops or fails. */
async function runCommit(committing: ThemeEditorSession, choice: ThemeSaveChoice | undefined): Promise<ThemeSaveOutcome> {
  let outcome: ThemeSaveOutcome;
  try {
    outcome = await getTheme().persistThemeToDb(committing.editingId, choice);
  } catch (error: unknown) {
    settleCommit(committing, { type: "commitStopped" });
    throw error;
  }
  if (outcome.kind === "conflict") {
    settleCommit(committing, { type: "commitStopped" });
    return outcome;
  }
  if (outcome.kind === "savedAsCopy") saveNotice = outcome;
  settleCommit(committing, { type: "committed" });
  if (session === undefined) getTheme().setEditingTheme(undefined);
  return outcome;
}

/** Apply a commit result only to the session that started it, since a later open replaces it. */
function settleCommit(committing: ThemeEditorSession, event: ThemeEditorSessionEvent): void {
  if (session !== committing) return;
  session = transitionThemeEditorSession(committing, event);
}
