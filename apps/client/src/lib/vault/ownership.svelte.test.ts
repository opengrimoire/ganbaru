import { afterEach, describe, expect, it, vi } from "vitest";
import * as vaultState from "./state";
import type { VaultOwnershipStatus } from "./state";
import { getVaultOwnership, vaultOwnerChanged } from "./ownership.svelte";

/** Builds the ownership status of `vaultId` as this device sees it while `ownerDeviceId` owns it. */
function status(ownerDeviceId: string, generation: number, vaultId = "vault"): VaultOwnershipStatus {
  const owns = ownerDeviceId === "device:this";
  return {
    vaultId, deviceId: "device:this", ownerDeviceId, generation,
    role: owns ? "owner" : "read-only", canWrite: owns, linkedReplica: !owns,
    replicaWritableDomains: [], transferPhase: { kind: "stable" },
  };
}

afterEach(() => {
  getVaultOwnership().clear();
  vi.restoreAllMocks();
});

describe("vault owner changes", () => {
  it("counts another owner device or generation of the same vault, but not a vault switch", () => {
    expect(vaultOwnerChanged(status("device:old", 1), status("device:this", 2))).toBe(true);
    expect(vaultOwnerChanged(status("device:this", 1), status("device:this", 2))).toBe(true);
    expect(vaultOwnerChanged(status("device:old", 1), status("device:old", 1))).toBe(false);
    expect(vaultOwnerChanged(status("device:old", 1), status("device:this", 2, "other"))).toBe(false);
  });

  it("tells listeners once per handoff found by a refresh, never on the first status", async () => {
    const read = vi.spyOn(vaultState, "getVaultOwnershipStatus");
    const ownership = getVaultOwnership();
    const listener = vi.fn();
    const unsubscribe = ownership.onOwnerChange(listener);

    read.mockResolvedValueOnce(status("device:old", 1));
    await ownership.refresh();
    expect(listener).not.toHaveBeenCalled();

    read.mockResolvedValueOnce(status("device:old", 1));
    await ownership.refresh();
    expect(listener).not.toHaveBeenCalled();

    const handedOver = status("device:this", 2);
    read.mockResolvedValueOnce(handedOver);
    await ownership.refresh();
    expect(listener).toHaveBeenCalledExactlyOnceWith(handedOver);

    unsubscribe();
    read.mockResolvedValueOnce(status("device:old", 3));
    await ownership.refresh();
    expect(listener).toHaveBeenCalledOnce();
  });
});
