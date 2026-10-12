import { describe, expect, it } from "vitest";
import { OFF_SYNC_STATUS, type SyncStatusView } from "$lib/api/sync";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import {
  canRequestSync,
  formatSyncTime,
  syncConflictTotal,
  syncDeviceName,
  syncHeldLines,
  syncLastExchangeLabel,
  syncRoleLabel,
  syncStateLabel,
} from "./status";

const t: Translate = ((key, ...args) => translateFromPartialCatalog({}, key, ...args)) as Translate;
const NOW = Date.UTC(2026, 9, 9, 12, 0);

function status(overrides: Partial<SyncStatusView>): SyncStatusView {
  return { ...OFF_SYNC_STATUS, held: { ...OFF_SYNC_STATUS.held }, ...overrides };
}

describe("sync status presentation", () => {
  it("uses relative time within the last hour and absolute time after", () => {
    expect(formatSyncTime(t, "en", NOW - 5 * 60_000, NOW)).toBe("5 min ago");
    expect(formatSyncTime(t, "en", NOW, NOW)).toBe("Now");
    expect(formatSyncTime(t, "en", NOW - 2 * 3_600_000, NOW)).not.toContain("min");
  });

  it("treats exchange times ahead of this clock as now", () => {
    expect(formatSyncTime(t, "en", NOW + 10 * 60_000, NOW)).toBe("Now");
  });

  it("labels a vault that never exchanged", () => {
    expect(syncLastExchangeLabel(t, "en", status({ state: "idle" }), NOW)).toBe("Not synced yet");
    expect(syncLastExchangeLabel(t, "en", status({ state: "idle", lastExchangeAtMs: NOW - 60_000 }), NOW))
      .toBe("Last synced 1 min ago");
  });

  it("lists only nonzero held reasons with plural wording", () => {
    expect(syncHeldLines(t, status({}))).toEqual([]);
    const lines = syncHeldLines(t, status({ held: { newerFormat: 1, newerManifest: 0, invalid: 2 } }));
    expect(lines).toEqual([
      "1 change needs a newer version of Ganbaru AI",
      "2 changes were rejected because they were invalid",
    ]);
  });

  it("totals conflicts across tables", () => {
    expect(syncConflictTotal(status({ conflicts: [{ table: "quick_notes", count: 2 }, { table: "quick_note_tags", count: 1 }] })))
      .toBe(3);
  });

  it("allows manual requests only while the service can exchange", () => {
    expect(canRequestSync(status({ state: "idle" }))).toBe(true);
    expect(canRequestSync(status({ state: "offline" }))).toBe(true);
    expect(canRequestSync(status({ state: "error" }))).toBe(true);
    expect(canRequestSync(status({ state: "off" }))).toBe(false);
    expect(canRequestSync(status({ state: "paused", paused: true }))).toBe(false);
    expect(canRequestSync(status({ state: "syncing" }))).toBe(false);
  });

  it("names own, labeled, and unknown devices", () => {
    expect(syncDeviceName({ deviceId: "a", deviceLabel: "Laptop", ownDevice: true }, "This device", "Another device"))
      .toBe("This device");
    expect(syncDeviceName({ deviceId: "b", deviceLabel: "Phone", ownDevice: false }, "This device", "Another device"))
      .toBe("Phone");
    expect(syncDeviceName({ deviceId: "c", deviceLabel: null, ownDevice: false }, "This device", "Another device"))
      .toBe("Another device");
  });
});

describe("sync state labels", () => {
  it("labels every state and role", () => {
    expect(syncStateLabel(t, "waiting_for_identity")).toBe("Waiting for your other device to confirm the link");
    expect(syncStateLabel(t, "idle")).toBe("Up to date");
    expect(syncRoleLabel(t, "hub")).toBe("Your other devices sync through this device");
  });
});
