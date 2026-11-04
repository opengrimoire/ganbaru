import { createVaultChangeChannel } from "$lib/windows/vault-change-channel";

const NOTES_SYNC_CHANGED_EVENT = "ganbaru-notes-sync-changed";
const NOTES_SYNC_CHANGE_KEY = "notes";
const notesSyncChanges = createVaultChangeChannel("notes-sync-changed");

/** Tells this window and the other windows of the vault that sync recovery changed Notes rows outside them. */
export function notifyNotesSyncChanged(): void {
  if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent(NOTES_SYNC_CHANGED_EVENT));
  notesSyncChanges.publish([NOTES_SYNC_CHANGE_KEY]);
}

/** Listens for {@link notifyNotesSyncChanged} in this window, and returns a function that stops listening. */
export function onNotesSyncChanged(listener: () => void): () => void {
  if (typeof window === "undefined") return () => undefined;
  window.addEventListener(NOTES_SYNC_CHANGED_EVENT, listener);
  return () => window.removeEventListener(NOTES_SYNC_CHANGED_EVENT, listener);
}

/** Listens for {@link notifyNotesSyncChanged} from other windows of the vault, and resolves to a function that stops listening. */
export function listenForNotesSyncChanges(onChange: () => void): Promise<() => void> {
  return notesSyncChanges.listen(() => onChange());
}
