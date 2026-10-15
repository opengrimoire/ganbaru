import { activeVaultIdentity } from "$lib/vault/active-vault";
import { createWindowSyncEnvelope, isForeignWindowSyncEnvelope, isWindowSyncEnvelope } from "./sync";
import { emitWindowSync, listenWindowSync } from "$lib/windows/sync-transport";

/** Quiet period that gathers a burst of writes from another window into one refresh. */
export const VAULT_CHANGE_QUIET_MS = 300;

interface VaultChange {
  vaultId: string;
  keys: string[];
}

function isVaultChange(value: unknown): value is VaultChange {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return typeof record.vaultId === "string"
    && Array.isArray(record.keys)
    && record.keys.every((key) => typeof key === "string");
}

/** Tells other windows of the active vault which records of one domain changed. */
export interface VaultChangeChannel {
  /** Announce changed record keys; does nothing without an active vault or keys. */
  publish: (keys: Iterable<string>) => void;
  /**
   * Collect changes from other windows of the active vault and hand them over once they go quiet, so continuous
   * editing elsewhere refreshes this window once per pause. Resolves to a function that stops listening.
   */
  listen: (onChange: (keys: ReadonlySet<string>) => void) => Promise<() => void>;
}

/** Create a change channel for one domain, identified by its window event name. */
export function createVaultChangeChannel(eventName: string, quietMs = VAULT_CHANGE_QUIET_MS): VaultChangeChannel {
  return {
    publish(keys) {
      const vaultId = activeVaultIdentity();
      const unique = [...new Set(keys)];
      if (!vaultId || unique.length === 0) return;
      void emitWindowSync(eventName, createWindowSyncEnvelope({ vaultId, keys: unique }))
        .catch((error: unknown) => console.warn(`Window sync ${eventName} failed`, error));
    },
    listen(onChange) {
      let pending = new Set<string>();
      let timer: ReturnType<typeof setTimeout> | null = null;
      const deliver = () => {
        timer = null;
        const keys = pending;
        pending = new Set();
        onChange(keys);
      };
      return listenWindowSync<unknown>(eventName, (event) => {
        const envelope = event.payload;
        if (!isWindowSyncEnvelope(envelope, isVaultChange)
          || !isForeignWindowSyncEnvelope(envelope)
          || envelope.payload.vaultId !== activeVaultIdentity()) return;
        for (const key of envelope.payload.keys) pending.add(key);
        if (timer) clearTimeout(timer);
        timer = setTimeout(deliver, quietMs);
      }).then((unlisten) => () => {
        if (timer) clearTimeout(timer);
        timer = null;
        unlisten();
      });
    },
  };
}
