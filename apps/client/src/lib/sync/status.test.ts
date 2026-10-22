import { describe, expect, it } from "vitest";
import { OFF_SYNC_STATUS, type SyncStatusView } from "$lib/api/sync";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import {
  canRequestSync,
  syncClockAheadLines,
  formatSyncTime,
  syncConflictLines,
  syncDescription,
  syncDeviceName,
  syncHeldLines,
  syncLastExchangeLabel,
  syncRecoveryTotal,
  syncRoleLabel,
  syncStateLabel,
} from "./status";
import { SyncPresentationRegistry } from "./presentation-registry";

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
      "1 change needs a newer version of Ganbaru",
      "2 changes were rejected because they were invalid",
    ]);
  });

  it("describes conflicts per table in the words of the owning domain", () => {
    const presentations = new SyncPresentationRegistry().register({
      label: () => "Notes",
      tables: { notes: { conflicts: (_t, count) => `${count} notes in conflict` }, note_tags: {} },
      changed: () => {},
    });
    const conflicts = [
      { table: "notes", count: 2 },
      { table: "note_tags", count: 0 },
      { table: "events", count: 1 },
      { table: "tasks", count: 1 },
    ];
    expect(syncConflictLines(t, presentations, status({ conflicts }))).toEqual([
      { table: "notes", line: "2 notes in conflict" },
      { table: "events", line: t("sync.conflicts", 1) },
      { table: "tasks", line: t("sync.conflicts", 1) },
    ]);
    expect(syncConflictLines(t, presentations, status({}))).toEqual([]);
  });

  it("lists the features of every registered domain in the description", () => {
    const presentations = new SyncPresentationRegistry()
      .register({ label: () => "Quick notes", tables: { quick_notes: {} }, changed: () => {} });
    expect(syncDescription(t, "en", presentations)).toBe(t("sync.description", "Quick notes"));
    presentations.register({ label: () => "Calendar", tables: { events: {} }, changed: () => {} });
    expect(syncDescription(t, "en", presentations)).toBe(t("sync.description", "Quick notes and Calendar"));
  });

  it("totals recovery offers across tables", () => {
    expect(syncRecoveryTotal(status({}))).toBe(0);
    expect(syncRecoveryTotal(status({ recovery: [{ table: "quick_notes", count: 2 }, { table: "projects", count: 1 }] })))
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

  it("names each device whose clock runs far ahead", () => {
    expect(syncClockAheadLines(t, status({}))).toEqual([]);
    const lines = syncClockAheadLines(t, status({
      clockAheadDevices: [
        { deviceId: "a", deviceLabel: "Laptop", ownDevice: false },
        { deviceId: "b", deviceLabel: null, ownDevice: false },
        { deviceId: "d", deviceLabel: null, ownDevice: false },
        { deviceId: "c", deviceLabel: "Phone", ownDevice: true },
      ],
    }));
    expect(lines).toHaveLength(3);
    expect(lines[0]).toContain("The clock on Laptop is ahead");
    expect(lines[1]).toContain("The clock on another device is ahead");
    expect(lines[2]).toContain("Some edits from this device");
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
