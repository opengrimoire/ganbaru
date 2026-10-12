import { invoke } from "@tauri-apps/api/core";
import type { CalendarEvent } from "$lib/calendar/types";
import { parseCalendarDate } from "$lib/calendar/utils";
import type { Translate } from "$lib/i18n/translator.svelte";
import { loadNotificationSchedulerEvents } from "./mobile-calendar-notifications";

const MAX_NATIVE_REMINDERS = 128;
const MAX_EVENT_TITLE_LENGTH = 160;

/** Supply localized copy without execution state to the native Focus owner. */
export function buildMobileFocusNotificationCopy(t: Translate) {
  return {
    channelName: t("pomodoroNotification.channelName"),
    channelDescription: t("pomodoroNotification.channelDescription"),
    alertsChannelName: t("pomodoroNotification.alertsChannelName"),
    alertsChannelDescription: t("pomodoroNotification.alertsChannelDescription"),
    focusTitle: t("pomodoroNotification.focusTitle"),
    shortBreakTitle: t("pomodoroNotification.shortBreakTitle"),
    longBreakTitle: t("pomodoroNotification.longBreakTitle"),
    pausedText: t("pomodoroNotification.pausedText"),
    focusCompleteTitle: t("pomodoroNotification.focusCompleteTitle"),
    breakCompleteTitle: t("pomodoroNotification.breakCompleteTitle"),
    sessionCompleteText: t("pomodoroNotification.sessionCompleteText"),
  };
}

function stableHash(value: string): string {
  let hash = 0x811c9dc5;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}

function boundedTitle(value: string): string | null {
  const trimmed = value.trim();
  if (trimmed.length === 0) return null;
  let result = "";
  for (const character of trimmed) {
    if (result.length + character.length > MAX_EVENT_TITLE_LENGTH) break;
    result += character;
  }
  return result;
}

/** A Calendar commitment reminder carries no execution state or rhythm projection. */
export interface MobilePomodoroReminder {
  id: string;
  eventId: string;
  startsAtEpochMs: number;
  endsAtEpochMs: number;
  title: string;
  body: string;
  channelName: string;
  channelDescription: string;
}

/** Build bounded Android reminders for future and currently due focus commitments. */
export function buildMobilePomodoroSchedule(
  events: readonly CalendarEvent[],
  t: Translate,
  nowMs: number = Date.now(),
): MobilePomodoroReminder[] {
  if (!Number.isFinite(nowMs)) return [];
  const identifiers = new Map<string, string>();
  return events
    .filter((event) => event.pomodoroConfig && !event.allDay && event.status !== "cancelled")
    .map((event): MobilePomodoroReminder | null => {
      if (event.id.trim().length === 0 || event.id.length > 256) return null;
      const startsAtEpochMs = parseCalendarDate(event.start).getTime();
      const endsAtEpochMs = parseCalendarDate(event.end).getTime();
      if (
        !Number.isFinite(startsAtEpochMs)
        || !Number.isFinite(endsAtEpochMs)
        || endsAtEpochMs <= Math.max(startsAtEpochMs, nowMs)
      ) return null;
      const occurrenceKey = `${event.id}\u0000${event.start}`;
      const id = `reminder-${stableHash(occurrenceKey)}`;
      const previous = identifiers.get(id);
      if (previous && previous !== occurrenceKey) {
        throw new Error("Focus reminder identifier collision");
      }
      if (previous) return null;
      identifiers.set(id, occurrenceKey);
      return {
        id,
        eventId: event.id,
        startsAtEpochMs,
        endsAtEpochMs,
        title: boundedTitle(event.title) ?? t("pomodoroNotification.commitmentDueTitle"),
        body: t("pomodoroNotification.commitmentDueText"),
        channelName: t("pomodoroNotification.alertsChannelName"),
        channelDescription: t("pomodoroNotification.alertsChannelDescription"),
      };
    })
    .filter((reminder): reminder is MobilePomodoroReminder => reminder !== null)
    .sort((left, right) => left.startsAtEpochMs - right.startsAtEpochMs
      || left.endsAtEpochMs - right.endsAtEpochMs
      || (left.id < right.id ? -1 : left.id > right.id ? 1 : 0))
    .slice(0, MAX_NATIVE_REMINDERS);
}

/** Reconcile Android's durable focus reminders with Calendar. */
export async function reconcileMobilePomodoroSchedule(t: Translate): Promise<void> {
  await Promise.all([
    invoke("focus_notification_copy", { copy: buildMobileFocusNotificationCopy(t) }),
    (async () => {
      const events = await loadNotificationSchedulerEvents();
      const schedule = buildMobilePomodoroSchedule(events, t);
      await invoke("plugin:ganbaru-mobile-notifications|reconcilePomodoroSchedule", { schedule });
    })(),
  ]);
}

/** Serialize Android focus reminder reconciliation across rapid Calendar mutations. */
export class MobilePomodoroScheduleScheduler {
  private active: Promise<void> | null = null;
  private rerunRequested = false;

  constructor(private readonly t: Translate) {}

  reconcile(): Promise<void> {
    if (this.active) {
      this.rerunRequested = true;
      return this.active;
    }
    this.active = this.drain().finally(() => {
      this.active = null;
    });
    return this.active;
  }

  private async drain(): Promise<void> {
    do {
      this.rerunRequested = false;
      await reconcileMobilePomodoroSchedule(this.t);
    } while (this.rerunRequested);
  }
}
