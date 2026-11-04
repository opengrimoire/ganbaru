import { describe, expect, it } from "vitest";
import { OFF_SYNC_STATUS, SYNC_RELAY_PROBLEMS, type SyncStatusView } from "$lib/api/sync";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import {
  canRequestSync,
  syncClockAheadLines,
  syncConnectionLabel,
  syncDegradedActionLabel,
  syncDegradedRows,
  formatSyncTime,
  syncConflictLines,
  syncDescription,
  syncDeviceAcknowledgementLines,
  syncDeviceName,
  syncHeldLines,
  syncLastExchangeLabel,
  syncRecoveryTotal,
  syncDeliveryLines,
  syncRelayLines,
  syncRelayProblemLabel,
  syncRoleLabel,
  syncStateLabel,
  syncUncountedDeviceLines,
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

  it("explains each device the coordinator does not count", () => {
    expect(syncUncountedDeviceLines(t, status({}))).toEqual([]);
    const lines = syncUncountedDeviceLines(t, status({
      uncountedDevices: [
        { device: { deviceId: "a", deviceLabel: "Old phone", ownDevice: false }, standing: "dormant" },
        { device: { deviceId: "b", deviceLabel: null, ownDevice: false }, standing: "relinkRequired" },
      ],
    }));
    expect(lines.map((entry) => entry.deviceId)).toEqual(["a", "b"]);
    expect(lines[0].line).toContain("Sync no longer waits for Old phone");
    expect(lines[1].line).toBe("Link another device again so it can sync.");
  });

  it("reports each device's last acknowledgement and the ones with none yet", () => {
    expect(syncDeviceAcknowledgementLines(t, "en", status({}), NOW)).toEqual([]);
    const lines = syncDeviceAcknowledgementLines(t, "en", status({
      deviceAcknowledgements: [
        { device: { deviceId: "a", deviceLabel: "Laptop", ownDevice: false }, lastAcknowledgedAtMs: NOW - 5 * 60_000 },
        { device: { deviceId: "b", deviceLabel: null, ownDevice: false }, lastAcknowledgedAtMs: null },
      ],
    }), NOW);
    expect(lines).toEqual([
      { deviceId: "a", line: "Last confirmation from Laptop: 5 min ago" },
      { deviceId: "b", line: "No confirmation from another device yet" },
    ]);
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

describe("sync connectivity", () => {
  const lan = { network: "lan", address: "192.168.1.20:43821" } as const;

  it("labels the connection of a client only while it is linked", () => {
    expect(syncConnectionLabel(t, status({ state: "idle", role: "client", connection: lan })))
      .toBe("Connected over local network at 192.168.1.20:43821");
    expect(syncConnectionLabel(t, status({
      state: "syncing",
      role: "client",
      connection: { network: "overlay", address: "100.64.0.7:43821" },
    }))).toBe("Connected over overlay network at 100.64.0.7:43821");
    expect(syncConnectionLabel(t, status({ state: "offline", role: "client", connection: lan }))).toBeNull();
    expect(syncConnectionLabel(t, status({ state: "error", role: "client", connection: lan }))).toBeNull();
    expect(syncConnectionLabel(t, status({ state: "idle", role: "hub", connection: lan }))).toBeNull();
    expect(syncConnectionLabel(t, status({ state: "idle", role: "client" }))).toBeNull();
  });

  it("explains each degraded reason with its recovery action", () => {
    const rows = syncDegradedRows(t, status({
      state: "offline",
      role: "client",
      degraded: ["backgroundRestricted", "standbyRestricted", "localNetworkDenied", "firewallAuthorizationRequired"],
    }));
    expect(rows.map((row) => [row.reason, row.action])).toEqual([
      ["backgroundRestricted", "openBatterySettings"],
      ["standbyRestricted", "openBatterySettings"],
      ["localNetworkDenied", null],
      ["firewallAuthorizationRequired", null],
    ]);
    expect(rows[3]?.label).toContain("Your main device was not found");
    expect(rows.every((row) => !row.label.startsWith("sync."))).toBe(true);
  });

  it("offers the firewall action only on the coordinator", () => {
    expect(syncDegradedRows(t, status({ state: "idle", role: "hub", degraded: ["firewallAuthorizationRequired"] })))
      .toEqual([{
        reason: "firewallAuthorizationRequired",
        label: "The firewall keeps your other devices from reaching this device.",
        action: "allowNetworkAccess",
      }]);
    expect(syncDegradedRows(t, status({ state: "idle", role: "hub" }))).toEqual([]);
  });

  it("offers the peer firewall grant to a member desktop whose listener is blocked", () => {
    const rows = syncDegradedRows(t, status({
      state: "idle",
      role: "client",
      degraded: ["firewallAuthorizationRequired", "firewallPeerListenerBlocked"],
    }));
    expect(rows.map((row) => [row.reason, row.action])).toEqual([
      ["firewallAuthorizationRequired", null],
      ["firewallPeerListenerBlocked", "allowPeerConnections"],
    ]);
    expect(syncDegradedActionLabel(t, "allowPeerConnections")).toBe("Allow peers to connect");
    expect(syncDegradedActionLabel(t, "allowNetworkAccess")).toBe("Allow network access");
  });
});

describe("relay delivery counts", () => {
  const relay = {
    lastPassAtMs: NOW - 2 * 60_000,
    lastContactAtMs: null,
    sent: 0,
    received: 0,
    undelivered: 0,
    uploadedFiles: 0,
    downloadedFiles: 0,
    deferredFiles: 0,
    storedBytes: 0,
    quotaBytes: 0,
    problem: null,
  };

  it("shows nothing for a vault without a relay pass", () => {
    expect(syncRelayLines(t, "en", status({ state: "idle" }), NOW)).toEqual([]);
  });

  it("shows only the pass time after a settled pass", () => {
    expect(syncRelayLines(t, "en", status({ state: "idle", relay }), NOW)).toEqual(["Relay checked 2 min ago"]);
  });

  it("lists each nonzero count with plural wording", () => {
    const lines = syncRelayLines(t, "en", status({
      state: "idle",
      relay: { ...relay, sent: 1, received: 3, undelivered: 2, uploadedFiles: 1, downloadedFiles: 2, deferredFiles: 1 },
    }), NOW);
    expect(lines).toEqual([
      "Relay checked 2 min ago",
      "1 change sent through the relay",
      "3 changes received through the relay",
      "1 file uploaded to the relay",
      "2 files downloaded from the relay",
      "1 large file waits for a network without data limits",
    ]);
  });

  it("shows storage only when the relay reports a quota, then the localized problem and last contact", () => {
    const lines = syncRelayLines(t, "en", status({
      state: "error",
      relay: {
        ...relay,
        storedBytes: 1_500_000,
        quotaBytes: 100_000_000,
        problem: "busy",
        lastContactAtMs: NOW - 5 * 60_000,
      },
    }), NOW);
    expect(lines.slice(1)).toEqual([
      "Relay storage: 1.5 MB of 100 MB",
      "Relay: The relay is busy. Sync tries again later.",
      "Relay last answered 5 min ago",
    ]);
    expect(syncRelayLines(t, "en", status({ state: "idle", relay: { ...relay, storedBytes: 9 } }), NOW)).toHaveLength(1);
  });

  it("says when the relay has not answered at all", () => {
    const lines = syncRelayLines(t, "en", status({ state: "error", relay: { ...relay, problem: "unreachable" } }), NOW);
    expect(lines.slice(1)).toEqual([
      "Relay: The relay could not be reached, or its certificate did not match.",
      "The relay has not answered since the app started",
    ]);
  });

  it("localizes every relay problem code", () => {
    for (const problem of SYNC_RELAY_PROBLEMS) {
      const label = syncRelayProblemLabel(t, problem);
      expect(label).not.toContain("sync.server.problems");
      expect(label.length).toBeGreaterThan(0);
    }
  });
});

describe("relay delivery stages", () => {
  const relay = {
    lastPassAtMs: NOW,
    lastContactAtMs: NOW,
    sent: 0,
    received: 0,
    undelivered: 0,
    uploadedFiles: 0,
    downloadedFiles: 0,
    deferredFiles: 0,
    storedBytes: 0,
    quotaBytes: 0,
    problem: null,
  };

  it("shows nothing without a relay pass", () => {
    expect(syncDeliveryLines(t, status({ state: "idle", unconfirmed: 3 }))).toEqual([]);
  });

  it("splits unconfirmed changes into saved here and received by the relay", () => {
    expect(syncDeliveryLines(t, status({ state: "idle", unconfirmed: 5, relay: { ...relay, undelivered: 2 } }))).toEqual([
      "2 changes are saved on this device and not on the relay yet",
      "3 changes were received by the relay and wait for the anchor to confirm them",
    ]);
    expect(syncDeliveryLines(t, status({ state: "idle", unconfirmed: 1, relay }))).toEqual([
      "1 change was received by the relay and waits for the anchor to confirm it",
    ]);
  });

  it("never reports a negative relay count when undelivered changes are not counted as unconfirmed yet", () => {
    expect(syncDeliveryLines(t, status({ state: "idle", unconfirmed: 0, relay: { ...relay, undelivered: 1 } }))).toEqual([
      "1 change is saved on this device and not on the relay yet",
    ]);
  });

  it("never credits the relay after a pass that ended before it reported what it holds", () => {
    const failed = { ...relay, undelivered: null, problem: "unreachable" as const };
    expect(syncDeliveryLines(t, status({ state: "error", unconfirmed: 3, relay: failed }))).toEqual([
      "3 changes are saved on this device and not on the relay yet",
    ]);
    expect(syncDeliveryLines(t, status({ state: "error", unconfirmed: 0, pendingLocalChanges: 1, relay: failed }))).toEqual([]);
  });

  it("reports everything confirmed only when nothing waits locally or at the anchor", () => {
    expect(syncDeliveryLines(t, status({ state: "idle", relay }))).toEqual(["Every change from this device is confirmed"]);
    expect(syncDeliveryLines(t, status({ state: "idle", pendingLocalChanges: 1, relay }))).toEqual([]);
  });
});
