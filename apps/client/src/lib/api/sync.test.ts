import { describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  discardSyncLateChange,
  forceSyncAnchor,
  getPendingSyncRecoveryKit,
  getSyncAuthority,
  getSyncContests,
  listenSyncApplied,
  listenSyncStatus,
  listSyncLateChanges,
  listSyncOutbox,
  listSyncRecovery,
  mapSyncAuthority,
  mapSyncContests,
  mapSyncLateChange,
  mapSyncLateChanges,
  mapSyncNotice,
  mapSyncOutboxEntry,
  mapSyncPendingRecoveryKit,
  mapSyncStatus,
  objectSyncContest,
  OFF_SYNC_STATUS,
  regenerateSyncRecoveryKit,
  removeSyncDevice,
  restoreSyncLateChange,
  restoreSyncRecovery,
  resumeSyncRecords,
  saveSyncRecoveryKitFile,
  takeSyncAnchor,
  yieldSyncContest,
} from "./sync";

const channels = vi.hoisted(() => [] as { onmessage: (value: unknown) => void }[]);

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  Channel: class {
    onmessage: (value: unknown) => void = () => {};
    constructor() {
      channels.push(this);
    }
  },
}));

function validStatus(): Record<string, unknown> {
  return {
    state: "idle",
    role: "client",
    paused: false,
    lastExchangeAtMs: 1_000,
    lastError: null,
    lastErrorCode: null,
    pendingLocalChanges: 2,
    waiting: 0,
    held: { newerFormat: 0, newerManifest: 1, invalid: 0 },
    conflicts: [{ table: "quick_notes", count: 3 }],
    recovery: [{ table: "quick_notes", count: 1 }],
    clockAheadDevices: [{ deviceId: "device-b", deviceLabel: "Laptop", ownDevice: false }],
    uncountedDevices: [{ device: { deviceId: "device-c", deviceLabel: "Old phone", ownDevice: false }, standing: "dormant" }],
    connection: { network: "lan", address: "192.168.1.20:43821" },
    degraded: ["localNetworkDenied"],
    unconfirmed: 3,
    oldestUnconfirmedAtMs: 500,
    displacedVersions: 4,
    anchorUnreachableSinceMs: 700,
  };
}

describe("sync API boundary", () => {
  it("maps a native status", () => {
    expect(mapSyncStatus(validStatus())).toEqual({
      state: "idle",
      role: "client",
      paused: false,
      lastExchangeAtMs: 1_000,
      lastError: null,
      lastErrorCode: null,
      pendingLocalChanges: 2,
      waiting: 0,
      held: { newerFormat: 0, newerManifest: 1, invalid: 0 },
      conflicts: [{ table: "quick_notes", count: 3 }],
      recovery: [{ table: "quick_notes", count: 1 }],
      clockAheadDevices: [{ deviceId: "device-b", deviceLabel: "Laptop", ownDevice: false }],
      uncountedDevices: [{ device: { deviceId: "device-c", deviceLabel: "Old phone", ownDevice: false }, standing: "dormant" }],
      connection: { network: "lan", address: "192.168.1.20:43821" },
      degraded: ["localNetworkDenied"],
      unconfirmed: 3,
      oldestUnconfirmedAtMs: 500,
      displacedVersions: 4,
      anchorUnreachableSinceMs: 700,
    });
  });

  it("maps the off status of an unlinked vault", () => {
    expect(mapSyncStatus({ ...OFF_SYNC_STATUS, held: { ...OFF_SYNC_STATUS.held } })).toEqual(OFF_SYNC_STATUS);
  });

  it("maps a known error code", () => {
    expect(mapSyncStatus({ ...validStatus(), lastErrorCode: "writerLimit" }).lastErrorCode).toBe("writerLimit");
    expect(mapSyncStatus({ ...validStatus(), lastErrorCode: "bootstrapRequired" }).lastErrorCode).toBe("bootstrapRequired");
  });

  it("maps every device standing and rejects malformed uncounted devices", () => {
    const device = { deviceId: "device-c", deviceLabel: null, ownDevice: false };
    const status = mapSyncStatus({
      ...validStatus(),
      uncountedDevices: [{ device, standing: "dormant" }, { device, standing: "relinkRequired" }],
    });
    expect(status.uncountedDevices.map((entry) => entry.standing)).toEqual(["dormant", "relinkRequired"]);
    expect(() => mapSyncStatus({ ...validStatus(), uncountedDevices: [{ device, standing: "counted" }] })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), uncountedDevices: [{ standing: "dormant" }] })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), uncountedDevices: undefined })).toThrow();
  });

  it("maps an overlay connection and every degraded reason", () => {
    const status = mapSyncStatus({
      ...validStatus(),
      connection: { network: "overlay", address: "[fd7a:115c:a1e0::1]:43821" },
      degraded: ["backgroundRestricted", "standbyRestricted", "localNetworkDenied", "firewallAuthorizationRequired"],
    });
    expect(status.connection).toEqual({ network: "overlay", address: "[fd7a:115c:a1e0::1]:43821" });
    expect(status.degraded).toEqual([
      "backgroundRestricted",
      "standbyRestricted",
      "localNetworkDenied",
      "firewallAuthorizationRequired",
    ]);
  });

  it("rejects malformed connections and degraded reasons", () => {
    expect(() => mapSyncStatus({ ...validStatus(), connection: { network: "wan", address: "1.2.3.4:1" } })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), connection: { network: "lan" } })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), connection: undefined })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), degraded: ["batteryLow"] })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), degraded: ["localNetworkDenied", "localNetworkDenied"] })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), degraded: undefined })).toThrow();
  });

  it("rejects unknown states, roles, and negative counts", () => {
    expect(() => mapSyncStatus({ ...validStatus(), state: "linked" })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), role: "relay" })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), waiting: -1 })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), lastErrorCode: "diskFull" })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), clockAheadDevices: [{ deviceId: "device-b" }] })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), held: { newerFormat: 0, newerManifest: 0 } })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), recovery: [{ table: "quick_notes", count: -1 }] })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), recovery: 1 })).toThrow();
  });

  it("maps status and applied notices and rejects malformed ones", () => {
    expect(mapSyncNotice({ kind: "applied", tables: ["quick_notes", "quick_note_tags"] })).toEqual({
      kind: "applied",
      tables: ["quick_notes", "quick_note_tags"],
    });
    expect(mapSyncNotice({ kind: "status", status: validStatus() })).toEqual({ kind: "status", status: mapSyncStatus(validStatus()) });
    expect(() => mapSyncNotice({ kind: "applied", tables: [1] })).toThrow();
    expect(() => mapSyncNotice({ kind: "status", status: { ...validStatus(), state: "linked" } })).toThrow();
    expect(() => mapSyncNotice({ kind: "moved", tables: [] })).toThrow();
    expect(() => mapSyncNotice(["quick_notes"])).toThrow();
  });

  it("shares one subscription, routes notices by kind, and unsubscribes after the last listener", async () => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(undefined);
    channels.length = 0;
    const statuses: unknown[] = [];
    const applied: string[][] = [];
    const stopStatus = await listenSyncStatus((status) => statuses.push(status));
    const stopApplied = await listenSyncApplied((tables) => applied.push(tables));

    expect(channels).toHaveLength(1);
    const subscribe = vi.mocked(invoke).mock.calls.filter(([command]) => command === "sync_subscribe");
    expect(subscribe).toHaveLength(1);
    const { subscriptionId } = subscribe[0]?.[1] as { subscriptionId: string };

    channels[0]?.onmessage({ kind: "applied", tables: ["quick_notes"] });
    channels[0]?.onmessage({ kind: "status", status: validStatus() });
    channels[0]?.onmessage({ kind: "applied", tables: "quick_notes" });
    expect(applied).toEqual([["quick_notes"]]);
    expect(statuses).toEqual([mapSyncStatus(validStatus())]);

    stopStatus();
    expect(invoke).not.toHaveBeenCalledWith("sync_unsubscribe", expect.anything());
    stopApplied();
    stopApplied();
    await Promise.resolve();
    await Promise.resolve();
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "sync_unsubscribe")).toEqual([
      ["sync_unsubscribe", { subscriptionId }],
    ]);
  });

  it("drops a listener whose subscription fails so the next one retries", async () => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockRejectedValueOnce(new Error("closed")).mockResolvedValue(undefined);
    await expect(listenSyncApplied(() => {})).rejects.toThrow("closed");
    const stop = await listenSyncApplied(() => {});
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "sync_subscribe")).toHaveLength(2);
    stop();
  });

  it("maps recovery offers with their device", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([
      {
        table: "quick_notes",
        rowKey: "note-1",
        title: "Plan",
        preview: "Start",
        descendants: 2,
        unseen: [{ title: "", preview: "Written offline" }],
        device: { deviceId: "device-b", deviceLabel: null, ownDevice: false },
        editedAtMs: 2_000,
      },
    ]);
    await expect(listSyncRecovery()).resolves.toEqual([
      {
        table: "quick_notes",
        rowKey: "note-1",
        title: "Plan",
        preview: "Start",
        descendants: 2,
        unseen: [{ title: "", preview: "Written offline" }],
        device: { deviceId: "device-b", deviceLabel: null, ownDevice: false },
        editedAtMs: 2_000,
      },
    ]);
  });

  it("rejects recovery offers without a valid descendant count", async () => {
    const offer = {
      table: "quick_notes",
      rowKey: "note-1",
      title: "Plan",
      preview: "Start",
      unseen: [],
      device: { deviceId: "device-b", deviceLabel: null, ownDevice: false },
      editedAtMs: 2_000,
    };
    vi.mocked(invoke).mockResolvedValueOnce([offer]);
    await expect(listSyncRecovery()).rejects.toThrow();
    vi.mocked(invoke).mockResolvedValueOnce([{ ...offer, descendants: -1 }]);
    await expect(listSyncRecovery()).rejects.toThrow();
  });

  it("rejects recovery offers without valid unseen texts", async () => {
    const offer = {
      table: "projects",
      rowKey: "project-1",
      title: "Launch",
      preview: "",
      descendants: 1,
      device: { deviceId: "device-b", deviceLabel: null, ownDevice: false },
      editedAtMs: 2_000,
    };
    vi.mocked(invoke).mockResolvedValueOnce([offer]);
    await expect(listSyncRecovery()).rejects.toThrow();
    vi.mocked(invoke).mockResolvedValueOnce([{ ...offer, unseen: [{ title: "" }] }]);
    await expect(listSyncRecovery()).rejects.toThrow();
  });

  it("sends the recovery target and returns the restored id", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("note-9");
    await expect(restoreSyncRecovery({ table: "quick_notes", rowKey: "note-1" })).resolves.toBe("note-9");
    expect(invoke).toHaveBeenLastCalledWith("sync_recovery_restore", { target: { table: "quick_notes", rowKey: "note-1" } });
  });

  it("rejects malformed unconfirmed counts", () => {
    expect(() => mapSyncStatus({ ...validStatus(), unconfirmed: -1 })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), unconfirmed: undefined })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), oldestUnconfirmedAtMs: "yesterday" })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), displacedVersions: undefined })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), anchorUnreachableSinceMs: -1 })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), anchorUnreachableSinceMs: undefined })).toThrow();
  });
});

function validDevice(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    device: { deviceId: "device-a", deviceLabel: "Desk", ownDevice: true },
    kind: "desktop",
    role: "admin",
    enrolledRevision: 1,
    removedRevision: null,
    quarantined: false,
    anchor: true,
    ...overrides,
  };
}

function validAuthority(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    anchorDeviceId: "device-a",
    epoch: 1,
    revision: 4,
    frozen: false,
    ownIsAdmin: true,
    devices: [
      validDevice(),
      validDevice({
        device: { deviceId: "device-b", deviceLabel: null, ownDevice: false },
        kind: "mobile",
        role: "member",
        enrolledRevision: 2,
        removedRevision: 4,
        quarantined: true,
        anchor: false,
      }),
    ],
    ...overrides,
  };
}

function validLateChange(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    removalRevision: 4,
    device: { deviceId: "device-b", deviceLabel: null, ownDevice: false },
    table: "quick_notes",
    rowKey: "note-1",
    kind: "values",
    title: "Plan",
    preview: "Start",
    changedAtMs: 3_000,
    decisionPending: false,
    ...overrides,
  };
}

describe("sync authority API boundary", () => {
  it("maps the authority state with every device field", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(validAuthority());
    const authority = await getSyncAuthority();
    expect(authority?.devices[1]).toEqual({
      device: { deviceId: "device-b", deviceLabel: null, ownDevice: false },
      kind: "mobile",
      role: "member",
      enrolledRevision: 2,
      removedRevision: 4,
      quarantined: true,
      anchor: false,
    });
    expect(authority?.ownIsAdmin).toBe(true);
    vi.mocked(invoke).mockResolvedValueOnce(null);
    await expect(getSyncAuthority()).resolves.toBeNull();
  });

  it("rejects malformed authority states", () => {
    expect(() => mapSyncAuthority(validAuthority({ frozen: "no" }))).toThrow();
    expect(() => mapSyncAuthority(validAuthority({ devices: [validDevice({ role: "owner" })] }))).toThrow();
    expect(() => mapSyncAuthority(validAuthority({ devices: [validDevice({ kind: "tablet" })] }))).toThrow();
    expect(() => mapSyncAuthority(validAuthority({ devices: [validDevice({ removedRevision: -1 })] }))).toThrow();
    expect(() => mapSyncAuthority(validAuthority({ devices: [validDevice(), validDevice()] }))).toThrow();
    expect(() =>
      mapSyncAuthority(validAuthority({
        devices: [validDevice(), validDevice({ device: { deviceId: "device-c", deviceLabel: null, ownDevice: false } })],
      })),
    ).toThrow();
  });

  it("maps outbox entries and ties the rejection reason to the rejected state", async () => {
    const target = { deviceId: "device-b", deviceLabel: "Phone", ownDevice: false };
    vi.mocked(invoke).mockResolvedValueOnce([
      { change: "remove", target, createdAtMs: 9, state: "rejected", rejection: "last_admin" },
      { change: "replaceRecoveryKit", target: null, createdAtMs: 8, state: "pending", rejection: null },
      { change: "handover", target: null, createdAtMs: 7, state: "held", rejection: null },
    ]);
    const entries = await listSyncOutbox();
    expect(entries.map((entry) => [entry.change, entry.state, entry.rejection])).toEqual([
      ["remove", "rejected", "last_admin"],
      ["replaceRecoveryKit", "pending", null],
      ["handover", "held", null],
    ]);
    expect(entries[0]?.target).toEqual(target);
    expect(() => mapSyncOutboxEntry({ change: "remove", target: null, createdAtMs: 1, state: "rejected", rejection: null }, "entry")).toThrow();
    expect(() => mapSyncOutboxEntry({ change: "remove", target: null, createdAtMs: 1, state: "pending", rejection: "not_admin" }, "entry")).toThrow();
    expect(() => mapSyncOutboxEntry({ change: "remove", target: null, createdAtMs: 1, state: "rejected", rejection: "bad_luck" }, "entry")).toThrow();
    expect(() => mapSyncOutboxEntry({ change: "rename", target: null, createdAtMs: 1, state: "pending", rejection: null }, "entry")).toThrow();
  });

  it("maps late changes, including a deletion without a time", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      changes: [validLateChange(), validLateChange({ kind: "tombstone", changedAtMs: null, decisionPending: true })],
      total: 5,
    });
    const { changes, total } = await listSyncLateChanges();
    expect(total).toBe(5);
    expect(changes[0]).toEqual({ ...validLateChange() });
    expect(changes[1]).toMatchObject({ kind: "tombstone", changedAtMs: null, decisionPending: true });
    expect(() => mapSyncLateChanges({ changes: [validLateChange()], total: 0 })).toThrow();
    expect(() => mapSyncLateChanges({ changes: [validLateChange()] })).toThrow();
    expect(() => mapSyncLateChanges([validLateChange()])).toThrow();
    expect(() => mapSyncLateChange(validLateChange({ kind: "moved" }), "late")).toThrow();
    expect(() => mapSyncLateChange(validLateChange({ removalRevision: undefined }), "late")).toThrow();
    expect(() => mapSyncLateChange(validLateChange({ decisionPending: 1 }), "late")).toThrow();
  });

  it("maps the pending recovery kit and refuses an empty code", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ code: "ABCD-EFGH", active: true });
    await expect(getPendingSyncRecoveryKit()).resolves.toEqual({ code: "ABCD-EFGH", active: true });
    vi.mocked(invoke).mockResolvedValueOnce(null);
    await expect(getPendingSyncRecoveryKit()).resolves.toBeNull();
    expect(() => mapSyncPendingRecoveryKit({ code: " ", active: true }, "kit")).toThrow();
    vi.mocked(invoke).mockResolvedValueOnce("");
    await expect(regenerateSyncRecoveryKit()).rejects.toThrow();
    vi.mocked(invoke).mockResolvedValueOnce("ABCD-EFGH");
    await expect(regenerateSyncRecoveryKit()).resolves.toBe("ABCD-EFGH");
  });

  it("sends late change targets, device ids, and kit text with their native names", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    const target = { removalRevision: 4, table: "quick_notes", rowKey: "note-1" };
    await restoreSyncLateChange(target);
    expect(invoke).toHaveBeenLastCalledWith("sync_late_change_restore", { target });
    await discardSyncLateChange(target);
    expect(invoke).toHaveBeenLastCalledWith("sync_late_change_discard", { target });
    await removeSyncDevice("device-b");
    expect(invoke).toHaveBeenLastCalledWith("sync_remove_device", { deviceId: "device-b" });
    vi.mocked(invoke).mockResolvedValueOnce(true);
    await expect(saveSyncRecoveryKitFile("kit.txt", "code")).resolves.toBe(true);
    expect(invoke).toHaveBeenLastCalledWith("vault_pick_and_write_recovery_kit", { defaultName: "kit.txt", contents: "code" });
    vi.mocked(invoke).mockResolvedValueOnce("yes");
    await expect(saveSyncRecoveryKitFile("kit.txt", "code")).rejects.toThrow();
    vi.mocked(invoke).mockResolvedValueOnce(-1);
    await expect(takeSyncAnchor()).rejects.toThrow();
  });
});

const HASH_A = "a".repeat(64);
const HASH_B = "b".repeat(64);
const HASH_C = "c".repeat(64);
const HASH_D = "d".repeat(64);

function ref(deviceId: string, ownDevice = false): Record<string, unknown> {
  return { deviceId, deviceLabel: null, ownDevice };
}

function validCandidate(hash: string, target: string, overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    hash,
    parent: { hash: HASH_A, revision: 7 },
    kind: "forced",
    target: ref(target),
    signer: ref(target),
    targetEnrolledRevision: 2,
    targetEnroller: ref("anchor"),
    withdrawnInFavorOf: null,
    local: false,
    ...overrides,
  };
}

function validContest(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    rootHash: HASH_A,
    epoch: 3,
    anchor: ref("anchor"),
    frozenAt: { hash: HASH_A, revision: 7 },
    cause: "contested",
    candidates: [
      validCandidate(HASH_B, "laptop", { local: true, target: ref("own", true) }),
      validCandidate(HASH_C, "intruder", { withdrawnInFavorOf: HASH_B, signer: null, targetEnroller: null }),
    ],
    objections: [{ hash: HASH_D, against: HASH_C, objector: ref("anchor"), withdrawn: false }],
    yields: [{ hash: HASH_D, author: null, to: HASH_B }],
    held: [ref("intruder")],
    localRole: { kind: "contestant", candidate: HASH_B },
    actions: [
      { kind: "yieldTo", candidate: HASH_C },
      { kind: "useKit", points: [{ entry: { hash: HASH_A, revision: 7 }, epoch: 3, place: { kind: "head" } }] },
    ],
    ...overrides,
  };
}

function validContests(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    contests: [validContest()],
    losingRemovals: [{ device: ref("phone"), removedBy: ref("intruder"), revision: 8 }],
    storage: {
      backstop: "reserve",
      alerts: [
        { kind: "device", device: ref("intruder"), bytes: 4_000_000 },
        { kind: "total", bytes: 9_000_000 },
        { kind: "forkedWriter", writer: "e".repeat(32), device: null },
      ],
    },
    ...overrides,
  };
}

describe("sync contest API boundary", () => {
  it("maps contests, losing removals, and storage alerts", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(validContests());
    const contests = await getSyncContests();
    expect(invoke).toHaveBeenLastCalledWith("sync_contests");
    expect(contests?.contests[0]?.candidates[1]).toEqual({
      hash: HASH_C,
      parent: { hash: HASH_A, revision: 7 },
      kind: "forced",
      target: { deviceId: "intruder", deviceLabel: null, ownDevice: false },
      signer: null,
      targetEnrolledRevision: 2,
      targetEnroller: null,
      withdrawnInFavorOf: HASH_B,
      local: false,
    });
    expect(contests?.contests[0]?.localRole).toEqual({ kind: "contestant", candidate: HASH_B });
    expect(contests?.losingRemovals[0]?.removedBy?.deviceId).toBe("intruder");
    expect(contests?.storage.backstop).toBe("reserve");
    expect(contests?.storage.alerts.map((alert) => alert.kind)).toEqual(["device", "total", "forkedWriter"]);
    vi.mocked(invoke).mockResolvedValueOnce(null);
    await expect(getSyncContests()).resolves.toBeNull();
  });

  it("maps a lone take without a freeze point", () => {
    const taken = validContest({ cause: "taken", frozenAt: null, localRole: { kind: "anchor" }, actions: [] });
    expect(mapSyncContests(validContests({ contests: [taken] })).contests[0]?.frozenAt).toBeNull();
  });

  it("rejects contests that break their invariants", () => {
    const rejects = (contest: Record<string, unknown>) =>
      expect(() => mapSyncContests(validContests({ contests: [contest] }))).toThrow();
    rejects(validContest({ cause: "taken" }));
    rejects(validContest({ frozenAt: null }));
    rejects(validContest({ candidates: [validCandidate(HASH_B, "a"), validCandidate(HASH_B, "b")] }));
    rejects(validContest({ actions: [{ kind: "object", candidate: HASH_D }] }));
    rejects(validContest({ actions: [{ kind: "useKit", points: [] }] }));
    rejects(validContest({ rootHash: HASH_A.toUpperCase() }));
    rejects(validContest({ rootHash: "a".repeat(63) }));
    rejects(validContest({ cause: "unknown" }));
    rejects(validContest({ candidates: [validCandidate(HASH_B, "a", { kind: "admin" })] }));
    rejects(validContest({ localRole: { kind: "contestant" } }));
    expect(() => mapSyncContests(validContests({ storage: { backstop: "full", alerts: [] } }))).toThrow();
    expect(() =>
      mapSyncContests(validContests({ storage: { backstop: null, alerts: [{ kind: "forkedWriter", writer: "e", device: null }] } })),
    ).toThrow();
    expect(() =>
      mapSyncContests(validContests({ storage: { backstop: null, alerts: [{ kind: "total", bytes: -1 }] } })),
    ).toThrow();
  });

  it("sends contest choices, the resume, and the kit review with their native names", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(12);
    await expect(yieldSyncContest(HASH_B)).resolves.toBe(12);
    expect(invoke).toHaveBeenLastCalledWith("sync_contest_yield", { handover: HASH_B });
    vi.mocked(invoke).mockResolvedValueOnce(13);
    await expect(objectSyncContest(HASH_C)).resolves.toBe(13);
    expect(invoke).toHaveBeenLastCalledWith("sync_contest_object", { handover: HASH_C });
    vi.mocked(invoke).mockResolvedValueOnce(-1);
    await expect(objectSyncContest(HASH_C)).rejects.toThrow();
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await resumeSyncRecords();
    expect(invoke).toHaveBeenLastCalledWith("sync_records_resume");
    vi.mocked(invoke).mockResolvedValueOnce(14);
    const point = { hash: HASH_A, revision: 7 };
    await expect(
      forceSyncAnchor({ kitCode: "CODE", point, removeDevices: ["intruder"], quarantineAnchor: false }),
    ).resolves.toBe(14);
    expect(invoke).toHaveBeenLastCalledWith("sync_force_anchor", {
      kitCode: "CODE",
      point,
      removeDevices: ["intruder"],
      quarantineAnchor: false,
    });
  });
});
