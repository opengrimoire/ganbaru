import { Temporal } from "@js-temporal/polyfill";
import type {
  NotesMentionNotification,
  NotesMentionNotificationKind,
} from "$lib/notes/types";

export const NOTES_DATE_ONLY_REMINDER_HOUR = 9;

export interface NotesMentionNotificationPreferenceSnapshot {
  mentionNotificationsEnabled: boolean;
  reminderNotificationsEnabled: boolean;
  userMentionNotificationsEnabled: boolean;
  taskMentionNotificationsEnabled: boolean;
}

export interface NotesMentionNotificationDueOptions {
  nowMs: number;
  timeZone?: string;
  dateOnlyReminderHour?: number;
}

function currentTimeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
}

function isKindEnabled(
  kind: NotesMentionNotificationKind,
  preferences: NotesMentionNotificationPreferenceSnapshot,
): boolean {
  if (!preferences.mentionNotificationsEnabled) return false;
  if (kind === "reminder") return preferences.reminderNotificationsEnabled;
  if (kind === "user_mention") return preferences.userMentionNotificationsEnabled;
  return preferences.taskMentionNotificationsEnabled;
}

function dateOnlyReminderMs(value: string, timeZone: string, hour: number): number | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return null;
  try {
    const date = Temporal.PlainDate.from(value);
    const dateTime = date.toPlainDateTime(Temporal.PlainTime.from({ hour }));
    return dateTime.toZonedDateTime(timeZone).toInstant().epochMilliseconds;
  } catch {
    return null;
  }
}

function dateTimeReminderMs(value: string, timeZone: string): number | null {
  try {
    return Temporal.Instant.from(value).epochMilliseconds;
  } catch {
    try {
      return Temporal.PlainDateTime.from(value)
        .toZonedDateTime(timeZone)
        .toInstant()
        .epochMilliseconds;
    } catch {
      return null;
    }
  }
}

export function notesMentionNotificationDueAtMs(
  notification: NotesMentionNotification,
  options: Omit<NotesMentionNotificationDueOptions, "nowMs"> = {},
): number | null {
  if (notification.kind === "user_mention" || notification.kind === "task_mention") {
    return dateTimeReminderMs(notification.created_time, options.timeZone ?? currentTimeZone());
  }

  const triggerAt = notification.trigger_at?.trim();
  if (!triggerAt) return null;
  const timeZone = options.timeZone ?? currentTimeZone();
  const dateOnlyHour = options.dateOnlyReminderHour ?? NOTES_DATE_ONLY_REMINDER_HOUR;
  const triggerMs = dateOnlyReminderMs(triggerAt, timeZone, dateOnlyHour)
    ?? dateTimeReminderMs(triggerAt, timeZone);
  return triggerMs === null ? null : triggerMs - notification.remind_minutes_before * 60_000;
}

export function notesMentionNotificationIsDue(
  notification: NotesMentionNotification,
  preferences: NotesMentionNotificationPreferenceSnapshot,
  options: NotesMentionNotificationDueOptions,
): boolean {
  if (notification.status !== "pending") return false;
  if (!isKindEnabled(notification.kind, preferences)) return false;
  const dueAtMs = notesMentionNotificationDueAtMs(notification, options);
  return dueAtMs !== null && dueAtMs <= options.nowMs;
}
