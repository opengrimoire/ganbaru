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
        isCoordinator: false,
        kind: "phone",
      }],
      peerDeviceId: "phone-1",
      peerLabel: "Phone",
      coordinatorEndpoint: null,
      vaultId: "vault-1",
      revokedByCoordinator: false,
      replicaReady: true,
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
      revokedByCoordinator: false,
      replicaReady: false,
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
    revokedByCoordinator: false,
    replicaReady: false,
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

describe("desktop network access", () => {
  it("reports a network that still needs the user's confirmation", async () => {
    invokeMock.mockResolvedValue({ state: "networkConfirmationRequired" });
    const { grantDesktopNetworkAccess } = await import("./vault-handoff");

    await expect(grantDesktopNetworkAccess()).resolves.toEqual({
      state: "networkConfirmationRequired",
    });
    expect(invokeMock).toHaveBeenCalledWith("handoff_grant_network_access");
  });

  it("refuses an unknown access state", async () => {
    invokeMock.mockResolvedValue({ state: "allowedEverywhere" });
    const { grantDesktopNetworkAccess } = await import("./vault-handoff");

    await expect(grantDesktopNetworkAccess()).rejects.toThrow(
      "Invalid desktop network access response",
    );
  });
});

describe("vault link status", () => {
  it("reports a linked device that still needs its first copy", async () => {
    invokeMock.mockResolvedValue({ linked: true, coordinatorDeviceId: "desktop-1", bootstrap: "required" });
    const { vaultLinkStatus } = await import("./vault-handoff");

    await expect(vaultLinkStatus()).resolves.toEqual({
      linked: true,
      coordinatorDeviceId: "desktop-1",
      bootstrap: "required",
    });
    expect(invokeMock).toHaveBeenCalledWith("vault_link_status");
  });

  it("reports an unlinked device without a coordinator", async () => {
    invokeMock.mockResolvedValue({ linked: false, coordinatorDeviceId: null, bootstrap: "none" });
    const { vaultLinkStatus } = await import("./vault-handoff");

    await expect(vaultLinkStatus()).resolves.toEqual({
      linked: false,
      coordinatorDeviceId: null,
      bootstrap: "none",
    });
  });

  it("refuses an unknown bootstrap state", async () => {
    invokeMock.mockResolvedValue({ linked: true, coordinatorDeviceId: "desktop-1", bootstrap: "owner" });
    const { vaultLinkStatus } = await import("./vault-handoff");

    await expect(vaultLinkStatus()).rejects.toThrow("Invalid vault link status response");
  });

  it("refuses a linked status without a coordinator", async () => {
    invokeMock.mockResolvedValue({ linked: true, coordinatorDeviceId: null, bootstrap: "none" });
    const { vaultLinkStatus } = await import("./vault-handoff");

    await expect(vaultLinkStatus()).rejects.toThrow("Invalid vault link status response");
  });

  it("refuses a coordinator on an unlinked status", async () => {
    invokeMock.mockResolvedValue({ linked: false, coordinatorDeviceId: "desktop-1", bootstrap: "none" });
    const { vaultLinkStatus } = await import("./vault-handoff");

    await expect(vaultLinkStatus()).rejects.toThrow("Invalid vault link status response");
  });
});

describe("vault bootstrap", () => {
  it("copies the linked vault and reports the activated copy", async () => {
    invokeMock.mockResolvedValue({ transferId: "transfer-1", activated: true, inProgress: false });
    const { requestVaultBootstrap } = await import("./vault-handoff");

    await expect(requestVaultBootstrap()).resolves.toEqual({
      transferId: "transfer-1",
      activated: true,
      inProgress: false,
    });
    expect(invokeMock).toHaveBeenCalledWith("handoff_request_bootstrap");
  });

  it("refuses an outcome without the in-progress flag", async () => {
    invokeMock.mockResolvedValue({ transferId: null, activated: false });
    const { requestVaultBootstrap } = await import("./vault-handoff");

    await expect(requestVaultBootstrap()).rejects.toThrow("Invalid vault bootstrap response");
  });
});

describe("local copy recovery", () => {
  it("keeps this device's copy without a response value", async () => {
    invokeMock.mockResolvedValue(null);
    const { recoverLocalVaultCopy } = await import("./vault-handoff");

    await expect(recoverLocalVaultCopy()).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith("handoff_recover_local_copy");
  });
});
