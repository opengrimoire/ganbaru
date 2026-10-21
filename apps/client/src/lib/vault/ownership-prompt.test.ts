import { describe, expect, it, vi } from "vitest";
import type { PairingStatus } from "$lib/api/vault-handoff";
import {
  beginVaultOwnershipTransition,
  cancelVaultOwnershipTransition,
  requestVaultOwnership,
  shouldPresentVaultOwnershipPrompt,
  VAULT_OWNERSHIP_TRANSITION_STORAGE_KEY,
  type VaultOwnershipRequestDependencies,
} from "./ownership-prompt";

function memoryStorage(): Storage {
  const values = new Map<string, string>();
  return {
    get length() {
      return values.size;
    },
    clear: () => values.clear(),
    getItem: (key) => values.get(key) ?? null,
    key: (index) => [...values.keys()][index] ?? null,
    removeItem: (key) => { values.delete(key); },
    setItem: (key, value) => { values.set(key, value); },
  };
}

function status(
  canWrite: boolean | null,
  linked = true,
  coordinatorClient = true,
): PairingStatus {
  return {
    deviceId: "local",
    linked,
    devices: linked
      ? [{ deviceId: "peer", label: "Phone", isOwner: canWrite === false, isCoordinator: true, kind: "computer" }]
      : [],
    peerDeviceId: linked ? "peer" : null,
    peerLabel: linked ? "Phone" : null,
    coordinatorEndpoint: linked && coordinatorClient ? "192.168.1.2:43821" : null,
    vaultId: "vault",
    canWrite,
    recoveryRequired: false,
    revokedByCoordinator: false,
    replicaReady: true,
    pendingTransfer: false,
    canInvite: false,
    untouchedStarterVault: false,
  };
}

function dependencies(statuses: PairingStatus[]): VaultOwnershipRequestDependencies {
  let index = 0;
  return {
    readStatus: vi.fn(async () => statuses[Math.min(index++, statuses.length - 1)]!),
    receiveFromDesktop: vi.fn(async () => ({ activated: true, inProgress: false })),
    requestFromCoordinator: vi.fn(async () => undefined),
    wait: vi.fn(async () => undefined),
    attempts: statuses.length,
  };
}

describe("vault ownership prompt", () => {
  it("appears only for a linked read-only copy", () => {
    expect(shouldPresentVaultOwnershipPrompt(status(false))).toBe(true);
    expect(shouldPresentVaultOwnershipPrompt(status(true))).toBe(false);
    expect(shouldPresentVaultOwnershipPrompt(status(null, false))).toBe(false);
    expect(shouldPresentVaultOwnershipPrompt(null)).toBe(false);
  });

  it("preserves and clears the transition copy used during reload", () => {
    const storage = memoryStorage();
    const snapshot = {
      title: "Phone is the main device",
      description: "Only the main device can make changes.",
      actionLabel: "Switching to this device...",
      secondaryLabel: "Continue in read-only (Esc)",
    };

    beginVaultOwnershipTransition(snapshot, storage);
    expect(JSON.parse(storage.getItem(VAULT_OWNERSHIP_TRANSITION_STORAGE_KEY)!))
      .toEqual(snapshot);
    cancelVaultOwnershipTransition(storage);
    expect(storage.getItem(VAULT_OWNERSHIP_TRANSITION_STORAGE_KEY)).toBeNull();
  });

  it("asks Android to receive the desktop vault", async () => {
    const deps = dependencies([status(true)]);

    await expect(requestVaultOwnership("android", deps)).resolves.toEqual(status(true));
    expect(deps.receiveFromDesktop).toHaveBeenCalledOnce();
    expect(deps.requestFromCoordinator).not.toHaveBeenCalled();
  });

  it("keeps polling after a desktop request until ownership changes", async () => {
    const deps = dependencies([
      status(false, true, false),
      status(false, true, false),
      status(true, true, false),
    ]);

    await expect(requestVaultOwnership("desktop", deps)).resolves.toEqual(
      status(true, true, false),
    );
    expect(deps.requestFromCoordinator).toHaveBeenCalledOnce();
    expect(deps.receiveFromDesktop).not.toHaveBeenCalled();
    expect(deps.wait).toHaveBeenCalledTimes(2);
  });

  it("lets a secondary desktop receive through its coordinator", async () => {
    const deps = dependencies([status(false), status(true)]);

    await expect(requestVaultOwnership("desktop", deps)).resolves.toEqual(status(true));
    expect(deps.receiveFromDesktop).toHaveBeenCalledOnce();
    expect(deps.requestFromCoordinator).not.toHaveBeenCalled();
  });

  it("stops with a bounded timeout when the owner remains unavailable", async () => {
    const deps = dependencies([status(false, true, false), status(false, true, false)]);

    await expect(requestVaultOwnership("desktop", deps)).rejects.toThrow(
      "coordinator request timed out",
    );
  });
});
