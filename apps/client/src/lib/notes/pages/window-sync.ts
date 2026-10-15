import { activeVaultIdentity } from "$lib/vault/active-vault";
import { createWindowSyncEnvelope, isForeignWindowSyncEnvelope, isWindowSyncEnvelope } from "$lib/windows/sync";
import { emitWindowSync, listenWindowSync } from "$lib/windows/sync-transport";

const PAGE_CHANGED_EVENT = "notes-page-changed";

/** Quiet period that gathers a burst of saves from another window into one refresh. */
export const NOTES_PAGE_REFRESH_DELAY_MS = 300;

interface PageChange {
  vaultId: string;
  pageIds: string[];
}

function isPageChange(value: unknown): value is PageChange {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return typeof record.vaultId === "string"
    && Array.isArray(record.pageIds)
    && record.pageIds.every((id) => typeof id === "string");
}

/** Tell other windows of this vault that the content of these pages changed. */
export function publishNotesPageChanges(pageIds: Iterable<string>): void {
  const vaultId = activeVaultIdentity();
  const ids = [...new Set(pageIds)];
  if (!vaultId || ids.length === 0) return;
  void emitWindowSync(PAGE_CHANGED_EVENT, createWindowSyncEnvelope({ vaultId, pageIds: ids }))
    .catch((error: unknown) => console.warn("Notes page window sync failed", error));
}

/**
 * Collect page changes from other windows and hand them over once they go quiet for
 * {@link NOTES_PAGE_REFRESH_DELAY_MS}, so continuous typing elsewhere refreshes this window once per pause.
 */
export function listenForNotesPageChanges(onChange: (pageIds: ReadonlySet<string>) => void): Promise<() => void> {
  let pending = new Set<string>();
  let timer: ReturnType<typeof setTimeout> | null = null;
  const deliver = () => {
    timer = null;
    const pageIds = pending;
    pending = new Set();
    onChange(pageIds);
  };
  return listenWindowSync<unknown>(PAGE_CHANGED_EVENT, (event) => {
    const envelope = event.payload;
    if (!isWindowSyncEnvelope(envelope, isPageChange)
      || !isForeignWindowSyncEnvelope(envelope)
      || envelope.payload.vaultId !== activeVaultIdentity()) return;
    for (const pageId of envelope.payload.pageIds) pending.add(pageId);
    if (timer) clearTimeout(timer);
    timer = setTimeout(deliver, NOTES_PAGE_REFRESH_DELAY_MS);
  }).then((unlisten) => () => {
    if (timer) clearTimeout(timer);
    timer = null;
    unlisten();
  });
}
