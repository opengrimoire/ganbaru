import { invoke } from "@tauri-apps/api/core";
import { publishNotesDatabaseChange } from "$lib/notes/database/window-sync";
import { applyNotesProjectHistoryMutationDeadline } from "$lib/notes/history/project-history-scheduler";

/** Invoke a Notes mutation and apply its project-history checkpoint envelope. */
export async function invokeNotesMutation(
  command: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  const result = await invoke<unknown>(command, args);
  publishNotesDatabaseChange();
  if (typeof result !== "object" || result === null || Array.isArray(result)) {
    throw new Error(`${command} returned an invalid Notes mutation envelope`);
  }
  const record = result as Record<string, unknown>;
  if (!Object.hasOwn(record, "value") || !Object.hasOwn(record, "nextHistoryCheckpointAt")) {
    throw new Error(`${command} returned an invalid Notes mutation envelope`);
  }
  const deadline = record.nextHistoryCheckpointAt;
  if (deadline !== null && typeof deadline !== "string") {
    throw new Error(`${command} returned an invalid Notes history deadline`);
  }
  applyNotesProjectHistoryMutationDeadline(deadline);
  return record.value;
}
