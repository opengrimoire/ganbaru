import { Channel, invoke } from "@tauri-apps/api/core";
import { parseQrMatrix, type PairingQrMatrix } from "$lib/api/vault-handoff";
import {
  readArray,
  readBoolean,
  readEnum,
  readNonNegativeSafeInteger,
  readNullable,
  readRecord,
  readString,
} from "$lib/utils/readers";

export const SYNC_STATES = ["off", "idle", "syncing", "offline", "paused", "waiting_for_identity", "error"] as const;
export type SyncState = (typeof SYNC_STATES)[number];

export const SYNC_ROLES = ["hub", "client"] as const;
export type SyncRole = (typeof SYNC_ROLES)[number];

/** Sync problems the interface explains in its own words. */
export const SYNC_ERROR_CODES = ["writerLimit", "bootstrapRequired", "coordinatorRemoved"] as const;
export type SyncErrorCode = (typeof SYNC_ERROR_CODES)[number];

/**
 * Why the coordinator does not count a linked device toward stability: it stored nothing for
 * long enough, or its link record has no valid time and it must be linked again.
 */
export const SYNC_DEVICE_STANDINGS = ["dormant", "relinkRequired"] as const;
export type SyncDeviceStanding = (typeof SYNC_DEVICE_STANDINGS)[number];

/** A linked device the coordinator does not count, with the reason. */
export interface SyncUncountedDevice {
  device: SyncDeviceRef;
  standing: SyncDeviceStanding;
}

/** The kind of network a linked device reached its coordinator over. */
export const SYNC_CONNECTION_NETWORKS = ["lan", "overlay"] as const;
export type SyncConnectionNetwork = (typeof SYNC_CONNECTION_NETWORKS)[number];

/** Why this device may sync late or not at all, in the order the native side reports them. */
export const SYNC_DEGRADED_REASONS = [
  "backgroundRestricted",
  "standbyRestricted",
  "localNetworkDenied",
  "firewallAuthorizationRequired",
  "firewallPeerListenerBlocked",
] as const;
export type SyncDegradedReason = (typeof SYNC_DEGRADED_REASONS)[number];

/** How a linked device last reached its coordinator in this app run. */
export interface SyncConnectionView {
  network: SyncConnectionNetwork;
  /** The coordinator endpoint, as `address:port`. */
  address: string;
}

/** What the last relay pass of this app run moved, and what still waits. */
export interface SyncRelayDelivery {
  /** When the last relay pass finished, in Unix milliseconds. */
  lastPassAtMs: number;
  /** Sealed changes the pass delivered to the relay. */
  sent: number;
  /** Sealed changes the pass received from the relay. */
  received: number;
  /** Changes of this device's writers the relay does not hold yet. */
  undelivered: number;
  /** Files the pass uploaded and downloaded. */
  uploadedFiles: number;
  downloadedFiles: number;
  /** Files left for a later pass because the network is metered. */
  deferredFiles: number;
  /** The relay's stored bytes for this vault and its quota; zero quota means no limit reported. */
  storedBytes: number;
  quotaBytes: number;
  /** Set when the pass stopped early or the relay needs an administrator. */
  problem: string | null;
}

/** Stored operations this device cannot apply, by reason. */
export interface SyncHeldCounts {
  newerFormat: number;
  newerManifest: number;
  invalid: number;
}

/** A count for one replicated table, by name. */
export interface SyncTableCount {
  table: string;
  count: number;
}

/** What the sync service is doing for the active vault. */
export interface SyncStatusView {
  state: SyncState;
  role: SyncRole | null;
  paused: boolean;
  lastExchangeAtMs: number | null;
  lastError: string | null;
  /** Set when the interface has its own explanation of the last error. */
  lastErrorCode: SyncErrorCode | null;
  /** Local rows with changes that are not sealed yet. */
  pendingLocalChanges: number;
  /** Stored operations waiting to be applied. */
  waiting: number;
  held: SyncHeldCounts;
  /** Rows with open conflicts, per table. */
  conflicts: SyncTableCount[];
  /** Recovery offers, per table of their root row. */
  recovery: SyncTableCount[];
  /** Devices whose edits carry clocks far ahead of this device, so they win concurrent edits. */
  clockAheadDevices: SyncDeviceRef[];
  /** On the coordinator, linked devices it does not count toward stability, in device id order. */
  uncountedDevices: SyncUncountedDevice[];
  /** Kept after a failed dial, so it describes the current link only while idle or syncing. */
  connection: SyncConnectionView | null;
  /** Why connectivity is degraded, without repeats. */
  degraded: SyncDegradedReason[];
  /** Changes this device made that the anchor has not confirmed storing yet. */
  unconfirmed: number;
  /** When the oldest unconfirmed change was made, in Unix milliseconds. */
  oldestUnconfirmedAtMs: number | null;
  /** Superseded versions kept until the anchor confirms storing the changes that superseded them. */
  displacedVersions: number;
  /**
   * On a linked device, since when no pass in this app run reached the anchor, in Unix
   * milliseconds. Null while the anchor answers, on the coordinator, and before the first pass.
   */
  anchorUnreachableSinceMs: number | null;
  /** The last relay pass of this app run, or null when the vault syncs without a relay or none ran yet. */
  relay: SyncRelayDelivery | null;
}

export interface SyncDeviceRef {
  deviceId: string;
  /** Null when this device does not know the other device's name. */
  deviceLabel: string | null;
  ownDevice: boolean;
}

/** A deleted row that kept edits its deletion did not see. */
export interface SyncRecoveryEntry {
  table: string;
  rowKey: string;
  title: string;
  preview: string;
  /** Deleted descendants that restoring the row brings back with it. */
  descendants: number;
  /**
   * Deleted descendants of another domain written or edited unseen by the deletion, such as Chat posts in a
   * project, which restoring the row does not bring back.
   */
  unseen: SyncRecoveryText[];
  /** Unseen rows past the ones listed in `unseen`, which the offer counts instead of listing. */
  unseenOmitted: number;
  /** Device of the newest unseen edit. */
  device: SyncDeviceRef;
  editedAtMs: number;
}

/** Title and preview of one deleted row in a recovery offer. */
export interface SyncRecoveryText {
  title: string;
  preview: string;
}

export interface SyncRecoveryTarget {
  table: string;
  rowKey: string;
}

export const OFF_SYNC_STATUS: SyncStatusView = {
  state: "off",
  role: null,
  paused: false,
  lastExchangeAtMs: null,
  lastError: null,
  lastErrorCode: null,
  pendingLocalChanges: 0,
  waiting: 0,
  held: { newerFormat: 0, newerManifest: 0, invalid: 0 },
  conflicts: [],
  recovery: [],
  clockAheadDevices: [],
  uncountedDevices: [],
  connection: null,
  degraded: [],
  unconfirmed: 0,
  oldestUnconfirmedAtMs: null,
  displacedVersions: 0,
  anchorUnreachableSinceMs: null,
  relay: null,
};

export const SYNC_DEVICE_KINDS = ["desktop", "mobile"] as const;
export type SyncDeviceKind = (typeof SYNC_DEVICE_KINDS)[number];

export const SYNC_AUTHORITY_ROLES = ["admin", "member"] as const;
export type SyncAuthorityRole = (typeof SYNC_AUTHORITY_ROLES)[number];

/** A device enrolled in the vault's sync space. */
export interface SyncAuthorityDevice {
  device: SyncDeviceRef;
  kind: SyncDeviceKind;
  role: SyncAuthorityRole;
  enrolledRevision: number;
  /** Set once the anchor sequenced the device's removal. */
  removedRevision: number | null;
  /** Whether this installation quarantined the device. */
  quarantined: boolean;
  /** Whether the device is the anchor that orders membership and role changes. */
  anchor: boolean;
}

/** The authorization state of the vault's sync space as this device last applied it. */
export interface SyncAuthorityView {
  anchorDeviceId: string;
  epoch: number;
  revision: number;
  /** Whether the space stopped accepting changes until a handover settles. */
  frozen: boolean;
  /** Whether this device is an admin desktop that holds its admin key. */
  ownIsAdmin: boolean;
  devices: SyncAuthorityDevice[];
}

export const SYNC_PROPOSAL_CHANGES = [
  "enroll",
  "remove",
  "makeAdmin",
  "makeMember",
  "replaceRecoveryKit",
  "resolveLate",
  "handover",
  "unknown",
] as const;
export type SyncProposalChange = (typeof SYNC_PROPOSAL_CHANGES)[number];

export const SYNC_OUTBOX_STATES = ["pending", "held", "rejected"] as const;
export type SyncOutboxState = (typeof SYNC_OUTBOX_STATES)[number];

/** Why the anchor refused a proposal, by the stable names the native side reports. */
export const SYNC_PROPOSAL_REJECTIONS = [
  "not_anchor",
  "wrong_space",
  "not_admin",
  "malformed",
  "unsupported_entry",
  "already_enrolled",
  "unknown_device",
  "device_removed",
  "names_anchor",
  "admin_not_desktop",
  "last_admin",
  "unknown_removal",
  "handover_conflict",
  "epoch_frozen",
  "catching_up",
  "device_quarantined",
  "key_unavailable",
  "anchor_only",
  "rotation_pending",
] as const;
export type SyncProposalRejection = (typeof SYNC_PROPOSAL_REJECTIONS)[number];

/** A proposal this device made that waits for the anchor or that the anchor refused. */
export interface SyncOutboxEntry {
  change: SyncProposalChange;
  /** Device the change names, when it names one. */
  target: SyncDeviceRef | null;
  createdAtMs: number;
  state: SyncOutboxState;
  /** Set only on a rejected proposal. */
  rejection: SyncProposalRejection | null;
}

export const SYNC_LATE_KINDS = ["values", "tombstone", "created"] as const;
export type SyncLateKind = (typeof SYNC_LATE_KINDS)[number];

/** A row a removed device changed after its removal, waiting for an admin to decide. */
export interface SyncLateChange {
  removalRevision: number;
  /** The removed device. */
  device: SyncDeviceRef;
  table: string;
  rowKey: string;
  kind: SyncLateKind;
  title: string;
  preview: string;
  /** When the newest late value was written; null for a deletion. */
  changedAtMs: number | null;
  /** Whether this device already proposed a decision the anchor has not sequenced yet. */
  decisionPending: boolean;
}

/** The first late changes the native side lists, with how many wait in all. */
export interface SyncLateChanges {
  changes: SyncLateChange[];
  /** Every late change waiting for a decision, including those past the listed bound. */
  total: number;
}

export interface SyncLateChangeTarget {
  removalRevision: number;
  table: string;
  rowKey: string;
}

/** An authorization entry by hexadecimal hash and revision. */
export interface SyncContestEntry {
  hash: string;
  revision: number;
}

export const SYNC_CONTEST_CAUSES = ["contested", "writerFork", "taken"] as const;
/**
 * Why an instance is contested: two handovers leave it or an objection stands, a writer of its
 * anchor forked, or a forced take won it alone and its anchor, this device, may still object.
 */
export type SyncContestCause = (typeof SYNC_CONTEST_CAUSES)[number];

export const SYNC_CANDIDATE_KINDS = ["planned", "forced", "selfHandover", "kit", "installedKey"] as const;
export type SyncCandidateKind = (typeof SYNC_CANDIDATE_KINDS)[number];

/** A handover out of a contested instance. */
export interface SyncContestCandidate {
  hash: string;
  parent: SyncContestEntry;
  kind: SyncCandidateKind;
  /** The device it makes the anchor. */
  target: SyncDeviceRef;
  /** The device whose admin key signed it, null for a recovery key. */
  signer: SyncDeviceRef | null;
  /** Revision that enrolled the target on the candidate's chain. */
  targetEnrolledRevision: number | null;
  /** The device that enrolled the target, when an admin device did. */
  targetEnroller: SyncDeviceRef | null;
  /** Hash of the candidate its signer yielded to. */
  withdrawnInFavorOf: string | null;
  /** Whether it makes this device the anchor. */
  local: boolean;
}

/** An objection of the instance's anchor to a candidate. */
export interface SyncContestObjection {
  hash: string;
  /** Hash of the candidate it objects to. */
  against: string;
  objector: SyncDeviceRef | null;
  withdrawn: boolean;
}

/** A stored yield to a candidate. */
export interface SyncContestYield {
  hash: string;
  author: SyncDeviceRef | null;
  /** Hash of the candidate it names. */
  to: string;
}

/** This device's part in a contest. */
export type SyncContestRole =
  | { kind: "contestant"; candidate: string }
  | { kind: "anchor" }
  | { kind: "bystander" };

/** Where a kit point lies: the selected head, under a candidate, or past the freeze point. */
export type SyncKitPlace =
  | { kind: "head" }
  | { kind: "candidate"; target: SyncDeviceRef }
  | { kind: "continuation"; anchor: SyncDeviceRef };

/** An entry a kit handover to this desktop may continue from. */
export interface SyncKitPoint {
  entry: SyncContestEntry;
  epoch: number;
  place: SyncKitPlace;
}

/** What this device may do in a contest. */
export type SyncContestAction =
  | { kind: "yieldTo"; candidate: string }
  | { kind: "withdrawObjection"; candidate: string }
  | { kind: "object"; candidate: string }
  | { kind: "useKit"; points: SyncKitPoint[] };

/** A contested instance of the authorization log. */
export interface SyncContest {
  /** Hash of the entry that opened the instance, which identifies it. */
  rootHash: string;
  epoch: number;
  anchor: SyncDeviceRef;
  /** Where the instance froze, null for a lone take this device may object to. */
  frozenAt: SyncContestEntry | null;
  cause: SyncContestCause;
  candidates: SyncContestCandidate[];
  objections: SyncContestObjection[];
  yields: SyncContestYield[];
  /** Devices whose operations the contest holds back. */
  held: SyncDeviceRef[];
  localRole: SyncContestRole;
  actions: SyncContestAction[];
}

/** A device a stored entry that is not effective removes, listed in the kit's device review. */
export interface SyncLosingRemoval {
  device: SyncDeviceRef;
  removedBy: SyncDeviceRef | null;
  revision: number;
}

export const SYNC_BACKSTOP_STATES = ["reserve", "stopped"] as const;
/** What the storage backstop still stores: only kit and contest records, or nothing. */
export type SyncBackstopState = (typeof SYNC_BACKSTOP_STATES)[number];

/** A growth alert of the authorization record store. */
export type SyncStorageAlert =
  | { kind: "device"; device: SyncDeviceRef; bytes: number }
  | { kind: "total"; bytes: number }
  | { kind: "forkedWriter"; writer: string; device: SyncDeviceRef | null };

/** The record store's storage state. */
export interface SyncRecordStorage {
  /** Set once the storage backstop started; full sync resumes only when the user resumes it. */
  backstop: SyncBackstopState | null;
  alerts: SyncStorageAlert[];
}

/** The contests of the vault's sync space and its record storage state. */
export interface SyncContests {
  contests: SyncContest[];
  losingRemovals: SyncLosingRemoval[];
  storage: SyncRecordStorage;
}

/** A recovery kit this desktop holds until the user confirms saving it. */
export interface SyncPendingRecoveryKit {
  /** The grouped kit code with its checksum. */
  code: string;
  /** The kit code as a QR module matrix, for the printable page. */
  qr: PairingQrMatrix;
  /**
   * Whether the kit protects the vault: the vault's sync space names it, or, before the vault
   * links, the vault keeps it for its first charter. A replacement kit waits for the anchor.
   */
  active: boolean;
}

/** The recovery kit of the active vault. */
export interface SyncRecoveryKitState {
  /** Whether the vault keeps a recovery kit's public keys. */
  stored: boolean;
  /** The kit this desktop holds until the user confirms saving it, or null. */
  pending: SyncPendingRecoveryKit | null;
}

function readConnection(value: unknown, label: string): SyncConnectionView {
  const row = readRecord(value, label);
  return {
    network: readEnum(row.network, SYNC_CONNECTION_NETWORKS, `${label}.network`),
    address: readString(row.address, `${label}.address`),
  };
}

function readDegradedReasons(value: unknown, label: string): SyncDegradedReason[] {
  const reasons = readArray(value, label, (reason, entryLabel) => readEnum(reason, SYNC_DEGRADED_REASONS, entryLabel));
  if (new Set(reasons).size !== reasons.length) throw new Error(`${label} must not repeat a reason`);
  return reasons;
}

function readHeld(value: unknown, label: string): SyncHeldCounts {
  const row = readRecord(value, label);
  return {
    newerFormat: readNonNegativeSafeInteger(row.newerFormat, `${label}.newerFormat`),
    newerManifest: readNonNegativeSafeInteger(row.newerManifest, `${label}.newerManifest`),
    invalid: readNonNegativeSafeInteger(row.invalid, `${label}.invalid`),
  };
}

function readTableCount(value: unknown, label: string): SyncTableCount {
  const row = readRecord(value, label);
  return {
    table: readString(row.table, `${label}.table`),
    count: readNonNegativeSafeInteger(row.count, `${label}.count`),
  };
}

function readRelayDelivery(value: unknown, label: string): SyncRelayDelivery {
  const row = readRecord(value, label);
  return {
    lastPassAtMs: readNonNegativeSafeInteger(row.lastPassAtMs, `${label}.lastPassAtMs`),
    sent: readNonNegativeSafeInteger(row.sent, `${label}.sent`),
    received: readNonNegativeSafeInteger(row.received, `${label}.received`),
    undelivered: readNonNegativeSafeInteger(row.undelivered, `${label}.undelivered`),
    uploadedFiles: readNonNegativeSafeInteger(row.uploadedFiles, `${label}.uploadedFiles`),
    downloadedFiles: readNonNegativeSafeInteger(row.downloadedFiles, `${label}.downloadedFiles`),
    deferredFiles: readNonNegativeSafeInteger(row.deferredFiles, `${label}.deferredFiles`),
    storedBytes: readNonNegativeSafeInteger(row.storedBytes, `${label}.storedBytes`),
    quotaBytes: readNonNegativeSafeInteger(row.quotaBytes, `${label}.quotaBytes`),
    problem: readNullable(row.problem, `${label}.problem`, readString),
  };
}

function readUncountedDevice(value: unknown, label: string): SyncUncountedDevice {
  const row = readRecord(value, label);
  return {
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    standing: readEnum(row.standing, SYNC_DEVICE_STANDINGS, `${label}.standing`),
  };
}

/** Validates a sync status from the native side. */
export function mapSyncStatus(value: unknown): SyncStatusView {
  const row = readRecord(value, "sync status");
  return {
    state: readEnum(row.state, SYNC_STATES, "sync status.state"),
    role: readNullable(row.role, "sync status.role", (role, label) => readEnum(role, SYNC_ROLES, label)),
    paused: readBoolean(row.paused, "sync status.paused"),
    lastExchangeAtMs: readNullable(row.lastExchangeAtMs, "sync status.lastExchangeAtMs", readNonNegativeSafeInteger),
    lastError: readNullable(row.lastError, "sync status.lastError", readString),
    lastErrorCode: readNullable(row.lastErrorCode, "sync status.lastErrorCode", (code, label) =>
      readEnum(code, SYNC_ERROR_CODES, label),
    ),
    pendingLocalChanges: readNonNegativeSafeInteger(row.pendingLocalChanges, "sync status.pendingLocalChanges"),
    waiting: readNonNegativeSafeInteger(row.waiting, "sync status.waiting"),
    held: readHeld(row.held, "sync status.held"),
    conflicts: readArray(row.conflicts, "sync status.conflicts", readTableCount),
    recovery: readArray(row.recovery, "sync status.recovery", readTableCount),
    clockAheadDevices: readArray(row.clockAheadDevices, "sync status.clockAheadDevices", mapSyncDeviceRef),
    uncountedDevices: readArray(row.uncountedDevices, "sync status.uncountedDevices", readUncountedDevice),
    connection: readNullable(row.connection, "sync status.connection", readConnection),
    degraded: readDegradedReasons(row.degraded, "sync status.degraded"),
    unconfirmed: readNonNegativeSafeInteger(row.unconfirmed, "sync status.unconfirmed"),
    oldestUnconfirmedAtMs: readNullable(
      row.oldestUnconfirmedAtMs,
      "sync status.oldestUnconfirmedAtMs",
      readNonNegativeSafeInteger,
    ),
    displacedVersions: readNonNegativeSafeInteger(row.displacedVersions, "sync status.displacedVersions"),
    anchorUnreachableSinceMs: readNullable(
      row.anchorUnreachableSinceMs,
      "sync status.anchorUnreachableSinceMs",
      readNonNegativeSafeInteger,
    ),
    relay: readNullable(row.relay, "sync status.relay", readRelayDelivery),
  };
}

function readAuthorityDevice(value: unknown, label: string): SyncAuthorityDevice {
  const row = readRecord(value, label);
  return {
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    kind: readEnum(row.kind, SYNC_DEVICE_KINDS, `${label}.kind`),
    role: readEnum(row.role, SYNC_AUTHORITY_ROLES, `${label}.role`),
    enrolledRevision: readNonNegativeSafeInteger(row.enrolledRevision, `${label}.enrolledRevision`),
    removedRevision: readNullable(row.removedRevision, `${label}.removedRevision`, readNonNegativeSafeInteger),
    quarantined: readBoolean(row.quarantined, `${label}.quarantined`),
    anchor: readBoolean(row.anchor, `${label}.anchor`),
  };
}

/** Validates the authorization state of the vault's sync space from the native side. */
export function mapSyncAuthority(value: unknown): SyncAuthorityView {
  const row = readRecord(value, "sync authority");
  const view: SyncAuthorityView = {
    anchorDeviceId: readString(row.anchorDeviceId, "sync authority.anchorDeviceId"),
    epoch: readNonNegativeSafeInteger(row.epoch, "sync authority.epoch"),
    revision: readNonNegativeSafeInteger(row.revision, "sync authority.revision"),
    frozen: readBoolean(row.frozen, "sync authority.frozen"),
    ownIsAdmin: readBoolean(row.ownIsAdmin, "sync authority.ownIsAdmin"),
    devices: readArray(row.devices, "sync authority.devices", readAuthorityDevice),
  };
  const ids = view.devices.map((entry) => entry.device.deviceId);
  if (new Set(ids).size !== ids.length) throw new Error("sync authority.devices must not repeat a device");
  if (view.devices.filter((entry) => entry.anchor).length > 1) {
    throw new Error("sync authority.devices must have at most one anchor");
  }
  return view;
}

/** Validates an outbox entry from the native side. */
export function mapSyncOutboxEntry(value: unknown, label: string): SyncOutboxEntry {
  const row = readRecord(value, label);
  const state = readEnum(row.state, SYNC_OUTBOX_STATES, `${label}.state`);
  const rejection = readNullable(row.rejection, `${label}.rejection`, (reason, reasonLabel) =>
    readEnum(reason, SYNC_PROPOSAL_REJECTIONS, reasonLabel),
  );
  if ((state === "rejected") !== (rejection !== null)) {
    throw new Error(`${label}.rejection must be set exactly when the proposal was rejected`);
  }
  return {
    change: readEnum(row.change, SYNC_PROPOSAL_CHANGES, `${label}.change`),
    target: readNullable(row.target, `${label}.target`, mapSyncDeviceRef),
    createdAtMs: readNonNegativeSafeInteger(row.createdAtMs, `${label}.createdAtMs`),
    state,
    rejection,
  };
}

/** Validates a late change from the native side. */
export function mapSyncLateChange(value: unknown, label: string): SyncLateChange {
  const row = readRecord(value, label);
  return {
    removalRevision: readNonNegativeSafeInteger(row.removalRevision, `${label}.removalRevision`),
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    table: readString(row.table, `${label}.table`),
    rowKey: readString(row.rowKey, `${label}.rowKey`),
    kind: readEnum(row.kind, SYNC_LATE_KINDS, `${label}.kind`),
    title: readString(row.title, `${label}.title`),
    preview: readString(row.preview, `${label}.preview`),
    changedAtMs: readNullable(row.changedAtMs, `${label}.changedAtMs`, readNonNegativeSafeInteger),
    decisionPending: readBoolean(row.decisionPending, `${label}.decisionPending`),
  };
}

/** Validates a pending recovery kit from the native side. */
export function mapSyncPendingRecoveryKit(value: unknown, label: string): SyncPendingRecoveryKit {
  const row = readRecord(value, label);
  const code = readString(row.code, `${label}.code`);
  if (code.trim().length === 0) throw new Error(`${label}.code must not be empty`);
  return { code, qr: parseQrMatrix(row.qr), active: readBoolean(row.active, `${label}.active`) };
}

/** Validates the recovery kit state from the native side. */
export function mapSyncRecoveryKitState(value: unknown): SyncRecoveryKitState {
  const label = "recovery kit state";
  const row = readRecord(value, label);
  return {
    stored: readBoolean(row.stored, `${label}.stored`),
    pending: readNullable(row.pending, `${label}.pending`, mapSyncPendingRecoveryKit),
  };
}

/** A 32-byte hash in lowercase hexadecimal, as the native side writes entry hashes. */
const SYNC_HASH_PATTERN = /^[0-9a-f]{64}$/;
/** A 16-byte writer id in lowercase hexadecimal. */
const SYNC_WRITER_PATTERN = /^[0-9a-f]{32}$/;

function readPattern(value: unknown, label: string, pattern: RegExp): string {
  const text = readString(value, label);
  if (!pattern.test(text)) throw new Error(`${label} must be lowercase hexadecimal of the expected length`);
  return text;
}

function readHash(value: unknown, label: string): string {
  return readPattern(value, label, SYNC_HASH_PATTERN);
}

function readContestEntry(value: unknown, label: string): SyncContestEntry {
  const row = readRecord(value, label);
  return {
    hash: readHash(row.hash, `${label}.hash`),
    revision: readNonNegativeSafeInteger(row.revision, `${label}.revision`),
  };
}

function readContestCandidate(value: unknown, label: string): SyncContestCandidate {
  const row = readRecord(value, label);
  return {
    hash: readHash(row.hash, `${label}.hash`),
    parent: readContestEntry(row.parent, `${label}.parent`),
    kind: readEnum(row.kind, SYNC_CANDIDATE_KINDS, `${label}.kind`),
    target: mapSyncDeviceRef(row.target, `${label}.target`),
    signer: readNullable(row.signer, `${label}.signer`, mapSyncDeviceRef),
    targetEnrolledRevision: readNullable(
      row.targetEnrolledRevision,
      `${label}.targetEnrolledRevision`,
      readNonNegativeSafeInteger,
    ),
    targetEnroller: readNullable(row.targetEnroller, `${label}.targetEnroller`, mapSyncDeviceRef),
    withdrawnInFavorOf: readNullable(row.withdrawnInFavorOf, `${label}.withdrawnInFavorOf`, readHash),
    local: readBoolean(row.local, `${label}.local`),
  };
}

function readContestObjection(value: unknown, label: string): SyncContestObjection {
  const row = readRecord(value, label);
  return {
    hash: readHash(row.hash, `${label}.hash`),
    against: readHash(row.against, `${label}.against`),
    objector: readNullable(row.objector, `${label}.objector`, mapSyncDeviceRef),
    withdrawn: readBoolean(row.withdrawn, `${label}.withdrawn`),
  };
}

function readContestYield(value: unknown, label: string): SyncContestYield {
  const row = readRecord(value, label);
  return {
    hash: readHash(row.hash, `${label}.hash`),
    author: readNullable(row.author, `${label}.author`, mapSyncDeviceRef),
    to: readHash(row.to, `${label}.to`),
  };
}

function readContestRole(value: unknown, label: string): SyncContestRole {
  const row = readRecord(value, label);
  const kind = readEnum(row.kind, ["contestant", "anchor", "bystander"] as const, `${label}.kind`);
  if (kind === "contestant") return { kind, candidate: readHash(row.candidate, `${label}.candidate`) };
  return { kind };
}

function readKitPlace(value: unknown, label: string): SyncKitPlace {
  const row = readRecord(value, label);
  const kind = readEnum(row.kind, ["head", "candidate", "continuation"] as const, `${label}.kind`);
  switch (kind) {
    case "head": return { kind };
    case "candidate": return { kind, target: mapSyncDeviceRef(row.target, `${label}.target`) };
    case "continuation": return { kind, anchor: mapSyncDeviceRef(row.anchor, `${label}.anchor`) };
  }
}

function readKitPoint(value: unknown, label: string): SyncKitPoint {
  const row = readRecord(value, label);
  return {
    entry: readContestEntry(row.entry, `${label}.entry`),
    epoch: readNonNegativeSafeInteger(row.epoch, `${label}.epoch`),
    place: readKitPlace(row.place, `${label}.place`),
  };
}

function readContestAction(value: unknown, label: string): SyncContestAction {
  const row = readRecord(value, label);
  const kind = readEnum(row.kind, ["yieldTo", "withdrawObjection", "object", "useKit"] as const, `${label}.kind`);
  if (kind === "useKit") {
    const points = readArray(row.points, `${label}.points`, readKitPoint);
    if (points.length === 0) throw new Error(`${label}.points must name at least one point`);
    return { kind, points };
  }
  return { kind, candidate: readHash(row.candidate, `${label}.candidate`) };
}

function readContest(value: unknown, label: string): SyncContest {
  const row = readRecord(value, label);
  const contest: SyncContest = {
    rootHash: readHash(row.rootHash, `${label}.rootHash`),
    epoch: readNonNegativeSafeInteger(row.epoch, `${label}.epoch`),
    anchor: mapSyncDeviceRef(row.anchor, `${label}.anchor`),
    frozenAt: readNullable(row.frozenAt, `${label}.frozenAt`, readContestEntry),
    cause: readEnum(row.cause, SYNC_CONTEST_CAUSES, `${label}.cause`),
    candidates: readArray(row.candidates, `${label}.candidates`, readContestCandidate),
    objections: readArray(row.objections, `${label}.objections`, readContestObjection),
    yields: readArray(row.yields, `${label}.yields`, readContestYield),
    held: readArray(row.held, `${label}.held`, mapSyncDeviceRef),
    localRole: readContestRole(row.localRole, `${label}.localRole`),
    actions: readArray(row.actions, `${label}.actions`, readContestAction),
  };
  if ((contest.cause === "taken") !== (contest.frozenAt === null)) {
    throw new Error(`${label}.frozenAt must be absent exactly for a lone take`);
  }
  const candidates = new Set(contest.candidates.map((candidate) => candidate.hash));
  if (candidates.size !== contest.candidates.length) throw new Error(`${label}.candidates must not repeat`);
  for (const action of contest.actions) {
    if (action.kind !== "useKit" && !candidates.has(action.candidate)) {
      throw new Error(`${label}.actions must name candidates of the contest`);
    }
  }
  return contest;
}

function readLosingRemoval(value: unknown, label: string): SyncLosingRemoval {
  const row = readRecord(value, label);
  return {
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    removedBy: readNullable(row.removedBy, `${label}.removedBy`, mapSyncDeviceRef),
    revision: readNonNegativeSafeInteger(row.revision, `${label}.revision`),
  };
}

function readStorageAlert(value: unknown, label: string): SyncStorageAlert {
  const row = readRecord(value, label);
  const kind = readEnum(row.kind, ["device", "total", "forkedWriter"] as const, `${label}.kind`);
  switch (kind) {
    case "device":
      return {
        kind,
        device: mapSyncDeviceRef(row.device, `${label}.device`),
        bytes: readNonNegativeSafeInteger(row.bytes, `${label}.bytes`),
      };
    case "total": return { kind, bytes: readNonNegativeSafeInteger(row.bytes, `${label}.bytes`) };
    case "forkedWriter":
      return {
        kind,
        writer: readPattern(row.writer, `${label}.writer`, SYNC_WRITER_PATTERN),
        device: readNullable(row.device, `${label}.device`, mapSyncDeviceRef),
      };
  }
}

/** Validates the contests and record storage state of the vault's sync space from the native side. */
export function mapSyncContests(value: unknown): SyncContests {
  const row = readRecord(value, "sync contests");
  const storage = readRecord(row.storage, "sync contests.storage");
  return {
    contests: readArray(row.contests, "sync contests.contests", readContest),
    losingRemovals: readArray(row.losingRemovals, "sync contests.losingRemovals", readLosingRemoval),
    storage: {
      backstop: readNullable(storage.backstop, "sync contests.storage.backstop", (state, label) =>
        readEnum(state, SYNC_BACKSTOP_STATES, label),
      ),
      alerts: readArray(storage.alerts, "sync contests.storage.alerts", readStorageAlert),
    },
  };
}

/** Validates a device reference from the native side. */
export function mapSyncDeviceRef(value: unknown, label: string): SyncDeviceRef {
  const row = readRecord(value, label);
  return {
    deviceId: readString(row.deviceId, `${label}.deviceId`),
    deviceLabel: readNullable(row.deviceLabel, `${label}.deviceLabel`, readString),
    ownDevice: readBoolean(row.ownDevice, `${label}.ownDevice`),
  };
}

function readRecoveryEntry(value: unknown, label: string): SyncRecoveryEntry {
  const row = readRecord(value, label);
  return {
    table: readString(row.table, `${label}.table`),
    rowKey: readString(row.rowKey, `${label}.rowKey`),
    title: readString(row.title, `${label}.title`),
    preview: readString(row.preview, `${label}.preview`),
    descendants: readNonNegativeSafeInteger(row.descendants, `${label}.descendants`),
    unseen: readArray(row.unseen, `${label}.unseen`, readRecoveryText),
    unseenOmitted: readNonNegativeSafeInteger(row.unseenOmitted, `${label}.unseenOmitted`),
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    editedAtMs: readNonNegativeSafeInteger(row.editedAtMs, `${label}.editedAtMs`),
  };
}

function readRecoveryText(value: unknown, label: string): SyncRecoveryText {
  const row = readRecord(value, label);
  return {
    title: readString(row.title, `${label}.title`),
    preview: readString(row.preview, `${label}.preview`),
  };
}

/** A notice the sync service sends to subscribed WebViews. */
export type SyncNotice = { kind: "status"; status: SyncStatusView } | { kind: "applied"; tables: string[] };

const SYNC_NOTICE_KINDS = ["status", "applied"] as const;

/** Validates a notice from the sync subscription channel. */
export function mapSyncNotice(value: unknown): SyncNotice {
  const row = readRecord(value, "sync notice");
  const kind = readEnum(row.kind, SYNC_NOTICE_KINDS, "sync notice.kind");
  if (kind === "status") return { kind, status: mapSyncStatus(row.status) };
  return { kind, tables: readArray(row.tables, "sync notice.tables", readString) };
}

export async function getSyncStatus(): Promise<SyncStatusView> {
  return mapSyncStatus(await invoke<unknown>("sync_status"));
}

/** Asks the service to exchange now instead of waiting for its next pass. */
export async function syncNow(): Promise<void> {
  await invoke("sync_now");
}

/**
 * Tells the phone's sync service whether the app is visible. A phone serves other linked
 * members only while it is in the foreground; hiding the app stops that listener.
 * @param visible Whether the app is in the foreground.
 */
export async function setSyncForeground(visible: boolean): Promise<void> {
  await invoke("sync_set_foreground", { visible });
}

export async function setSyncPaused(paused: boolean): Promise<void> {
  await invoke("sync_set_paused", { paused });
}

export async function listSyncRecovery(): Promise<SyncRecoveryEntry[]> {
  return readArray(await invoke<unknown>("sync_recovery_list"), "sync recovery", readRecoveryEntry);
}

/** Restores a recovery offer as a new row and returns its id. */
export async function restoreSyncRecovery(target: SyncRecoveryTarget): Promise<string | null> {
  return readNullable(await invoke<unknown>("sync_recovery_restore", { target }), "restored id", readString);
}

export async function discardSyncRecovery(target: SyncRecoveryTarget): Promise<void> {
  await invoke("sync_recovery_discard", { target });
}

/** The authorization state of the active vault's sync space, or null before it joined one. */
export async function getSyncAuthority(): Promise<SyncAuthorityView | null> {
  return readNullable(await invoke<unknown>("sync_authority"), "sync authority", (value) => mapSyncAuthority(value));
}

/** Proposals this device made that wait for the anchor or that it refused, newest first. */
export async function listSyncOutbox(): Promise<SyncOutboxEntry[]> {
  return readArray(await invoke<unknown>("sync_outbox_list"), "sync outbox", mapSyncOutboxEntry);
}

/** Validates the bounded late change list from the native side. */
export function mapSyncLateChanges(value: unknown): SyncLateChanges {
  const row = readRecord(value, "sync late changes");
  const changes = readArray(row.changes, "sync late changes.changes", mapSyncLateChange);
  const total = readNonNegativeSafeInteger(row.total, "sync late changes.total");
  if (total < changes.length) throw new Error("sync late changes.total is smaller than the listed changes");
  return { changes, total };
}

/** The first rows removed devices changed after their removal that no admin decided yet, with their total. */
export async function listSyncLateChanges(): Promise<SyncLateChanges> {
  return mapSyncLateChanges(await invoke<unknown>("sync_late_changes"));
}

/** On an admin desktop, keeps a late change and proposes the decision to the anchor. */
export async function restoreSyncLateChange(target: SyncLateChangeTarget): Promise<void> {
  await invoke("sync_late_change_restore", { target });
}

/** On an admin desktop, proposes discarding a late change for good. */
export async function discardSyncLateChange(target: SyncLateChangeTarget): Promise<void> {
  await invoke("sync_late_change_discard", { target });
}

/** On an admin desktop, proposes removing another device from the vault. */
export async function removeSyncDevice(deviceId: string): Promise<void> {
  await invoke("sync_remove_device", { deviceId });
}

/** Stops accepting a device's connections and changes on this installation at once. */
export async function quarantineSyncDevice(deviceId: string): Promise<void> {
  await invoke("sync_quarantine_device", { deviceId });
}

/** Lifts a quarantine on this installation. */
export async function liftSyncQuarantine(deviceId: string): Promise<void> {
  await invoke("sync_quarantine_lift", { deviceId });
}

/** On a linked admin desktop, takes the anchor in a planned handover and returns its revision. */
export async function takeSyncAnchor(): Promise<number> {
  return readNonNegativeSafeInteger(await invoke<unknown>("sync_take_anchor"), "handover revision");
}

/** On the coordinating desktop, proposes making a linked desktop an admin. */
export async function promoteSyncDevice(deviceId: string): Promise<void> {
  await invoke("sync_promote_device", { deviceId });
}

/**
 * Replaces the recovery kit from the anchor desktop. The new kit then waits to be saved, as
 * {@link getSyncRecoveryKitState} shows.
 */
export async function regenerateSyncRecoveryKit(): Promise<void> {
  await invoke("sync_regenerate_recovery_kit");
}

/** The active vault's recovery kit, with the kit this desktop holds until the user saves it. */
export async function getSyncRecoveryKitState(): Promise<SyncRecoveryKitState> {
  return mapSyncRecoveryKitState(await invoke<unknown>("sync_recovery_kit_state"));
}

/** Creates or replaces the recovery kit of a desktop vault that is not linked yet. */
export async function createSyncRecoveryKit(): Promise<void> {
  await invoke("sync_create_recovery_kit");
}

/**
 * Confirms the user saved the waiting kit by sending the last group of its code as typed. When
 * it matches, this desktop deletes the recovery seed and the call returns true.
 */
export async function confirmSyncRecoveryKitSaved(lastGroup: string): Promise<boolean> {
  return readBoolean(await invoke<unknown>("sync_recovery_kit_saved", { lastGroup }), "recovery kit confirmation");
}

/** What a forced anchor handover signs with and whether it also quarantines the lost anchor. */
export interface SyncForceAnchorRequest {
  /** A recovery kit code, needed only when this desktop is not an admin yet. */
  kitCode: string | null;
  /** With a kit code, the kit point to continue from; null for the default one. */
  point: SyncContestEntry | null;
  /** With a kit code, devices the device review marked, which the kit key removes after the handover. */
  removeDevices: string[];
  /** Whether to quarantine the old anchor here and propose removing it. */
  quarantineAnchor: boolean;
}

/**
 * On a linked desktop whose anchor stopped answering or whose vault is contested, makes this
 * desktop the anchor without it, signed with its admin key or, with a kit code, with the
 * recovery key, which also makes it an admin. Returns the revision of the last entry it sealed.
 */
export async function forceSyncAnchor(request: SyncForceAnchorRequest): Promise<number> {
  return readNonNegativeSafeInteger(
    await invoke<unknown>("sync_force_anchor", {
      kitCode: request.kitCode,
      point: request.point,
      removeDevices: request.removeDevices,
      quarantineAnchor: request.quarantineAnchor,
    }),
    "forced handover revision",
  );
}

/** The contests and record storage state of the active vault's sync space, or null before it joined one. */
export async function getSyncContests(): Promise<SyncContests | null> {
  return readNullable(await invoke<unknown>("sync_contests"), "sync contests", (value) => mapSyncContests(value));
}

/** Yields to the candidate handover `handover`, or withdraws this anchor's objection to it. */
export async function yieldSyncContest(handover: string): Promise<number> {
  return readNonNegativeSafeInteger(
    await invoke<unknown>("sync_contest_yield", { handover }),
    "contest yield revision",
  );
}

/** As the anchor of the contested instance, blocks the candidate handover `handover`. */
export async function objectSyncContest(handover: string): Promise<number> {
  return readNonNegativeSafeInteger(
    await invoke<unknown>("sync_contest_object", { handover }),
    "contest objection revision",
  );
}

/** Resumes full record exchange after the storage backstop stopped it. */
export async function resumeSyncRecords(): Promise<void> {
  await invoke("sync_records_resume");
}

/** Proposes making this enrolled desktop an admin with a recovery kit code. */
export async function useSyncRecoveryKit(code: string): Promise<void> {
  await invoke("sync_use_recovery_kit", { code });
}

/** On the coordinating desktop, unlinks every device and starts sync over from current data. */
export async function resetSyncState(): Promise<void> {
  await invoke("sync_reset_state");
}

/**
 * Asks where to save the recovery kit as a text file and writes it.
 *
 * @returns Whether the user picked a file.
 */
export async function saveSyncRecoveryKitFile(defaultName: string, contents: string): Promise<boolean> {
  return readBoolean(
    await invoke<unknown>("vault_pick_and_write_recovery_kit", { defaultName, contents }),
    "recovery kit saved",
  );
}

type SyncNoticeListener = (notice: SyncNotice) => void;

const noticeListeners = new Set<SyncNoticeListener>();
let subscription: Promise<string> | null = null;

function deliver(value: unknown): void {
  let notice: SyncNotice;
  try {
    notice = mapSyncNotice(value);
  } catch (error: unknown) {
    console.warn("Ignored an invalid sync notice", error);
    return;
  }
  for (const listener of [...noticeListeners]) {
    try {
      listener(notice);
    } catch (error: unknown) {
      console.warn("A sync notice listener failed", error);
    }
  }
}

async function openSubscription(): Promise<string> {
  const subscriptionId = crypto.randomUUID();
  const channel = new Channel<unknown>();
  channel.onmessage = deliver;
  await invoke("sync_subscribe", { subscriptionId, channel });
  return subscriptionId;
}

/**
 * Shares one native subscription per WebView among every notice listener.
 *
 * @returns A function that removes `listener` and ends the subscription after the last one.
 */
async function listenSyncNotices(listener: SyncNoticeListener): Promise<() => void> {
  noticeListeners.add(listener);
  subscription ??= openSubscription();
  const current = subscription;
  try {
    await current;
  } catch (error: unknown) {
    noticeListeners.delete(listener);
    if (subscription === current) subscription = null;
    throw error;
  }
  let active = true;
  return () => {
    if (!active) return;
    active = false;
    noticeListeners.delete(listener);
    if (noticeListeners.size > 0 || subscription !== current) return;
    subscription = null;
    void current
      .then((subscriptionId) => invoke("sync_unsubscribe", { subscriptionId }))
      .catch((error: unknown) => console.warn("sync notices could not be unsubscribed", error));
  };
}

/** Runs `onStatus` with every valid status the service sends. */
export function listenSyncStatus(onStatus: (status: SyncStatusView) => void): Promise<() => void> {
  return listenSyncNotices((notice) => {
    if (notice.kind === "status") onStatus(notice.status);
  });
}

/** Runs `onApplied` with the tables that replicated changes touched. */
export function listenSyncApplied(onApplied: (tables: string[]) => void): Promise<() => void> {
  return listenSyncNotices((notice) => {
    if (notice.kind === "applied") onApplied(notice.tables);
  });
}
