import { afterEach, describe, expect, it, vi } from "vitest";
import { emitWindowSync, listenWindowSync } from "$lib/windows/sync-transport";
import { ownWindowSyncSourceId } from "$lib/windows/sync";
import { setActiveVaultIdentity } from "$lib/vault/active-vault";
import { listenForNotesDatabaseChanges, publishNotesDatabaseChange } from "./window-sync";
import { notesDatabaseSession } from "./session.svelte";
import type { WindowSyncTransportListener } from "$lib/windows/sync-transport-contracts";

vi.mock("$lib/windows/sync-transport", () => ({ emitWindowSync: vi.fn(async () => {}), listenWindowSync: vi.fn() }));

afterEach(() => {
  setActiveVaultIdentity(null);
  vi.clearAllMocks();
});

describe("Notes database window invalidation", () => {
  it("invalidates locally and publishes only the active vault identity", () => {
    setActiveVaultIdentity("vault");
    const before = notesDatabaseSession.revision;
    publishNotesDatabaseChange();
    expect(notesDatabaseSession.revision).toBe(before + 1);
    expect(emitWindowSync).toHaveBeenCalledWith("notes-database-changed", {
      sourceId: ownWindowSyncSourceId(), payload: { vaultId: "vault" },
    });
  });

  it("accepts foreign changes in the same vault and ignores echoes, malformed events, and other vaults", async () => {
    let receive: WindowSyncTransportListener<unknown> = () => {};
    const unlisten = vi.fn();
    vi.mocked(listenWindowSync).mockImplementation(async (_name, listener) => {
      receive = listener;
      return unlisten;
    });
    setActiveVaultIdentity("vault");
    expect(await listenForNotesDatabaseChanges()).toBe(unlisten);
    const before = notesDatabaseSession.revision;
    for (const payload of [
      { sourceId: ownWindowSyncSourceId(), payload: { vaultId: "vault" } },
      { sourceId: "other", payload: { vaultId: "different-vault" } },
      { sourceId: "other", payload: { vaultId: 42 } }, null,
    ]) receive({ payload });
    expect(notesDatabaseSession.revision).toBe(before);
    receive({ payload: { sourceId: "other", payload: { vaultId: "vault" } } });
    expect(notesDatabaseSession.revision).toBe(before + 1);
  });
});
