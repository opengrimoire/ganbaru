import { describe, expect, it } from "vitest";
import type {
  SyncContest,
  SyncContestCandidate,
  SyncContests,
  SyncDeviceRef,
  SyncKitPoint,
} from "$lib/api/sync";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import {
  syncCandidateActions,
  syncCandidateDetails,
  syncContestCauseLine,
  syncKitContested,
  syncKitPointLabel,
  syncKitPoints,
  syncKitReviewDevices,
  syncLiveObjection,
  syncStorageAlertLines,
} from "./contests";

const t: Translate = ((key, ...args) => translateFromPartialCatalog({}, key, ...args)) as Translate;
const HASH_A = "a".repeat(64);
const HASH_B = "b".repeat(64);
const HASH_C = "c".repeat(64);

function ref(deviceId: string, ownDevice = false): SyncDeviceRef {
  return { deviceId, deviceLabel: deviceId, ownDevice };
}

function candidate(hash: string, target: SyncDeviceRef, overrides: Partial<SyncContestCandidate> = {}): SyncContestCandidate {
  return {
    hash,
    parent: { hash: HASH_A, revision: 7 },
    kind: "forced",
    target,
    signer: target,
    targetEnrolledRevision: 2,
    targetEnroller: ref("Anchor"),
    withdrawnInFavorOf: null,
    local: false,
    ...overrides,
  };
}

const own = ref("Desk", true);
const laptop = ref("Laptop");
const intruder = ref("Intruder");
const mine = candidate(HASH_B, own, { local: true });
const theirs = candidate(HASH_C, intruder, { targetEnroller: null });

function contest(overrides: Partial<SyncContest> = {}): SyncContest {
  return {
    rootHash: HASH_A,
    epoch: 3,
    anchor: ref("Anchor"),
    frozenAt: { hash: HASH_A, revision: 7 },
    cause: "contested",
    candidates: [mine, theirs],
    objections: [],
    yields: [],
    held: [],
    localRole: { kind: "contestant", candidate: HASH_B },
    actions: [],
    ...overrides,
  };
}

function point(place: SyncKitPoint["place"], hash = HASH_A): SyncKitPoint {
  return { entry: { hash, revision: 7 }, epoch: 3, place };
}

function contests(overrides: Partial<SyncContests> = {}): SyncContests {
  return { contests: [], losingRemovals: [], storage: { backstop: null, alerts: [] }, ...overrides };
}

describe("sync kit device review", () => {
  it("preselects only flooders and forked writers and lists losing removals unmarked with who removed them", () => {
    const review = syncKitReviewDevices(contests({
      losingRemovals: [
        { device: laptop, removedBy: intruder, revision: 8 },
        { device: ref("Phone"), removedBy: null, revision: 9 },
      ],
      storage: {
        backstop: null,
        alerts: [
          { kind: "device", device: intruder, bytes: 5_000_000 },
          { kind: "total", bytes: 9_000_000 },
          { kind: "forkedWriter", writer: "e".repeat(32), device: ref("Twin") },
          { kind: "forkedWriter", writer: "f".repeat(32), device: null },
        ],
      },
    }));
    expect(review.map((entry) => [entry.device.deviceId, entry.reason, entry.preselected])).toEqual([
      ["Intruder", "flooder", true],
      ["Twin", "flooder", true],
      ["Laptop", "losingRemoval", false],
      ["Phone", "losingRemoval", false],
    ]);
    expect(review[2]?.removedBy).toBe(intruder);
    expect(review[3]?.removedBy).toBeNull();
  });

  it("never lists this device and lists a device once, marked when any reason marks it", () => {
    const review = syncKitReviewDevices(contests({
      losingRemovals: [
        { device: own, removedBy: intruder, revision: 8 },
        { device: intruder, removedBy: laptop, revision: 9 },
        { device: laptop, removedBy: intruder, revision: 10 },
        { device: laptop, removedBy: null, revision: 11 },
      ],
      storage: {
        backstop: "stopped",
        alerts: [
          { kind: "device", device: own, bytes: 1 },
          { kind: "device", device: intruder, bytes: 1 },
          { kind: "forkedWriter", writer: "e".repeat(32), device: intruder },
        ],
      },
    }));
    expect(review.map((entry) => [entry.device.deviceId, entry.preselected])).toEqual([
      ["Intruder", true],
      ["Laptop", false],
    ]);
    expect(syncKitReviewDevices(null)).toEqual([]);
  });
});

describe("sync kit points", () => {
  it("reads the kit points of the first contest that offers the kit", () => {
    const points = [point({ kind: "head" }), point({ kind: "candidate", target: intruder }, HASH_C)];
    const offered = contests({
      contests: [contest(), contest({ rootHash: HASH_B, actions: [{ kind: "useKit", points }] })],
    });
    expect(syncKitPoints(offered)).toBe(points);
    expect(syncKitContested(offered)).toBe(true);
    expect(syncKitContested(contests({ contests: [contest()] }))).toBe(false);
    expect(syncKitContested(null)).toBe(false);
  });

  it("labels each kit point by where it lies", () => {
    expect(syncKitPointLabel(t, point({ kind: "head" }))).toBe("The current device list");
    expect(syncKitPointLabel(t, point({ kind: "candidate", target: intruder }))).toBe("The takeover by Intruder");
    expect(syncKitPointLabel(t, point({ kind: "continuation", anchor: laptop }))).toBe(
      "Changes Laptop made during the contest",
    );
  });
});

describe("sync contest presentation", () => {
  it("offers per candidate only the actions the contest names for it", () => {
    const offered = contest({
      actions: [
        { kind: "yieldTo", candidate: HASH_C },
        { kind: "object", candidate: HASH_C },
        { kind: "withdrawObjection", candidate: HASH_B },
      ],
    });
    expect(syncCandidateActions(offered, theirs)).toEqual({ yieldTo: true, object: true, withdrawObjection: false });
    expect(syncCandidateActions(offered, mine)).toEqual({ yieldTo: false, object: false, withdrawObjection: true });
  });

  it("finds only an objection that still stands", () => {
    const withdrawn = { hash: HASH_A, against: HASH_C, objector: own, withdrawn: true };
    const live = { ...withdrawn, withdrawn: false };
    expect(syncLiveObjection(contest({ objections: [withdrawn] }), theirs)).toBeNull();
    expect(syncLiveObjection(contest({ objections: [withdrawn, live] }), theirs)).toBe(live);
    expect(syncLiveObjection(contest({ objections: [live] }), mine)).toBeNull();
  });

  it("explains the cause and names the taker of a lone take", () => {
    expect(syncContestCauseLine(t, contest())).toContain("More than one device");
    expect(syncContestCauseLine(t, contest({ cause: "writerFork" }))).toContain("used in two places");
    const taken = contest({ cause: "taken", frozenAt: null, candidates: [theirs] });
    expect(syncContestCauseLine(t, taken)).toBe("Intruder took over the anchor from this computer. If it is not yours, block it.");
  });

  it("details how a candidate was signed, enrolled, withdrawn, and blocked", () => {
    const blocked = contest({
      candidates: [mine, { ...theirs, kind: "kit", withdrawnInFavorOf: HASH_B }],
      objections: [{ hash: HASH_A, against: HASH_C, objector: null, withdrawn: false }],
    });
    expect(syncCandidateDetails(t, blocked, blocked.candidates[1]!)).toEqual([
      "Signed with the recovery kit",
      "Enrolled at revision 2 with a recovery key",
      "Withdrawn in favor of This device",
      "Blocked by the anchor",
    ]);
    expect(syncCandidateDetails(t, contest(), candidate(HASH_B, laptop, { targetEnrolledRevision: null }))).toEqual([
      "Forced takeover with an admin key",
    ]);
    expect(syncCandidateDetails(t, contest(), { ...theirs, targetEnroller: laptop })).toEqual(["Forced takeover with an admin key", "Enrolled at revision 2 by Laptop"]);
  });

  it("writes storage alerts as status lines with their size", () => {
    const lines = syncStorageAlertLines(t, "en", {
      backstop: "reserve",
      alerts: [
        { kind: "device", device: intruder, bytes: 4_500_000 },
        { kind: "total", bytes: 12_000_000 },
        { kind: "forkedWriter", writer: "e".repeat(32), device: laptop },
        { kind: "forkedWriter", writer: "f".repeat(32), device: null },
      ],
    });
    expect(lines).toEqual([
      "Intruder sent an unusual amount of sync records (4.5 MB).",
      "Sync records take 12 MB.",
      "The sync identity of Laptop was used in two places at once.",
      "A sync identity was used in two places at once.",
    ]);
  });
});
