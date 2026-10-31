import { mapSyncDeviceRef, type SyncDeviceRef } from "$lib/api/sync";
import type { Translate } from "$lib/i18n/translator.svelte";
import { syncDeviceName } from "$lib/sync/status";
import {
  readArray,
  readEnum,
  readNullable,
  readRecord,
  readSafeInteger,
  readString,
} from "$lib/utils/readers";

/** Activity states another device can report; idle rows never reach the frontend. */
export const FOCUS_ACTIVITY_STATES = ["focus", "short_break", "long_break", "paused"] as const;

export type FocusActivityState = (typeof FOCUS_ACTIVITY_STATES)[number];

/** Sync change topic the Focus post-apply hook reports for applied runs and activity rows. */
export const FOCUS_SYNC_TOPIC = "focus";

/** A fresh Focus activity row of another linked device. */
export interface FocusOtherDeviceActivity {
  device: SyncDeviceRef;
  state: FocusActivityState;
  occurrenceId: string | null;
  eventTitle: string | null;
  phaseStartedAtMs: number | null;
  phaseEndsAtMs: number | null;
  expiresAtMs: number;
  updatedAtMs: number;
}

function readActivity(value: unknown, label: string): FocusOtherDeviceActivity {
  const row = readRecord(value, label);
  return {
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    state: readEnum(row.state, FOCUS_ACTIVITY_STATES, `${label}.state`),
    occurrenceId: readNullable(row.occurrenceId, `${label}.occurrenceId`, readString),
    eventTitle: readNullable(row.eventTitle, `${label}.eventTitle`, readString),
    phaseStartedAtMs: readNullable(row.phaseStartedAtMs, `${label}.phaseStartedAtMs`, readSafeInteger),
    phaseEndsAtMs: readNullable(row.phaseEndsAtMs, `${label}.phaseEndsAtMs`, readSafeInteger),
    expiresAtMs: readSafeInteger(row.expiresAtMs, `${label}.expiresAtMs`),
    updatedAtMs: readSafeInteger(row.updatedAtMs, `${label}.updatedAtMs`),
  };
}

/**
 * Validates the native activity response before the UI uses it.
 * @param value Untrusted command response.
 * @returns The activity rows in response order.
 */
export function parseFocusDeviceActivity(value: unknown): FocusOtherDeviceActivity[] {
  return readArray(value, "focusDeviceActivity", readActivity);
}

/**
 * Whether a sync applied notice touched Focus rows.
 * @param tables Tables and topics from the applied notice.
 */
export function focusActivityChanged(tables: readonly string[]): boolean {
  return tables.includes(FOCUS_SYNC_TOPIC);
}

/**
 * Describes what another device is doing, naming it from its linked-device record.
 * @param t Typed translator.
 * @param activity A fresh activity row.
 */
export function focusActivityLine(t: Translate, activity: FocusOtherDeviceActivity): string {
  const otherDevice = t("pomodoroNotification.activityOtherDevice");
  const device = syncDeviceName(activity.device, otherDevice, otherDevice);
  const title = activity.eventTitle?.trim() || null;
  switch (activity.state) {
    case "focus": return t("pomodoroNotification.activityFocusing", device, title);
    case "short_break":
    case "long_break": return t("pomodoroNotification.activityOnBreak", device, title);
    case "paused": return t("pomodoroNotification.activityPaused", device, title);
  }
}

/**
 * One line per device, keeping each device's newest row.
 * @param t Typed translator.
 * @param activities Fresh activity rows of other devices.
 */
export function focusActivityLines(t: Translate, activities: readonly FocusOtherDeviceActivity[]): string[] {
  const newest = new Map<string, FocusOtherDeviceActivity>();
  for (const activity of activities) {
    const current = newest.get(activity.device.deviceId);
    if (!current || activity.updatedAtMs > current.updatedAtMs) newest.set(activity.device.deviceId, activity);
  }
  return [...newest.values()]
    .sort((left, right) => right.updatedAtMs - left.updatedAtMs)
    .map((activity) => focusActivityLine(t, activity));
}
