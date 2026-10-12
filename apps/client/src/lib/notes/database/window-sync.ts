import { activeVaultIdentity } from "$lib/vault/active-vault";
import { createWindowSyncEnvelope, isForeignWindowSyncEnvelope, isWindowSyncEnvelope } from "$lib/windows/sync";
import { emitWindowSync, listenWindowSync } from "$lib/windows/sync-transport";
import { notesDatabaseSession } from "./session.svelte";

const DATABASE_CHANGED_EVENT = "notes-database-changed";
interface DatabaseChange { vaultId: string; }

function isDatabaseChange(value: unknown): value is DatabaseChange {
  return typeof value === "object" && value !== null
    && typeof (value as Record<string, unknown>).vaultId === "string";
}

/** Invalidate database reads locally and notify other windows in this vault. */
export function publishNotesDatabaseChange(): void {
  notesDatabaseSession.invalidate();
  const vaultId = activeVaultIdentity();
  if (!vaultId) return;
  void emitWindowSync(DATABASE_CHANGED_EVENT, createWindowSyncEnvelope({ vaultId }))
    .catch((error: unknown) => console.warn("Notes database window sync failed", error));
}

/** Keep inactive database snapshots aware of writes in sibling app windows. */
export function listenForNotesDatabaseChanges(): Promise<() => void> {
  return listenWindowSync<unknown>(DATABASE_CHANGED_EVENT, (event) => {
    const envelope = event.payload;
    if (!isWindowSyncEnvelope(envelope, isDatabaseChange)
      || !isForeignWindowSyncEnvelope(envelope)
      || envelope.payload.vaultId !== activeVaultIdentity()) return;
    notesDatabaseSession.invalidate();
  });
}
