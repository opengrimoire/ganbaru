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

/** Stored operations this device cannot apply, by reason. */
export interface SyncHeldCounts {
  newerFormat: number;
  newerManifest: number;
  invalid: number;
}

export interface SyncConflictCount {
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
  /** Local rows with changes that are not sealed yet. */
  pendingLocalChanges: number;
  /** Stored operations waiting to be applied. */
  waiting: number;
  held: SyncHeldCounts;
  conflicts: SyncConflictCount[];
  recoveryCount: number;
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
  pendingLocalChanges: 0,
  waiting: 0,
  held: { newerFormat: 0, newerManifest: 0, invalid: 0 },
  conflicts: [],
  recoveryCount: 0,
};

function readHeld(value: unknown, label: string): SyncHeldCounts {
  const row = readRecord(value, label);
  return {
    newerFormat: readNonNegativeSafeInteger(row.newerFormat, `${label}.newerFormat`),
    newerManifest: readNonNegativeSafeInteger(row.newerManifest, `${label}.newerManifest`),
    invalid: readNonNegativeSafeInteger(row.invalid, `${label}.invalid`),
  };
}

function readConflictCount(value: unknown, label: string): SyncConflictCount {
  const row = readRecord(value, label);
  return {
    table: readString(row.table, `${label}.table`),
    count: readNonNegativeSafeInteger(row.count, `${label}.count`),
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
    pendingLocalChanges: readNonNegativeSafeInteger(row.pendingLocalChanges, "sync status.pendingLocalChanges"),
    waiting: readNonNegativeSafeInteger(row.waiting, "sync status.waiting"),
    held: readHeld(row.held, "sync status.held"),
    conflicts: readArray(row.conflicts, "sync status.conflicts", readConflictCount),
    recoveryCount: readNonNegativeSafeInteger(row.recoveryCount, "sync status.recoveryCount"),
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
