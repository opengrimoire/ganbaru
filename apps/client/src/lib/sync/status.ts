import type { SyncDeviceRef, SyncRole, SyncState, SyncStatusView } from "$lib/api/sync";
import type { AppLocale } from "$lib/i18n/locales";
import { formatDateTime, formatRelativeMinutes } from "$lib/i18n/formatters";
import type { Translate } from "$lib/i18n/translator.svelte";

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

/** Counts rows with open conflicts across all synced tables. */
export function syncConflictTotal(status: SyncStatusView): number {
  return status.conflicts.reduce((total, entry) => total + entry.count, 0);
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

/** Describes this device's place in the sync topology. */
export function syncRoleLabel(t: Translate, role: SyncRole): string {
  return role === "hub" ? t("sync.role.hub") : t("sync.role.client");
}
