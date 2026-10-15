import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { emitWindowSync, listenWindowSync } from "$lib/windows/sync-transport";
import { ownWindowSyncSourceId } from "$lib/windows/sync";
import { setActiveVaultIdentity } from "$lib/vault/active-vault";
import type { WindowSyncTransportListener } from "$lib/windows/sync-transport-contracts";
import { listenForNotesPageChanges, NOTES_PAGE_REFRESH_DELAY_MS, publishNotesPageChanges } from "./window-sync";

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
  return { event: "notes-page-changed", id: 0, payload } as Parameters<WindowSyncTransportListener<unknown>>[0];
}

describe("Notes page window sync", () => {
  it("publishes distinct page ids for the active vault and nothing without one", () => {
    publishNotesPageChanges(["page-a"]);
    expect(emitWindowSync).not.toHaveBeenCalled();
    setActiveVaultIdentity("vault");
    publishNotesPageChanges(["page-a", "page-a", "page-b"]);
    expect(emitWindowSync).toHaveBeenCalledWith("notes-page-changed", {
      sourceId: ownWindowSyncSourceId(), payload: { vaultId: "vault", pageIds: ["page-a", "page-b"] },
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
    const stop = await listenForNotesPageChanges(onChange);

    receive(event({ sourceId: ownWindowSyncSourceId(), payload: { vaultId: "vault", pageIds: ["own"] } }));
    receive(event({ sourceId: "other", payload: { vaultId: "different", pageIds: ["elsewhere"] } }));
    receive(event({ sourceId: "other", payload: { vaultId: "vault", pageIds: [42] } }));
    receive(event({ sourceId: "other", payload: { vaultId: "vault", pageIds: ["page-a"] } }));
    vi.advanceTimersByTime(NOTES_PAGE_REFRESH_DELAY_MS - 1);
    receive(event({ sourceId: "other", payload: { vaultId: "vault", pageIds: ["page-b"] } }));
    vi.advanceTimersByTime(NOTES_PAGE_REFRESH_DELAY_MS - 1);
    expect(onChange).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onChange).toHaveBeenCalledOnce();
    expect([...onChange.mock.calls[0][0]]).toEqual(["page-a", "page-b"]);

    receive(event({ sourceId: "other", payload: { vaultId: "vault", pageIds: ["page-c"] } }));
    stop();
    vi.advanceTimersByTime(NOTES_PAGE_REFRESH_DELAY_MS);
    expect(onChange).toHaveBeenCalledOnce();
    expect(unlisten).toHaveBeenCalledOnce();
  });
});
