import { Temporal } from "@js-temporal/polyfill";
import { wallClockToUtcIso } from "$lib/calendar/utils";
import {
  CHAT_ALL_WEEKDAYS_MASK,
  CHAT_RECURRENCE_MAX_INTERVAL_MINUTES,
  CHAT_RECURRENCE_MIN_INTERVAL_MINUTES,
  type ChatRecurrenceSpec,
  type ChatScheduledMessageRead,
  type UtcTimestamp,
} from "$lib/chat/contracts";

export type ChatScheduleSuggestionKind = "later_today" | "tomorrow_morning" | "monday_morning";

export interface ChatScheduleSelection {
  date: string;
  time: string;
}

export interface ChatScheduleSuggestion extends ChatScheduleSelection {
  kind: ChatScheduleSuggestionKind;
}

const QUICK_SCHEDULE_LEAD_MINUTES = 60;
const QUICK_SCHEDULE_CUTOFF_HOUR = 20;

function timeLabel(value: Temporal.PlainDateTime): string {
  return `${String(value.hour).padStart(2, "0")}:${String(value.minute).padStart(2, "0")}`;
}

function roundedScheduleStart(now: Temporal.PlainDateTime): Temporal.PlainDateTime {
  const candidate = now.add({ minutes: QUICK_SCHEDULE_LEAD_MINUTES }).with({
    second: 0,
    millisecond: 0,
    microsecond: 0,
    nanosecond: 0,
  });
  const dayStart = candidate.with({ hour: 0, minute: 0 });
  const totalMinutes = candidate.hour * 60 + candidate.minute;
  return dayStart.add({ minutes: Math.ceil(totalMinutes / 30) * 30 });
}

/** Builds distinct Slack-style quick delivery choices from the local wall clock. */
export function chatScheduleSuggestions(
  now: Temporal.PlainDateTime = Temporal.Now.plainDateTimeISO(),
): ChatScheduleSuggestion[] {
  const suggestions: ChatScheduleSuggestion[] = [];
  const later = roundedScheduleStart(now);
  if (later.toPlainDate().equals(now.toPlainDate()) && later.hour <= QUICK_SCHEDULE_CUTOFF_HOUR) {
    suggestions.push({
      kind: "later_today",
      date: later.toPlainDate().toString(),
      time: timeLabel(later),
    });
  }
  const tomorrow = now.toPlainDate().add({ days: 1 }).toPlainDateTime({ hour: 9 });
  suggestions.push({
    kind: "tomorrow_morning",
    date: tomorrow.toPlainDate().toString(),
    time: timeLabel(tomorrow),
  });
  let daysUntilMonday = (8 - now.dayOfWeek) % 7;
  if (daysUntilMonday <= 1) daysUntilMonday += 7;
  const monday = now.toPlainDate().add({ days: daysUntilMonday }).toPlainDateTime({ hour: 9 });
  suggestions.push({
    kind: "monday_morning",
    date: monday.toPlainDate().toString(),
    time: timeLabel(monday),
  });
  return suggestions;
}

export function defaultChatScheduleSelection(
  now: Temporal.PlainDateTime = Temporal.Now.plainDateTimeISO(),
): ChatScheduleSelection {
  const rounded = roundedScheduleStart(now);
  return {
    date: rounded.toPlainDate().toString(),
    time: timeLabel(rounded),
  };
}

export function chatScheduleUtc(
  selection: ChatScheduleSelection,
  timezone: string,
): string | null {
  if (!selection.date || !selection.time || !timezone) return null;
  try {
    return wallClockToUtcIso(`${selection.date} ${selection.time}`, timezone);
  } catch {
    return null;
  }
}

export function chatScheduleIsFuture(
  scheduledFor: string | null,
  nowMs = Date.now(),
  minimumLeadMs = 30_000,
): boolean {
  if (!scheduledFor) return false;
  const scheduledMs = Date.parse(scheduledFor);
  return Number.isFinite(scheduledMs) && scheduledMs >= nowMs + minimumLeadMs;
}

/** Repeat choices the schedule menu offers, in display order. */
export const CHAT_REPEAT_CHOICES = ["none", "daily", "weekdays", "weekly", "custom_days", "interval"] as const;
export type ChatRepeatChoice = (typeof CHAT_REPEAT_CHOICES)[number];

/** A repeat choice with the values the custom choices need. */
export interface ChatRepeatSelection {
  choice: ChatRepeatChoice;
  /** Weekday mask of `custom_days`. */
  weekdays: number;
  /** Hours between occurrences of `interval`. */
  intervalHours: number;
}

/** Weekday mask of Monday through Friday. */
export const CHAT_WORKDAYS_MASK = 0b001_1111;
/** Shortest and longest `interval` choice, in hours. */
export const CHAT_REPEAT_MIN_INTERVAL_HOURS = CHAT_RECURRENCE_MIN_INTERVAL_MINUTES / 60;
export const CHAT_REPEAT_MAX_INTERVAL_HOURS = CHAT_RECURRENCE_MAX_INTERVAL_MINUTES / 60;

/** The repeat selection of a message that sends once. */
export const CHAT_NO_REPEAT: ChatRepeatSelection = {
  choice: "none",
  weekdays: CHAT_WORKDAYS_MASK,
  intervalHours: 24,
};

/** The weekday mask bit of an ISO weekday, 1 for Monday through 7 for Sunday. */
export function chatWeekdayBit(isoWeekday: number): number {
  return 1 << (isoWeekday - 1);
}

/** The ISO weekdays a mask names, in week order. */
export function chatWeekdaysOfMask(mask: number): number[] {
  return [1, 2, 3, 4, 5, 6, 7].filter((day) => (mask & chatWeekdayBit(day)) !== 0);
}

/**
 * Builds the repeat rule a selection names, anchored at the local `selection` time in `timeZone`. Fixed time rules
 * post at that local minute; `weekly` repeats on the selected date's weekday. Returns `null` for `none` and for a
 * selection outside the backend bounds, such as an empty custom day set.
 */
export function chatRecurrenceFromRepeat(
  repeat: ChatRepeatSelection,
  selection: ChatScheduleSelection,
  timeZone: string,
): ChatRecurrenceSpec | null {
  if (repeat.choice === "none") return null;
  if (repeat.choice === "interval") {
    const hours = repeat.intervalHours;
    if (!Number.isInteger(hours) || hours < CHAT_REPEAT_MIN_INTERVAL_HOURS || hours > CHAT_REPEAT_MAX_INTERVAL_HOURS) {
      return null;
    }
    return { rule: { kind: "interval", minutes: hours * 60 }, timeZone, quietPolicy: "defer" };
  }
  const localMinute = chatTimeLocalMinute(selection.time);
  const weekdays = repeatWeekdays(repeat, selection.date);
  if (localMinute === null || weekdays === null) return null;
  return { rule: { kind: "fixed_time", localMinute, weekdays }, timeZone, quietPolicy: "defer" };
}

/** The repeat selection that shows `spec` in the schedule menu, so a confirmation can start from a proposed rule. */
export function chatRepeatFromRecurrence(spec: ChatRecurrenceSpec | null): ChatRepeatSelection {
  if (spec === null) return CHAT_NO_REPEAT;
  const { rule } = spec;
  if (rule.kind === "interval") {
    const hours = Math.min(
      CHAT_REPEAT_MAX_INTERVAL_HOURS,
      Math.max(CHAT_REPEAT_MIN_INTERVAL_HOURS, Math.round(rule.minutes / 60)),
    );
    return { ...CHAT_NO_REPEAT, choice: "interval", intervalHours: hours };
  }
  if (rule.weekdays === CHAT_ALL_WEEKDAYS_MASK) return { ...CHAT_NO_REPEAT, choice: "daily" };
  if (rule.weekdays === CHAT_WORKDAYS_MASK) return { ...CHAT_NO_REPEAT, choice: "weekdays" };
  return { ...CHAT_NO_REPEAT, choice: "custom_days", weekdays: rule.weekdays };
}

function repeatWeekdays(repeat: ChatRepeatSelection, date: string): number | null {
  if (repeat.choice === "daily") return CHAT_ALL_WEEKDAYS_MASK;
  if (repeat.choice === "weekdays") return CHAT_WORKDAYS_MASK;
  if (repeat.choice === "custom_days") {
    const mask = repeat.weekdays & CHAT_ALL_WEEKDAYS_MASK;
    return mask === 0 ? null : mask;
  }
  try {
    return chatWeekdayBit(Temporal.PlainDate.from(date).dayOfWeek);
  } catch {
    return null;
  }
}

/** The local minute of an `HH:MM` time, or `null` when it is not a valid time. */
export function chatTimeLocalMinute(time: string): number | null {
  const match = /^(\d{2}):(\d{2})$/.exec(time);
  if (!match) return null;
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (hour > 23 || minute > 59) return null;
  return hour * 60 + minute;
}

/** The `HH:MM` label of a local minute of the day. */
export function chatLocalMinuteTime(localMinute: number): string {
  const hour = Math.floor(localMinute / 60);
  return `${String(hour).padStart(2, "0")}:${String(localMinute % 60).padStart(2, "0")}`;
}

/** When a scheduled message next posts: its pending occurrence, or the end of the quiet period that occurrence waits for. */
export function chatScheduledNextDelivery(
  message: Pick<ChatScheduledMessageRead, "scheduledFor" | "recurrence">,
): UtcTimestamp {
  const recurrence = message.recurrence;
  if (!recurrence) return message.scheduledFor;
  return recurrence.deferredUntil ?? recurrence.nextOccurrenceAt;
}
