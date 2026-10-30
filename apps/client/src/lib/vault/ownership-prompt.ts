import {
  readPairingStatus,
  receiveDesktopBundle,
  requestOwnerBundle,
  type PairingStatus,
} from "$lib/api/vault-handoff";

export type VaultOwnershipPlatform = "desktop" | "android";

export const VAULT_OWNERSHIP_TRANSITION_STORAGE_KEY = "ganbaru:vault-ownership-transition";

export interface VaultOwnershipTransitionSnapshot {
  title: string;
  description: string;
  actionLabel: string;
  secondaryLabel: string;
}

export interface VaultOwnershipRequestDependencies {
  readStatus: () => Promise<PairingStatus>;
  receiveFromDesktop: () => Promise<{ activated: boolean; inProgress: boolean }>;
  requestFromCoordinator: () => Promise<void>;
  wait: () => Promise<void>;
  attempts: number;
}

const defaultDependencies: VaultOwnershipRequestDependencies = {
  readStatus: readPairingStatus,
  receiveFromDesktop: () => receiveDesktopBundle("ownership"),
  requestFromCoordinator: () => requestOwnerBundle("ownership"),
  wait: () => new Promise<void>((resolve) => window.setTimeout(resolve, 1_000)),
  attempts: 90,
};

/** Returns whether a linked device should explain that its vault is read-only. */
export function shouldPresentVaultOwnershipPrompt(status: PairingStatus | null): boolean {
  return status?.linked === true && status.canWrite === false;
}

/** Preserves the ownership decision surface across the activating webview reload. */
export function beginVaultOwnershipTransition(
  snapshot: VaultOwnershipTransitionSnapshot,
  storage: Storage = window.sessionStorage,
): void {
  try {
    storage.setItem(VAULT_OWNERSHIP_TRANSITION_STORAGE_KEY, JSON.stringify(snapshot));
  } catch {
    // The transfer remains safe when session storage is unavailable.
  }
}

/** Removes a transition snapshot when the ownership request fails before reloading. */
export function cancelVaultOwnershipTransition(
  storage: Storage = window.sessionStorage,
): void {
  try {
    storage.removeItem(VAULT_OWNERSHIP_TRANSITION_STORAGE_KEY);
  } catch {
    // Session storage is an optional presentation aid.
  }
}

/** Requests vault ownership and waits until the local role becomes writable. */
export async function requestVaultOwnership(
  platform: VaultOwnershipPlatform,
  dependencies: VaultOwnershipRequestDependencies = defaultDependencies,
): Promise<PairingStatus> {
  const initialStatus = await dependencies.readStatus();
  if (platform === "android" || initialStatus.coordinatorEndpoint !== null) {
    await dependencies.receiveFromDesktop();
  } else {
    await dependencies.requestFromCoordinator();
  }

  for (let attempt = 0; attempt < dependencies.attempts; attempt += 1) {
    const status = attempt === 0 ? initialStatus : await dependencies.readStatus();
    if (status.canWrite) return status;
    if (attempt + 1 < dependencies.attempts) await dependencies.wait();
  }
  throw new Error("coordinator request timed out");
}

/**
 * Copies the linked vault to this device for its first link. A device must hold a copy before it
 * can become the main device, and the main device does not change.
 *
 * @param receive - Receives the bootstrap copy from the coordinator.
 * @throws When another vault transfer is still running.
 */
export async function requestFirstVaultCopy(
  receive: () => Promise<{ activated: boolean; inProgress: boolean }> = () =>
    receiveDesktopBundle("bootstrap"),
): Promise<void> {
  const outcome = await receive();
  if (outcome.inProgress) throw new Error("finish or retry the current vault transfer first");
}
