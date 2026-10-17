import { invoke } from "@tauri-apps/api/core";
import { Temporal } from "@js-temporal/polyfill";
import type { CalendarEvent } from "$lib/calendar/types";
import {
  formatEventNotificationBody,
} from "$lib/calendar/event-notifications";
import { parseCalendarDate } from "$lib/calendar/utils";
import type { Translate } from "$lib/i18n/translator.svelte";
import { localTimezone } from "$lib/stores/calendar/event-payloads";
import { loadNativeCalendarWindow } from "$lib/stores/calendar/native-window";

const CALENDAR_NOTIFICATION_ID_NAMESPACE = "calendar-notification";
const CALENDAR_NOTIFICATION_ID_START = 1_000_000_000;
const CALENDAR_NOTIFICATION_ID_SPAN = 400_000_000;
const SCHEDULING_HORIZON_MONTHS = 12;
const MAX_NATIVE_DELIVERIES = 256;
const STARTUP_CATCH_UP_MS = 2_500;
const MIN_FUTURE_SCHEDULE_MS = 750;
const MAX_CALENDAR_ACTION_EVENT_ID_LENGTH = 256;

interface NativePendingNotification {
  id: number;
  title?: string;
  body?: string;
  eventId?: string;
  scheduledAtEpochMs?: number;
}

export interface NativeCalendarNotification {
  id: number;
  title: string;
  body: string;
  eventId: string;
  scheduledAtEpochMs: number;
}

export interface MobileCalendarNotificationStatus {
  permission: "granted" | "denied" | "prompt";
  channel: {
    exists: boolean;
    enabled: boolean;
    soundConfigured: boolean;
  };
  exactAlarm: {
    apiLevel: number;
    required: boolean;
    granted: boolean;
  };
}

interface MobileCalendarNotificationCopy {
  channelName: string;
  channelDescription: string;
}

interface BuildNativeNotificationsOptions {
  nowMs: number;
  locale: string | readonly string[];
  t: Translate;
  titleFallback: string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Produce a deterministic positive Android notification ID in Calendar's reserved range. */
export function calendarNativeNotificationId(key: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < key.length; index += 1) {
    hash ^= key.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return CALENDAR_NOTIFICATION_ID_START + ((hash >>> 0) % CALENDAR_NOTIFICATION_ID_SPAN);
}

function calendarDeliveryKey(event: CalendarEvent, minutesBefore: number): string {
  return `${CALENDAR_NOTIFICATION_ID_NAMESPACE}::${event.id}::${event.start}::${minutesBefore}`;
}

function truncate(value: string, maxLength: number): string {
  return value.length <= maxLength ? value : value.slice(0, maxLength);
}

/** Convert expanded Calendar occurrences into bounded native Android deliveries. */
export function buildNativeCalendarNotifications(
  events: readonly CalendarEvent[],
  options: BuildNativeNotificationsOptions,
): NativeCalendarNotification[] {
  const notifications: NativeCalendarNotification[] = [];
  const keysById = new Map<number, string>();

  for (const event of events) {
    if (event.status === "cancelled" || !event.notifications?.length) continue;
    const startMs = event.startInstant
      ? Date.parse(event.startInstant) : parseCalendarDate(event.start).getTime();
    if (!Number.isFinite(startMs)) continue;
    const offsets = [...new Set(event.notifications)]
      .filter((minutes) => Number.isSafeInteger(minutes) && minutes >= 0)
      .sort((left, right) => left - right);

    for (const minutesBefore of offsets) {
      const deadlineMs = startMs - minutesBefore * 60_000;
      if (deadlineMs < options.nowMs - STARTUP_CATCH_UP_MS) continue;
      const scheduledMs = Math.max(deadlineMs, options.nowMs + MIN_FUTURE_SCHEDULE_MS);
      const key = calendarDeliveryKey(event, minutesBefore);
      const id = calendarNativeNotificationId(key);
      const existingKey = keysById.get(id);
      if (existingKey && existingKey !== key) {
        throw new Error("Calendar notification identifier collision");
      }
      keysById.set(id, key);
      notifications.push({
        id,
        title: truncate(event.title.trim() || options.titleFallback, 160),
        body: truncate(
          formatEventNotificationBody(event, new Date(deadlineMs), {
            t: options.t,
            locale: options.locale,
          }),
          1_000,
        ),
        eventId: event.recurringParentId ?? event.id,
        scheduledAtEpochMs: scheduledMs,
      });
    }
  }

  return notifications
    .sort((left, right) => {
      return left.scheduledAtEpochMs - right.scheduledAtEpochMs || left.id - right.id;
    })
    .slice(0, MAX_NATIVE_DELIVERIES);
}

function isCalendarNotificationId(id: number): boolean {
  return Number.isInteger(id)
    && id >= CALENDAR_NOTIFICATION_ID_START
    && id < CALENDAR_NOTIFICATION_ID_START + CALENDAR_NOTIFICATION_ID_SPAN;
}

function pendingMatchesDesired(
  pending: NativePendingNotification,
  desired: NativeCalendarNotification,
): boolean {
  return pending.title === desired.title
    && pending.body === desired.body
    && pending.eventId === desired.eventId
    && pending.scheduledAtEpochMs === desired.scheduledAtEpochMs;
}

/** Read Android notification permission and exact-alarm access together. */
export async function mobileCalendarNotificationStatus(): Promise<MobileCalendarNotificationStatus> {
  const [permissionGranted, channel, exactAlarm] = await Promise.all([
    invoke<boolean | null>("plugin:notification|is_permission_granted"),
    invoke<MobileCalendarNotificationStatus["channel"]>(
      "plugin:ganbaru-mobile-notifications|calendarChannelStatus",
    ),
    invoke<MobileCalendarNotificationStatus["exactAlarm"]>(
      "mobile_notification_exact_alarm_status",
    ),
  ]);
  return {
    permission: permissionGranted === true
      ? "granted"
      : permissionGranted === false ? "denied" : "prompt",
    channel,
    exactAlarm,
  };
}

/** Request Android notification permission in response to a user action. */
export async function requestMobileCalendarNotificationPermission(): Promise<MobileCalendarNotificationStatus> {
  await invoke("plugin:notification|request_permission");
  return mobileCalendarNotificationStatus();
}

/** Open the relevant Android system settings for the current delivery limitation. */
export async function resolveMobileCalendarNotificationStatus(
  status: MobileCalendarNotificationStatus,
): Promise<void> {
  if (status.permission !== "granted") {
    await invoke("mobile_notification_open_settings");
  } else if (
    status.channel.exists
    && (!status.channel.enabled || !status.channel.soundConfigured)
  ) {
    await invoke("mobile_notification_open_settings");
  } else if (status.exactAlarm.required && !status.exactAlarm.granted) {
    await invoke("mobile_notification_open_exact_alarm_settings");
  }
}

/** Read native occurrences for the bounded Android reminder window. */
export async function loadNotificationSchedulerEvents(): Promise<CalendarEvent[]> {
  const renderZone = localTimezone();
  const today = Temporal.Now.plainDateISO(renderZone);
  const windowStart = today.subtract({ days: 1 });
  const windowEnd = today.add({ months: SCHEDULING_HORIZON_MONTHS });
  const mapped = await loadNativeCalendarWindow({
    windowStartDate: windowStart.toString(),
    windowEndDate: windowEnd.toString(),
    renderZone,
    includeTotalEventCount: false,
  }, "notifications");
  return mapped.windowEvents;
}

async function reconcileNativeSchedule(
  desired: NativeCalendarNotification[],
  copy: MobileCalendarNotificationCopy,
): Promise<void> {
  await invoke("mobile_notification_ensure_calendar_channel", {
    name: copy.channelName,
    description: copy.channelDescription,
  });

  const pending = await invoke<NativePendingNotification[]>(
    "plugin:ganbaru-mobile-notifications|pendingCalendarNotifications",
  );
  const desiredById = new Map(desired.map((notification) => [notification.id, notification]));
  const pendingById = new Map(
    pending
      .filter((notification) => isCalendarNotificationId(notification.id))
      .map((notification) => [notification.id, notification]),
  );
  const cancelIds: number[] = [];
  const schedule: NativeCalendarNotification[] = [];

  for (const [id, notification] of pendingById) {
    const next = desiredById.get(id);
    if (!next || !pendingMatchesDesired(notification, next)) cancelIds.push(id);
  }
  for (const notification of desired) {
    const existing = pendingById.get(notification.id);
    if (!existing || !pendingMatchesDesired(existing, notification)) {
      schedule.push(notification);
    }
  }

  if (cancelIds.length > 0) {
    await invoke("plugin:ganbaru-mobile-notifications|cancelCalendarNotifications", {
      ids: cancelIds,
    });
  }
  if (schedule.length > 0) {
    await invoke("plugin:ganbaru-mobile-notifications|scheduleCalendarNotifications", {
      notifications: schedule.map((notification) => ({
        id: notification.id,
        title: notification.title,
        body: notification.body,
        eventId: notification.eventId,
        scheduledAtEpochMs: notification.scheduledAtEpochMs,
      })),
    });
  }
}

/** Extract a bounded Calendar event ID from an untrusted native action response. */
export function calendarEventIdFromNativeAction(payload: unknown): string | null {
  if (!isRecord(payload)) return null;
  const eventId = payload.eventId;
  if (typeof eventId !== "string") return null;
  return eventId.length > 0 && eventId.length <= MAX_CALENDAR_ACTION_EVENT_ID_LENGTH
    ? eventId
    : null;
}

/** Serialize Android schedule reconciliation and coalesce mutations that occur during a run. */
export class MobileCalendarNotificationScheduler {
  private active: Promise<void> | null = null;
  private rerunRequested = false;

  constructor(
    private readonly t: Translate,
    private readonly locale: () => string,
  ) {}

  reconcile(): Promise<void> {
    if (this.active) {
      this.rerunRequested = true;
      return this.active;
    }
    this.active = this.run().finally(() => {
      this.active = null;
      if (this.rerunRequested) {
        this.rerunRequested = false;
        void this.reconcile();
      }
    });
    return this.active;
  }

  async takeAction(): Promise<string | null> {
    const payload = await invoke<unknown>(
      "plugin:ganbaru-mobile-notifications|takeCalendarNotificationAction",
    );
    return calendarEventIdFromNativeAction(payload);
  }

  private async run(): Promise<void> {
    const events = await loadNotificationSchedulerEvents();
    const desired = buildNativeCalendarNotifications(events, {
      nowMs: Date.now(),
      locale: this.locale(),
      t: this.t,
      titleFallback: this.t("calendar.notification.titleFallback"),
    });
    await reconcileNativeSchedule(desired, {
      channelName: this.t("calendar.notifications.androidChannelName"),
      channelDescription: this.t("calendar.notifications.androidChannelDescription"),
    });
  }
}
