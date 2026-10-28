import { Channel, invoke } from "@tauri-apps/api/core";
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
export const SYNC_ERROR_CODES = ["writerLimit", "bootstrapRequired"] as const;
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
  /** Device of the newest unseen edit. */
  device: SyncDeviceRef;
  editedAtMs: number;
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

export interface SyncLateChangeTarget {
  removalRevision: number;
  table: string;
  rowKey: string;
}

/** A recovery kit this desktop holds until the user confirms saving it. */
export interface SyncPendingRecoveryKit {
  /** The grouped kit code with its checksum. */
  code: string;
  /** Whether the vault already trusts the kit; a regenerated kit waits for the anchor. */
  active: boolean;
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
  return { code, active: readBoolean(row.active, `${label}.active`) };
}

function readKitCode(value: unknown, label: string): string {
  const code = readString(value, label);
  if (code.trim().length === 0) throw new Error(`${label} must not be empty`);
  return code;
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
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    editedAtMs: readNonNegativeSafeInteger(row.editedAtMs, `${label}.editedAtMs`),
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

/** Rows removed devices changed after their removal that no admin decided yet. */
export async function listSyncLateChanges(): Promise<SyncLateChange[]> {
  return readArray(await invoke<unknown>("sync_late_changes"), "sync late changes", mapSyncLateChange);
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

/** Replaces the recovery kit on an admin desktop and returns the new code, shown once. */
export async function regenerateSyncRecoveryKit(): Promise<string> {
  return readKitCode(await invoke<unknown>("sync_regenerate_recovery_kit"), "recovery kit code");
}

/** The recovery kit this admin desktop holds until the user confirms saving it, or null. */
export async function getPendingSyncRecoveryKit(): Promise<SyncPendingRecoveryKit | null> {
  return readNullable(
    await invoke<unknown>("sync_recovery_kit_pending"),
    "pending recovery kit",
    mapSyncPendingRecoveryKit,
  );
}

/** Deletes the recovery seed from this desktop after the user saved the kit. */
export async function confirmSyncRecoveryKitSaved(): Promise<void> {
  await invoke("sync_recovery_kit_saved");
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
