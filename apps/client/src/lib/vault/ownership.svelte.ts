import { getVaultOwnershipStatus, type VaultOwnershipStatus } from "./state";

/** Called with the new status when the owner of the same vault changes, as after a live handoff. */
export type VaultOwnerChangeListener = (status: VaultOwnershipStatus) => void;

/**
 * Whether `next` names another owner of the vault `previous` described: a different owner device or ownership generation.
 * A different vault is a vault switch, not an ownership change.
 */
export function vaultOwnerChanged(previous: VaultOwnershipStatus, next: VaultOwnershipStatus): boolean {
  return previous.vaultId === next.vaultId
    && (previous.ownerDeviceId !== next.ownerDeviceId || previous.generation !== next.generation);
}

/** Shell-level ownership state used to guard vault-writing interactions. */
class VaultOwnershipController {
  status = $state.raw<VaultOwnershipStatus | null>(null);
  loading = $state(false);
  private readonly ownerChangeListeners = new Set<VaultOwnerChangeListener>();

  get canWrite(): boolean {
    return this.status?.canWrite ?? false;
  }

  async refresh(): Promise<void> {
    this.loading = true;
    try {
      const previous = this.status;
      const next = await getVaultOwnershipStatus();
      this.status = next;
      if (previous && vaultOwnerChanged(previous, next)) {
        for (const listener of this.ownerChangeListeners) listener(next);
      }
    } finally {
      this.loading = false;
    }
  }

  /**
   * Calls `listener` each time a refresh finds that the vault's owner changed while the vault stayed open.
   * @returns A function that removes the listener.
   */
  onOwnerChange(listener: VaultOwnerChangeListener): () => void {
    this.ownerChangeListeners.add(listener);
    return () => this.ownerChangeListeners.delete(listener);
  }

  clear(): void {
    this.status = null;
  }
}

const ownership = new VaultOwnershipController();

export function getVaultOwnership(): VaultOwnershipController {
  return ownership;
}
