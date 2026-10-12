export type ActiveVaultIdentityListener = (
  previousVaultId: string | null,
  nextVaultId: string | null,
) => void;

let activeVaultId: string | null = null;
const listeners = new Set<ActiveVaultIdentityListener>();

/** Return the validated vault identity, or null before a vault is active. */
export function activeVaultIdentity(): string | null {
  return activeVaultId;
}

/** Returns the active vault identity after startup validation. */
export function requireActiveVaultIdentity(): string {
  if (!activeVaultId) {
    throw new Error("active vault identity is unavailable");
  }
  return activeVaultId;
}

/** Updates the process-local vault identity and notifies cache owners on change. */
export function setActiveVaultIdentity(vaultId: string | null): void {
  const normalized = vaultId?.trim() || null;
  if (normalized === activeVaultId) return;
  const previousVaultId = activeVaultId;
  activeVaultId = normalized;
  for (const listener of listeners) listener(previousVaultId, activeVaultId);
}

/** Subscribes to validated active-vault identity changes. */
export function onActiveVaultIdentityChange(
  listener: ActiveVaultIdentityListener,
): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
