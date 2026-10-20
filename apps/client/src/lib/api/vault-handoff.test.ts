import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...(args as [string, unknown])),
}));

beforeEach(() => {
  vi.resetModules();
  invokeMock.mockReset();
});

describe("linked-device status cache", () => {
  it("exposes the last status validated by the native boundary", async () => {
    invokeMock.mockResolvedValue({
      deviceId: "desktop-1",
      linked: true,
      devices: [{
        deviceId: "phone-1",
        label: "Phone",
        isOwner: false,
        isCoordinator: false,
        kind: "phone",
      }],
      peerDeviceId: "phone-1",
      peerLabel: "Phone",
      coordinatorEndpoint: null,
      vaultId: "vault-1",
      canWrite: true,
      recoveryRequired: false,
      revokedByCoordinator: false,
      replicaReady: true,
      pendingTransfer: false,
      canInvite: true,
      untouchedStarterVault: false,
    });
    const { getCachedPairingStatus, readPairingStatus } = await import("./vault-handoff");

    expect(getCachedPairingStatus()).toBeUndefined();
    const status = await readPairingStatus();

    expect(getCachedPairingStatus()).toBe(status);
    expect(status.devices).toHaveLength(1);
  });

  it("refuses a status that does not state whether the starter vault is untouched", async () => {
    invokeMock.mockResolvedValue({
      deviceId: "phone-1",
      linked: false,
      devices: [],
      peerDeviceId: null,
      peerLabel: null,
      coordinatorEndpoint: null,
      vaultId: "vault-1",
      canWrite: true,
      recoveryRequired: false,
      revokedByCoordinator: false,
      replicaReady: false,
      pendingTransfer: false,
      canInvite: false,
    });
    const { getCachedPairingStatus, readPairingStatus } = await import("./vault-handoff");

    await expect(readPairingStatus()).rejects.toThrow("Invalid pairing status response");
    expect(getCachedPairingStatus()).toBeUndefined();
  });
});
