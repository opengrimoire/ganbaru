import {
  createWindowSyncEnvelope,
  isForeignWindowSyncEnvelope,
  isWindowSyncEnvelope,
} from "$lib/windows/sync";
import {
  emitWindowSync,
  listenWindowSync,
} from "$lib/windows/sync-transport";
import { listenSyncApplied } from "$lib/api/sync";
import { invalidateQuickNotesInitialSnapshot } from "$lib/quick-notes/initial-snapshot";

const QUICK_NOTES_SYNC_EVENT = "quick-notes-window-sync";
/** Replicated tables whose changes refresh Quick notes. */
const QUICK_NOTES_TABLES: ReadonlySet<string> = new Set(["quick_notes", "quick_note_tags"]);

interface QuickNotesSyncPayload {
  kind: "data-changed";
}

function isPayload(value: unknown): value is QuickNotesSyncPayload {
  return typeof value === "object" && value !== null
    && (value as Record<string, unknown>).kind === "data-changed";
}

const localChangeListeners = new Set<() => void>();

/** Runs `onChange` after this window publishes a Quick notes change. */
export function onLocalQuickNotesChange(onChange: () => void): () => void {
  localChangeListeners.add(onChange);
  return () => {
    localChangeListeners.delete(onChange);
  };
}

export function publishQuickNotesChanged(): void {
  invalidateQuickNotesInitialSnapshot();
  for (const listener of localChangeListeners) listener();
  void emitWindowSync(
    QUICK_NOTES_SYNC_EVENT,
    createWindowSyncEnvelope<QuickNotesSyncPayload>({ kind: "data-changed" }),
  ).catch((error: unknown) => console.warn("Quick notes window sync failed", error));
}

/** Whether replicated changes to these tables affect Quick notes views. */
export function touchesQuickNotes(tables: readonly string[]): boolean {
  return tables.some((table) => QUICK_NOTES_TABLES.has(table));
}

/** Runs `onChange` after another window or a linked device changes Quick notes. */
export async function listenForQuickNotesChanges(onChange: () => void): Promise<() => void> {
  const changed = (): void => {
    invalidateQuickNotesInitialSnapshot();
    onChange();
  };
  const stopWindows = await listenWindowSync<unknown>(QUICK_NOTES_SYNC_EVENT, (event) => {
    const envelope = event.payload;
    if (!isWindowSyncEnvelope(envelope, isPayload) || !isForeignWindowSyncEnvelope(envelope)) return;
    changed();
  });
  try {
    const stopReplicated = await listenSyncApplied((tables) => {
      if (touchesQuickNotes(tables)) changed();
    });
    return () => {
      stopWindows();
      stopReplicated();
    };
  } catch (error: unknown) {
    stopWindows();
    throw error;
  }
}
