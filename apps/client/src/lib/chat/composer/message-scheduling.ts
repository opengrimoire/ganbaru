import { Temporal } from "@js-temporal/polyfill";
import { wallClockToUtcIso } from "$lib/calendar/utils";

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
