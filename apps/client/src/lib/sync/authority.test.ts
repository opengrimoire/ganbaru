import { describe, expect, it } from "vitest";
import {
  OFF_SYNC_STATUS,
  SYNC_PROPOSAL_REJECTIONS,
  type SyncAuthorityDevice,
  type SyncAuthorityView,
  type SyncLateChange,
  type SyncStatusView,
} from "$lib/api/sync";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import {
  canResetSyncState,
  canResolveSyncLateChanges,
  canTakeSyncAnchor,
  canForceSyncAnchor,
  SYNC_HANDOVER_SUGGESTION_MS,
  type SyncAuthorityContext,
  syncDeviceActions,
  syncDeviceBadges,
  syncKitCodeLines,
  syncKitFileName,
  syncKitFileText,
  syncLateChangeKey,
  syncLateChangeOrigin,
  syncOutboxChangeLabel,
  syncAnchorUnreachableLine,
  syncOutboxStateLine,
  syncQuarantinableAnchor,
  syncRecoveryKitUse,
  syncRejectionLabel,
  syncUnconfirmedNotice,
} from "./authority";

const t: Translate = ((key, ...args) => translateFromPartialCatalog({}, key, ...args)) as Translate;
const NOW = Date.UTC(2026, 9, 25, 12, 0);
const CODE = "0123-4567-89AB-CDEF-GHJK-MNPQ-RSTV-WXYZ-0123-4567-89AB-CDEF-GHJK-MNPQ";

function device(id: string, overrides: Partial<SyncAuthorityDevice> = {}): SyncAuthorityDevice {
  return {
    device: { deviceId: id, deviceLabel: id, ownDevice: false },
    kind: "desktop",
    role: "member",
    enrolledRevision: 1,
    removedRevision: null,
    quarantined: false,
    anchor: false,
    ...overrides,
  };
}

const own = device("own", { device: { deviceId: "own", deviceLabel: "Desk", ownDevice: true }, role: "admin" });
const anchor = device("anchor", { role: "admin", anchor: true });
const memberDesktop = device("laptop");
const phone = device("phone", { kind: "mobile" });

function context(overrides: Partial<SyncAuthorityContext> = {}, authority: Partial<SyncAuthorityView> = {}): SyncAuthorityContext {
  return {
    authority: {
      anchorDeviceId: "anchor",
      epoch: 1,
      revision: 5,
      frozen: false,
      ownIsAdmin: true,
      devices: [own, anchor, memberDesktop, phone],
      ...authority,
    },
    statusRole: "client",
    mobileLayout: false,
    anchorUnreachableSinceMs: null,
    ...overrides,
  };
}

function status(overrides: Partial<SyncStatusView>): SyncStatusView {
  return { ...OFF_SYNC_STATUS, held: { ...OFF_SYNC_STATUS.held }, ...overrides };
}

describe("sync device badges", () => {
  it("shows the role, then anchor and quarantine, and replaces the role once removed", () => {
    expect(syncDeviceBadges(anchor)).toEqual(["admin", "anchor"]);
    expect(syncDeviceBadges(device("x", { quarantined: true }))).toEqual(["member", "quarantined"]);
    expect(syncDeviceBadges(device("x", { removedRevision: 4, quarantined: true }))).toEqual(["removed", "quarantined"]);
  });
});

describe("sync device actions", () => {
  it("offers removal on a linked admin desktop but never for itself, the anchor, or a removed device", () => {
    expect(syncDeviceActions(context(), memberDesktop).remove).toBe(true);
    expect(syncDeviceActions(context(), phone).remove).toBe(true);
    expect(syncDeviceActions(context(), own).remove).toBe(false);
    expect(syncDeviceActions(context(), anchor).remove).toBe(false);
    expect(syncDeviceActions(context(), device("gone", { removedRevision: 3 })).remove).toBe(false);
  });

  it("hides admin proposals on members, on Android, on the coordinator for removal, and while frozen", () => {
    expect(syncDeviceActions(context({}, { ownIsAdmin: false }), memberDesktop).remove).toBe(false);
    expect(syncDeviceActions(context({ mobileLayout: true }), memberDesktop).remove).toBe(false);
    expect(syncDeviceActions(context({ statusRole: "hub" }), memberDesktop).remove).toBe(false);
    expect(syncDeviceActions(context({}, { frozen: true }), memberDesktop).remove).toBe(false);
  });

  it("promotes only member desktops, only on the coordinator", () => {
    const hub = context({ statusRole: "hub" });
    expect(syncDeviceActions(hub, memberDesktop).promote).toBe(true);
    expect(syncDeviceActions(hub, phone).promote).toBe(false);
    expect(syncDeviceActions(hub, anchor).promote).toBe(false);
    expect(syncDeviceActions(hub, device("q", { quarantined: true })).promote).toBe(false);
    expect(syncDeviceActions(context(), memberDesktop).promote).toBe(false);
    expect(syncDeviceActions(context({ statusRole: "hub", mobileLayout: true }), memberDesktop).promote).toBe(false);
  });

  it("lets any device quarantine other live devices except the anchor, and lift it later", () => {
    const member = context({ mobileLayout: true }, { ownIsAdmin: false });
    expect(syncDeviceActions(member, memberDesktop)).toMatchObject({ quarantine: true, lift: false });
    expect(syncDeviceActions(member, anchor).quarantine).toBe(false);
    expect(syncDeviceActions(member, own).quarantine).toBe(false);
    const quarantined = device("q", { quarantined: true, removedRevision: 2 });
    expect(syncDeviceActions(member, quarantined)).toMatchObject({ quarantine: false, lift: true });
  });
});

describe("sync authority controls", () => {
  it("offers a planned handover only to a linked admin desktop that is not the anchor", () => {
    expect(canTakeSyncAnchor(context())).toBe(true);
    expect(canTakeSyncAnchor(context({ statusRole: "hub" }))).toBe(false);
    expect(canTakeSyncAnchor(context({ mobileLayout: true }))).toBe(false);
    expect(canTakeSyncAnchor(context({}, { ownIsAdmin: false }))).toBe(false);
    expect(canTakeSyncAnchor(context({}, { frozen: true }))).toBe(false);
    expect(canTakeSyncAnchor(context({}, { devices: [{ ...own, anchor: true }] }))).toBe(false);
    expect(canTakeSyncAnchor(context({}, { devices: [anchor] }))).toBe(false);
  });

  it("offers a forced handover only to a linked admin desktop whose anchor stopped answering", () => {
    const lost = { anchorUnreachableSinceMs: NOW - 60_000 };
    expect(canForceSyncAnchor(context(lost))).toBe(true);
    expect(canForceSyncAnchor(context(lost, { frozen: true }))).toBe(true);
    expect(canForceSyncAnchor(context())).toBe(false);
    expect(canForceSyncAnchor(context({ ...lost, statusRole: "hub" }))).toBe(false);
    expect(canForceSyncAnchor(context({ ...lost, mobileLayout: true }))).toBe(false);
    expect(canForceSyncAnchor(context(lost, { ownIsAdmin: false }))).toBe(false);
    expect(canForceSyncAnchor(context(lost, { devices: [{ ...own, anchor: true }] }))).toBe(false);
    expect(canForceSyncAnchor(context(lost, { devices: [{ ...own, removedRevision: 3 }, anchor] }))).toBe(false);
  });

  it("offers the anchor for quarantine only while it is another live device not quarantined yet", () => {
    expect(syncQuarantinableAnchor(context().authority)).toBe(anchor);
    expect(syncQuarantinableAnchor(context({}, { devices: [own, { ...anchor, quarantined: true }] }).authority)).toBeNull();
    expect(syncQuarantinableAnchor(context({}, { devices: [{ ...own, anchor: true }] }).authority)).toBeNull();
    expect(syncQuarantinableAnchor(context({}, { devices: [own] }).authority)).toBeNull();
  });

  it("proposes kit use while the anchor answers and forces the handover once it stopped", () => {
    const member = context({}, { ownIsAdmin: false, devices: [{ ...own, role: "member" }, anchor] });
    expect(syncRecoveryKitUse(member, "idle")).toBe("propose");
    expect(syncRecoveryKitUse(member, "syncing")).toBe("propose");
    expect(syncRecoveryKitUse(member, "offline")).toBeNull();
    expect(syncRecoveryKitUse({ ...member, authority: { ...member.authority, frozen: true } }, "idle")).toBeNull();
    expect(syncRecoveryKitUse(context(), "idle")).toBeNull();
    expect(syncRecoveryKitUse({ ...member, mobileLayout: true }, "idle")).toBeNull();
    expect(syncRecoveryKitUse(context({}, { ownIsAdmin: false, devices: [{ ...own, kind: "mobile" }] }), "idle")).toBeNull();

    const lost = { ...member, anchorUnreachableSinceMs: NOW - 60_000 };
    expect(syncRecoveryKitUse(lost, "offline")).toBe("force");
    expect(syncRecoveryKitUse({ ...lost, authority: { ...lost.authority, frozen: true } }, "offline")).toBe("force");
    expect(syncRecoveryKitUse({ ...lost, statusRole: "hub" }, "offline")).toBeNull();
    expect(syncRecoveryKitUse({ ...lost, mobileLayout: true }, "offline")).toBeNull();
    expect(syncRecoveryKitUse({ ...lost, authority: { ...lost.authority, devices: [anchor] } }, "offline")).toBeNull();
  });

  it("limits late decisions to admin desktops and reset to the coordinator", () => {
    expect(canResolveSyncLateChanges(context())).toBe(true);
    expect(canResolveSyncLateChanges(context({ mobileLayout: true }))).toBe(false);
    expect(canResolveSyncLateChanges(context({}, { ownIsAdmin: false }))).toBe(false);
    expect(canResetSyncState("hub", false)).toBe(true);
    expect(canResetSyncState("client", false)).toBe(false);
    expect(canResetSyncState("hub", true)).toBe(false);
    expect(canResetSyncState(null, false)).toBe(false);
  });
});

describe("sync recovery kit format", () => {
  it("keeps the native groups and splits them into lines", () => {
    const lines = syncKitCodeLines(CODE);
    expect(lines).toHaveLength(4);
    expect(lines[0]).toBe("0123-4567-89AB-CDEF");
    expect(lines[3]).toBe("GHJK-MNPQ");
    expect(lines.join("-")).toBe(CODE);
    expect(syncKitCodeLines(` ${CODE} `, 7)).toHaveLength(2);
    expect(syncKitCodeLines("")).toEqual([]);
    expect(() => syncKitCodeLines(CODE, 0)).toThrow();
  });

  it("composes a text file with the warning and every code line", () => {
    const text = syncKitFileText(t, "en", CODE, NOW);
    expect(text.startsWith("Ganbaru recovery kit\n")).toBe(true);
    expect(text).toContain("Anyone with this code");
    expect(text).toContain("Code:\n0123-4567-89AB-CDEF\n");
    expect(text).toContain("2026");
    expect(text.endsWith("GHJK-MNPQ\n")).toBe(true);
    expect(syncKitFileName(t)).toBe("ganbaru-recovery-kit.txt");
  });
});

describe("sync outbox presentation", () => {
  it("names the target device and falls back for unnamed ones", () => {
    expect(syncOutboxChangeLabel(t, {
      change: "remove",
      target: { deviceId: "b", deviceLabel: "Phone", ownDevice: false },
      createdAtMs: 1,
      state: "pending",
      rejection: null,
    })).toBe("Remove Phone");
    expect(syncOutboxChangeLabel(t, { change: "makeAdmin", target: null, createdAtMs: 1, state: "pending", rejection: null }))
      .toBe("Make Unnamed device an admin");
    expect(syncOutboxChangeLabel(t, { change: "handover", target: null, createdAtMs: 1, state: "held", rejection: null }))
      .toBe("Hand over the anchor");
  });

  it("explains each refusal and plain waiting states", () => {
    const labels = SYNC_PROPOSAL_REJECTIONS.map((reason) => syncRejectionLabel(t, reason));
    expect(new Set(labels).size).toBe(SYNC_PROPOSAL_REJECTIONS.length);
    expect(labels.every((label) => label.length > 0 && !label.includes("_"))).toBe(true);
    expect(syncOutboxStateLine(t, { change: "remove", target: null, createdAtMs: 1, state: "rejected", rejection: "last_admin" }))
      .toBe("Refused. It would leave the vault without an admin.");
    expect(syncOutboxStateLine(t, { change: "remove", target: null, createdAtMs: 1, state: "held", rejection: null }))
      .toBe("Waiting for this device to catch up");
  });
});

describe("sync late change presentation", () => {
  const change: SyncLateChange = {
    removalRevision: 4,
    device: { deviceId: "b", deviceLabel: "Old phone", ownDevice: false },
    table: "quick_notes",
    rowKey: "note-1",
    kind: "values",
    title: "Plan",
    preview: "",
    changedAtMs: NOW - 5 * 60_000,
    decisionPending: false,
  };

  it("says what the removed device did, where, and when", () => {
    expect(syncLateChangeOrigin(t, "en", change, NOW)).toBe("Edited on Old phone, 5 min ago");
    expect(syncLateChangeOrigin(t, "en", { ...change, kind: "tombstone", changedAtMs: null }, NOW)).toBe("Deleted on Old phone");
    expect(syncLateChangeOrigin(t, "en", { ...change, kind: "created", device: { ...change.device, deviceLabel: null } }, NOW))
      .toBe("Created on another device, 5 min ago");
  });

  it("keys late changes by removal, table, and row", () => {
    expect(syncLateChangeKey(change)).toBe("4:quick_notes:note-1");
    expect(syncLateChangeKey({ ...change, removalRevision: 5 })).not.toBe(syncLateChangeKey(change));
  });
});

describe("sync unconfirmed changes", () => {
  it("stays silent without unconfirmed changes", () => {
    expect(syncUnconfirmedNotice(t, "en", status({}), NOW)).toBeNull();
  });

  it("counts changes and suggests a handover once the oldest is old enough", () => {
    const recent = syncUnconfirmedNotice(t, "en", status({ unconfirmed: 2, oldestUnconfirmedAtMs: NOW - 10 * 60_000 }), NOW);
    expect(recent?.suggestHandover).toBe(false);
    expect(recent?.lines).toEqual([
      "The anchor has not confirmed storing 2 changes from this device yet.",
      "The oldest one was made 10 min ago.",
    ]);
    const old = syncUnconfirmedNotice(
      t,
      "en",
      status({ unconfirmed: 1, oldestUnconfirmedAtMs: NOW - SYNC_HANDOVER_SUGGESTION_MS }),
      NOW,
    );
    expect(old?.suggestHandover).toBe(true);
    expect(old?.lines).toHaveLength(3);
    expect(syncUnconfirmedNotice(t, "en", status({ unconfirmed: 1, oldestUnconfirmedAtMs: NOW - 2_000 }), NOW, 1_000)?.suggestHandover)
      .toBe(true);
  });

  it("says since when the anchor stopped answering only on a linked device", () => {
    const since = NOW - 10 * 60_000;
    expect(syncAnchorUnreachableLine(t, "en", status({ role: "client", anchorUnreachableSinceMs: since }), NOW))
      .toBe("The anchor stopped answering 10 min ago.");
    expect(syncAnchorUnreachableLine(t, "en", status({ role: "hub", anchorUnreachableSinceMs: since }), NOW)).toBeNull();
    expect(syncAnchorUnreachableLine(t, "en", status({ role: "client" }), NOW)).toBeNull();
  });
});
