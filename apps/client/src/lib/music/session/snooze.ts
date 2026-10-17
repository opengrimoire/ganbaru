import { Temporal } from "@js-temporal/polyfill";

export type MusicSnoozePreset = "day" | "week" | "month";

/** Recognizes a timed preset saved with its start and end instants. */
export function musicSnoozePreset(startsAtMs: number, endsAtMs: number | null, timeZone: string): MusicSnoozePreset | null {
  if (endsAtMs === null) return null;
  for (const duration of ["day", "week", "month"] as const) {
    if (musicSnoozeEndsAt(duration, startsAtMs, timeZone) === endsAtMs) return duration;
  }
  return null;
}

/** Resolves a user-facing Snooze duration to one stable instant. */
export function musicSnoozeEndsAt(
  duration: MusicSnoozePreset,
  nowMs: number,
  timeZone: string,
): number {
  const now = Temporal.Instant.fromEpochMilliseconds(nowMs).toZonedDateTimeISO(timeZone);
  if (duration === "day") return now.add({ days: 1 }).epochMilliseconds;
  if (duration === "week") return now.add({ weeks: 1 }).epochMilliseconds;
  return now.add({ months: 1 }).epochMilliseconds;
}
