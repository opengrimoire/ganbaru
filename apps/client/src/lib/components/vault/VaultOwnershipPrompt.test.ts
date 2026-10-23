// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PairingStatus } from "$lib/api/vault-handoff";
import { requestVaultOwnershipPrompt } from "$lib/vault/owner-required";
import VaultOwnershipPrompt from "./VaultOwnershipPrompt.svelte";

const backend = vi.hoisted(() => ({
  status: null as PairingStatus | null,
  readPairingStatus: vi.fn<() => Promise<PairingStatus>>(),
  receiveDesktopBundle: vi.fn(async () => ({
    transferId: "transfer",
    generation: 2,
    activated: true,
    inProgress: false,
  })),
}));

vi.mock("$lib/api/vault-handoff", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/vault-handoff")>();
  return {
    ...actual,
    readPairingStatus: backend.readPairingStatus,
    receiveDesktopBundle: backend.receiveDesktopBundle,
    requestOwnerBundle: vi.fn(async () => undefined),
  };
});

function pairingStatus(canWrite: boolean): PairingStatus {
  return {
    deviceId: "desktop",
    linked: true,
    devices: [{ deviceId: "phone", label: "Phone", isOwner: !canWrite, isCoordinator: false, kind: "phone" }],
    peerDeviceId: "phone",
    peerLabel: "Phone",
    coordinatorEndpoint: "192.168.1.2:43821",
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

describe("VaultOwnershipPrompt", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    backend.status = pairingStatus(false);
    backend.readPairingStatus.mockImplementation(async () => backend.status!);
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.clearAllMocks();
  });

  it("identifies the main device and allows this session to continue read-only", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(VaultOwnershipPrompt, {
      target,
      props: { platform: "desktop" },
    });

    await vi.waitFor(() => {
      expect(target?.querySelector("[data-vault-ownership-prompt]")).not.toBeNull();
    });
    expect(target.textContent).toContain("Phone is the main device");
    expect(target.textContent).toContain("Switch to this device (Enter)");
    expect(target.textContent).toContain("Continue in read-only (Esc)");

    const continueButton = [...target.querySelectorAll("button")]
      .find((button) => button.textContent?.trim() === "Continue in read-only (Esc)");
    expect(continueButton).toBeDefined();
    continueButton?.click();
    await tick();

    expect(target.querySelector("[data-vault-ownership-prompt]")).toBeNull();
  });

  it("stays out of the way while this device owns the vault", async () => {
    backend.status = pairingStatus(true);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(VaultOwnershipPrompt, {
      target,
      props: { platform: "desktop" },
    });
    await vi.waitFor(() => expect(backend.readPairingStatus).toHaveBeenCalled());

    expect(target.querySelector("[data-vault-ownership-prompt]")).toBeNull();
  });

  it("waits in a detached window until a refused change there asks for it", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(VaultOwnershipPrompt, {
      target,
      props: { platform: "desktop", presentOnLoad: false },
    });
    await vi.waitFor(() => expect(backend.readPairingStatus).toHaveBeenCalled());
    await tick();
    expect(target.querySelector("[data-vault-ownership-prompt]")).toBeNull();

    requestVaultOwnershipPrompt();

    await vi.waitFor(() => {
      expect(target?.querySelector("[data-vault-ownership-prompt]")).not.toBeNull();
    });
    expect(target.textContent).toContain("Phone is the main device");
  });

  it("requires confirmation before the first linked vault replaces local data", async () => {
    backend.status = { ...pairingStatus(false), replicaReady: false };
    target = document.createElement("div");
    document.body.append(target);
    component = mount(VaultOwnershipPrompt, {
      target,
      props: { platform: "android" },
    });
    await vi.waitFor(() => {
      expect(target?.querySelector("[data-vault-ownership-prompt]")).not.toBeNull();
    });

    const switchButton = [...target.querySelectorAll("button")]
      .find((button) => button.textContent?.trim() === "Switch to this device");
    switchButton?.click();
    await tick();

    expect(target.textContent).toContain("Replace this device's current data?");
    expect(target.textContent).toContain("cannot be merged");
    expect(target.textContent).toContain("backup to Downloads");
    expect(backend.receiveDesktopBundle).not.toHaveBeenCalled();
  });

  it("reports its initial status decision even when the status read fails", async () => {
    const onStatusReady = vi.fn();
    backend.readPairingStatus.mockRejectedValueOnce(new Error("unavailable"));
    target = document.createElement("div");
    document.body.append(target);
    component = mount(VaultOwnershipPrompt, {
      target,
      props: {
        platform: "android",
        onStatusReady,
      },
    });

    await vi.waitFor(() => expect(onStatusReady).toHaveBeenCalledOnce());
    expect(target.querySelector("[data-vault-ownership-prompt]")).toBeNull();
  });

  it("routes Ctrl+Shift+W to the shell close confirmation", async () => {
    const requestClose = vi.fn(async () => undefined);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(VaultOwnershipPrompt, {
      target,
      props: {
        platform: "desktop",
        onRequestClose: requestClose,
      },
    });
    await vi.waitFor(() => {
      expect(target?.querySelector("[data-vault-ownership-prompt]")).not.toBeNull();
    });

    window.dispatchEvent(new KeyboardEvent("keydown", {
      key: "w",
      ctrlKey: true,
      shiftKey: true,
      cancelable: true,
    }));

    await vi.waitFor(() => expect(requestClose).toHaveBeenCalledOnce());
  });
});
