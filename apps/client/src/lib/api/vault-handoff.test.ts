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

describe("pending enrollment status", () => {
  const coordinatorStatus = {
    deviceId: "desktop-1",
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
    canInvite: true,
    untouchedStarterVault: false,
  };

  it("reports the waiting device with the code it shows", async () => {
    invokeMock.mockResolvedValue({
      ...coordinatorStatus,
      pendingEnrollment: {
        requestId: "enrollment-1",
        deviceLabel: "Pixel",
        deviceKind: "phone",
        code: "ABCD-EFGH-JKMN",
      },
    });
    const { readPairingStatus } = await import("./vault-handoff");

    const status = await readPairingStatus();

    expect(status.pendingEnrollment).toEqual({
      requestId: "enrollment-1",
      deviceLabel: "Pixel",
      deviceKind: "phone",
      code: "ABCD-EFGH-JKMN",
    });
  });

  it("distinguishes no waiting device from a platform that cannot host linking", async () => {
    invokeMock.mockResolvedValueOnce({ ...coordinatorStatus, pendingEnrollment: null });
    invokeMock.mockResolvedValueOnce(coordinatorStatus);
    const { readPairingStatus } = await import("./vault-handoff");

    expect((await readPairingStatus()).pendingEnrollment).toBeNull();
    expect((await readPairingStatus()).pendingEnrollment).toBeUndefined();
  });

  it("refuses a waiting device without a code", async () => {
    invokeMock.mockResolvedValue({
      ...coordinatorStatus,
      pendingEnrollment: {
        requestId: "enrollment-1",
        deviceLabel: "Pixel",
        deviceKind: "phone",
      },
    });
    const { getCachedPairingStatus, readPairingStatus } = await import("./vault-handoff");

    await expect(readPairingStatus()).rejects.toThrow("Invalid pending enrollment response");
    expect(getCachedPairingStatus()).toBeUndefined();
  });
});
