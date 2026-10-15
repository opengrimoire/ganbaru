import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { emitWindowSync, listenWindowSync } from "$lib/windows/sync-transport";
import { ownWindowSyncSourceId } from "$lib/windows/sync";
import { setActiveVaultIdentity } from "$lib/vault/active-vault";
import type { WindowSyncTransportListener } from "$lib/windows/sync-transport-contracts";
import { createVaultChangeChannel, VAULT_CHANGE_QUIET_MS } from "./vault-change-channel";

const channel = createVaultChangeChannel("records-changed");

vi.mock("$lib/windows/sync-transport", () => ({ emitWindowSync: vi.fn(async () => {}), listenWindowSync: vi.fn() }));

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  setActiveVaultIdentity(null);
  vi.clearAllMocks();
});

function event(payload: unknown): Parameters<WindowSyncTransportListener<unknown>>[0] {
  return { event: "records-changed", id: 0, payload } as Parameters<WindowSyncTransportListener<unknown>>[0];
}

describe("Vault change channel", () => {
  it("publishes distinct keys for the active vault and nothing without one", () => {
    channel.publish(["page-a"]);
    expect(emitWindowSync).not.toHaveBeenCalled();
    setActiveVaultIdentity("vault");
    channel.publish(["page-a", "page-a", "page-b"]);
    expect(emitWindowSync).toHaveBeenCalledWith("records-changed", {
      sourceId: ownWindowSyncSourceId(), payload: { vaultId: "vault", keys: ["page-a", "page-b"] },
    });
  });

  it("gathers a burst of foreign changes into one delivery and ignores echoes and other vaults", async () => {
    let receive: WindowSyncTransportListener<unknown> = () => {};
    const unlisten = vi.fn();
    vi.mocked(listenWindowSync).mockImplementation(async (_name, listener) => {
      receive = listener;
      return unlisten;
    });
    setActiveVaultIdentity("vault");
    const onChange = vi.fn();
    const stop = await channel.listen(onChange);

    receive(event({ sourceId: ownWindowSyncSourceId(), payload: { vaultId: "vault", keys: ["own"] } }));
    receive(event({ sourceId: "other", payload: { vaultId: "different", keys: ["elsewhere"] } }));
    receive(event({ sourceId: "other", payload: { vaultId: "vault", keys: [42] } }));
    receive(event({ sourceId: "other", payload: { vaultId: "vault", keys: ["page-a"] } }));
    vi.advanceTimersByTime(VAULT_CHANGE_QUIET_MS - 1);
    receive(event({ sourceId: "other", payload: { vaultId: "vault", keys: ["page-b"] } }));
    vi.advanceTimersByTime(VAULT_CHANGE_QUIET_MS - 1);
    expect(onChange).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onChange).toHaveBeenCalledOnce();
    expect([...onChange.mock.calls[0][0]]).toEqual(["page-a", "page-b"]);

    receive(event({ sourceId: "other", payload: { vaultId: "vault", keys: ["page-c"] } }));
    stop();
    vi.advanceTimersByTime(VAULT_CHANGE_QUIET_MS);
    expect(onChange).toHaveBeenCalledOnce();
    expect(unlisten).toHaveBeenCalledOnce();
  });
});
