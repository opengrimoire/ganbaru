import type {
  SyncDegradedReason,
  SyncDeviceRef,
  SyncRelayProblem,
  SyncRole,
  SyncState,
  SyncStatusView,
} from "$lib/api/sync";
import type { AppLocale } from "$lib/i18n/locales";
import { formatDateTime, formatList, formatNumber, formatRelativeMinutes } from "$lib/i18n/formatters";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { SyncPresentationRegistry } from "./presentation-registry";

const MINUTE_MS = 60_000;
const RELATIVE_LIMIT_MINUTES = 60;

/** States in which a manual sync request has nothing to do. */
const IDLE_REQUEST_STATES: readonly SyncState[] = ["off", "paused", "syncing"];

/**
 * Formats an edit or exchange time: relative within the last hour, absolute after.
 *
 * @param t Translator for relative labels.
 * @param locale Locale for absolute dates.
 * @param atMs Time of the event in Unix milliseconds.
 * @param nowMs Current time in Unix milliseconds.
 */
export function formatSyncTime(t: Translate, locale: AppLocale, atMs: number, nowMs: number): string {
  const minutesAgo = Math.max(0, (nowMs - atMs) / MINUTE_MS);
  if (minutesAgo < RELATIVE_LIMIT_MINUTES) return formatRelativeMinutes(t, -minutesAgo);
  return formatDateTime(locale, atMs, { dateStyle: "medium", timeStyle: "short" });
}

/**
 * Labels the last completed exchange, or that none happened yet.
 */
export function syncLastExchangeLabel(
  t: Translate,
  locale: AppLocale,
  status: SyncStatusView,
  nowMs: number,
): string {
  if (status.lastExchangeAtMs === null) return t("sync.neverExchanged");
  return t("sync.lastExchange", formatSyncTime(t, locale, status.lastExchangeAtMs, nowMs));
}

/**
 * Describes held operations, one line per nonzero reason.
 */
export function syncHeldLines(t: Translate, status: SyncStatusView): string[] {
  const lines: string[] = [];
  if (status.held.newerFormat > 0) lines.push(t("sync.heldNewerFormat", status.held.newerFormat));
  if (status.held.newerManifest > 0) lines.push(t("sync.heldNewerManifest", status.held.newerManifest));
  if (status.held.invalid > 0) lines.push(t("sync.heldInvalid", status.held.invalid));
  return lines;
}

/** Explains what syncs, listing the features of the registered domains. */
export function syncDescription(t: Translate, locale: AppLocale, presentations: SyncPresentationRegistry): string {
  return t("sync.description", formatList(locale, presentations.labels(t), { type: "conjunction" }));
}

/** A conflict line of one replicated table. */
export interface SyncConflictLine {
  table: string;
  line: string;
}

/**
 * Describes open conflicts, one line per table with conflicted rows, in the words of the domain
 * that owns the table.
 */
export function syncConflictLines(
  t: Translate,
  presentations: SyncPresentationRegistry,
  status: SyncStatusView,
): SyncConflictLine[] {
  return status.conflicts
    .filter((entry) => entry.count > 0)
    .map((entry) => ({ table: entry.table, line: presentations.conflictLine(t, entry.table, entry.count) }));
}

/** Counts recovery offers across all synced tables. */
export function syncRecoveryTotal(status: SyncStatusView): number {
  return status.recovery.reduce((total, entry) => total + entry.count, 0);
}

/** Whether a manual sync request can start an exchange. */
export function canRequestSync(status: SyncStatusView): boolean {
  return !IDLE_REQUEST_STATES.includes(status.state);
}

/**
 * Names the device behind a version or edit.
 *
 * @param device The device reference from the native side.
 * @param ownLabel Label for this device.
 * @param unknownLabel Label for a device whose name is unknown here.
 */
export function syncDeviceName(device: SyncDeviceRef, ownLabel: string, unknownLabel: string): string {
  if (device.ownDevice) return ownLabel;
  return device.deviceLabel ?? unknownLabel;
}

/**
 * Describes devices whose edits carry clocks far ahead of this device, without repeated lines.
 * Those edits win concurrent changes until the clocks agree.
 */
export function syncClockAheadLines(t: Translate, status: SyncStatusView): string[] {
  const lines = status.clockAheadDevices.map((device) =>
    device.ownDevice
      ? t("sync.clockAheadOwn")
      : t("sync.clockAhead", syncDeviceName(device, t("sync.recovery.thisDevice"), t("sync.recovery.otherDevice"))),
  );
  return [...new Set(lines)];
}

/**
 * Describes each linked device the coordinator does not count toward stability, keyed by
 * device id: a dormant device that will need a fresh copy of the vault, or one that must be
 * linked again.
 */
export function syncUncountedDeviceLines(t: Translate, status: SyncStatusView): { deviceId: string; line: string }[] {
  return status.uncountedDevices.map(({ device, standing }) => {
    const name = syncDeviceName(device, t("sync.recovery.thisDevice"), t("sync.recovery.otherDevice"));
    const line = standing === "dormant" ? t("sync.uncounted.dormant", name) : t("sync.uncounted.relinkRequired", name);
    return { deviceId: device.deviceId, line };
  });
}

/** Labels the service state. */
export function syncStateLabel(t: Translate, state: SyncState): string {
  switch (state) {
    case "off": return t("sync.state.off");
    case "idle": return t("sync.state.idle");
    case "syncing": return t("sync.state.syncing");
    case "offline": return t("sync.state.offline");
    case "paused": return t("sync.state.paused");
    case "waiting_for_identity": return t("sync.state.waiting_for_identity");
    case "error": return t("sync.state.error");
  }
}

/** States in which the last reached connection still describes the current link. */
const CONNECTED_STATES: readonly SyncState[] = ["idle", "syncing"];

/**
 * Labels how this device reaches its coordinator, or `null` when it is the coordinator or the
 * last reached connection no longer describes the current link.
 */
export function syncConnectionLabel(t: Translate, status: SyncStatusView): string | null {
  if (status.role !== "client" || status.connection === null || !CONNECTED_STATES.includes(status.state)) return null;
  const { network, address } = status.connection;
  return network === "lan" ? t("sync.connection.lan", address) : t("sync.connection.overlay", address);
}

/** A recovery action a degraded reason offers. */
export type SyncDegradedAction = "openBatterySettings" | "allowNetworkAccess" | "allowPeerConnections";

/** Labels the button that runs a degraded reason's recovery action. */
export function syncDegradedActionLabel(t: Translate, action: SyncDegradedAction): string {
  switch (action) {
    case "openBatterySettings":
      return t("sync.degraded.openBatterySettings");
    case "allowNetworkAccess":
      return t("sync.degraded.allowNetworkAccess");
    case "allowPeerConnections":
      return t("sync.degraded.allowPeerConnections");
  }
}

/** One degraded reason with its explanation and recovery action. */
export interface SyncDegradedRow {
  reason: SyncDegradedReason;
  label: string;
  action: SyncDegradedAction | null;
}

/**
 * Explains each degraded reason with its recovery action. The firewall reason has an action only
 * on the coordinator, where this device can allow its listener through the firewall; a member
 * desktop whose own listener the firewall blocks gets its own reason with the same grant.
 */
export function syncDegradedRows(t: Translate, status: SyncStatusView): SyncDegradedRow[] {
  return status.degraded.map((reason) => {
    switch (reason) {
      case "backgroundRestricted":
        return { reason, label: t("sync.degraded.backgroundRestricted"), action: "openBatterySettings" };
      case "standbyRestricted":
        return { reason, label: t("sync.degraded.standbyRestricted"), action: "openBatterySettings" };
      case "localNetworkDenied":
        return { reason, label: t("sync.degraded.localNetworkDenied"), action: null };
      case "firewallAuthorizationRequired":
        return status.role === "hub"
          ? { reason, label: t("sync.degraded.firewallHub"), action: "allowNetworkAccess" }
          : { reason, label: t("sync.degraded.firewallClient"), action: null };
      case "firewallPeerListenerBlocked":
        return { reason, label: t("sync.degraded.firewallPeers"), action: "allowPeerConnections" };
    }
  });
}

/** The unit relay storage is shown in, with one fraction digit. */
const STORAGE_UNIT: Intl.NumberFormatOptions = { style: "unit", unit: "megabyte", maximumFractionDigits: 1 };
const BYTES_PER_MEGABYTE = 1_000_000;

/**
 * Describes what the last relay pass moved and what still waits, one line per nonzero count,
 * with the relay's storage use when it reports a quota. Empty without a relay pass.
 */
export function syncRelayLines(t: Translate, locale: AppLocale, status: SyncStatusView, nowMs: number): string[] {
  const relay = status.relay;
  if (relay === null) return [];
  const lines = [t("sync.relay.lastPass", formatSyncTime(t, locale, relay.lastPassAtMs, nowMs))];
  if (relay.sent > 0) lines.push(t("sync.relay.sent", relay.sent));
  if (relay.received > 0) lines.push(t("sync.relay.received", relay.received));
  if (relay.uploadedFiles > 0) lines.push(t("sync.relay.uploadedFiles", relay.uploadedFiles));
  if (relay.downloadedFiles > 0) lines.push(t("sync.relay.downloadedFiles", relay.downloadedFiles));
  if (relay.deferredFiles > 0) lines.push(t("sync.relay.deferredFiles", relay.deferredFiles));
  if (relay.quotaBytes > 0) {
    const megabytes = (bytes: number) => formatNumber(locale, bytes / BYTES_PER_MEGABYTE, STORAGE_UNIT);
    lines.push(t("sync.relay.storage", megabytes(relay.storedBytes), megabytes(relay.quotaBytes)));
  }
  if (relay.problem !== null) {
    lines.push(t("sync.relay.problem", syncRelayProblemLabel(t, relay.problem)));
    lines.push(
      relay.lastContactAtMs === null
        ? t("sync.relay.noContact")
        : t("sync.relay.lastContact", formatSyncTime(t, locale, relay.lastContactAtMs, nowMs)),
    );
  }
  return lines;
}

/** Explains a typed relay problem in the user's language. */
export function syncRelayProblemLabel(t: Translate, problem: SyncRelayProblem): string {
  return t(`sync.server.problems.${problem}`);
}

/**
 * Describes where this device's own changes stand on the way through the relay: saved here and
 * not on the relay yet, received by the relay and waiting for the anchor, or all confirmed. The
 * relay count is the unconfirmed changes the relay already holds, so it is an upper bound when
 * the anchor has not reported on changes that moved directly. A pass that ended before the
 * relay reported what it holds proves nothing reached it, so every unconfirmed change counts as
 * saved here. Empty without a relay pass.
 */
export function syncDeliveryLines(t: Translate, status: SyncStatusView): string[] {
  const relay = status.relay;
  if (relay === null) return [];
  const lines: string[] = [];
  const undelivered = relay.undelivered ?? status.unconfirmed;
  if (undelivered > 0) lines.push(t("sync.delivery.saved", undelivered));
  const atRelay = Math.max(0, status.unconfirmed - undelivered);
  if (atRelay > 0) lines.push(t("sync.delivery.receivedByRelay", atRelay));
  if (lines.length === 0 && status.unconfirmed === 0 && status.pendingLocalChanges === 0) {
    lines.push(t("sync.delivery.confirmed"));
  }
  return lines;
}

/** Describes this device's place in the sync topology. */
export function syncRoleLabel(t: Translate, role: SyncRole): string {
  return role === "hub" ? t("sync.role.hub") : t("sync.role.client");
}
