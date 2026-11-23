import { listen } from "@tauri-apps/api/event";
import { AUTOMATIC_BACKUP_CHANGED_EVENT } from "./automatic-backup";

/**
 * Runs `onChange` whenever Rust reports that the automatic backup status changed.
 *
 * @param onChange - Called with no arguments after each change.
 * @returns A promise of the function that stops listening.
 */
export function listenForAutomaticBackupChanges(onChange: () => void): Promise<() => void> {
  return listen(AUTOMATIC_BACKUP_CHANGED_EVENT, () => onChange());
}
