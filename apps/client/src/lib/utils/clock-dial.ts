import { parseClockTime } from "$lib/calendar/utils";
import type { CalendarTimeFormat } from "$lib/stores/preference-options";

/** A time of day on the clock dial, with a 0 to 23 hour. */
export interface ClockTime {
  hour: number;
  minute: number;
}

/** Whether the dial is choosing the hour or the minute. */
export type ClockDialMode = "hour" | "minute";

/** Half of a 12-hour day. */
export type ClockPeriod = "am" | "pm";

/** The ring of the dial a value sits on. In 24-hour time, 0 to 11 sit on the outer ring and 12 to 23 on the inner. */
export type ClockDialRing = "outer" | "inner";

/** One labeled value on the dial face. */
export interface ClockDialItem {
  /** The hour (0 to 23) or minute (0 to 59) the item selects. */
  value: number;
  /** The number the face shows. */
  label: number;
  ring: ClockDialRing;
  /** Degrees clockwise from 12 o'clock. */
  angle: number;
}

/** Where the dial hand points. */
export interface ClockDialHand {
  angle: number;
  ring: ClockDialRing;
}

/** Ring radii as fractions of the dial radius, taken from the Material 3 time picker proportions. */
export const CLOCK_DIAL_RING_RADIUS: Readonly<Record<ClockDialRing, number>> = {
  outer: 0.79,
  inner: 0.52,
};

/** A press closer to the center than this fraction of the dial radius picks the inner ring. */
const INNER_RING_BOUNDARY = (CLOCK_DIAL_RING_RADIUS.outer + CLOCK_DIAL_RING_RADIUS.inner) / 2;

const HOUR_STEP_DEGREES = 360 / 12;
const MINUTE_STEP_DEGREES = 360 / 60;
/** Minutes between labeled marks on the minute face. */
export const CLOCK_DIAL_MINUTE_LABEL_STEP = 5;

/**
 * Reads an `HH:MM` value, falling back to another one and then to midnight.
 *
 * @param value The current value, which may be empty.
 * @param fallback The value to start from when `value` is empty or invalid.
 * @returns The parsed time.
 */
export function clockTimeFromValue(value: string, fallback: string): ClockTime {
  return parseClockTime(value) ?? parseClockTime(fallback) ?? { hour: 0, minute: 0 };
}

/** Formats a dial time as the canonical `HH:MM` value. */
export function formatClockValue(time: ClockTime): string {
  return `${String(time.hour).padStart(2, "0")}:${String(time.minute).padStart(2, "0")}`;
}

/** The half of the day an hour falls in. */
export function clockPeriod(hour: number): ClockPeriod {
  return hour < 12 ? "am" : "pm";
}

/** Moves a time into the given half of the day, keeping its hour on the 12-hour face. */
export function withClockPeriod(time: ClockTime, period: ClockPeriod): ClockTime {
  return { hour: (time.hour % 12) + (period === "pm" ? 12 : 0), minute: time.minute };
}

/** The hour as the face and header show it: 1 to 12 in 12-hour time, 0 to 23 otherwise. */
export function displayClockHour(hour: number, format: CalendarTimeFormat): number {
  if (format === "24h") return hour;
  return hour % 12 === 0 ? 12 : hour % 12;
}

/**
 * The angle of a point around the dial center.
 *
 * @param dx Horizontal offset from the center, positive to the right.
 * @param dy Vertical offset from the center, positive downward as in screen coordinates.
 * @returns Degrees clockwise from 12 o'clock, from 0 up to 360.
 */
export function clockDialAngle(dx: number, dy: number): number {
  const degrees = (Math.atan2(dx, -dy) * 180) / Math.PI;
  return (degrees + 360) % 360;
}

/**
 * The labeled values of one dial face.
 *
 * @param mode Whether the face shows hours or minutes.
 * @param format The 12-hour face has one ring of 12, 1 to 11; the 24-hour face adds an inner ring of 12 to 23.
 * @param period The half of the day the 12-hour face selects in.
 */
export function clockDialItems(
  mode: ClockDialMode,
  format: CalendarTimeFormat,
  period: ClockPeriod = "am",
): ClockDialItem[] {
  if (mode === "minute") {
    return Array.from({ length: 60 / CLOCK_DIAL_MINUTE_LABEL_STEP }, (_, index) => {
      const minute = index * CLOCK_DIAL_MINUTE_LABEL_STEP;
      return { value: minute, label: minute, ring: "outer", angle: minute * MINUTE_STEP_DEGREES };
    });
  }
  const outer = Array.from({ length: 12 }, (_, index): ClockDialItem => {
    const hour = format === "24h" ? index : index + (period === "pm" ? 12 : 0);
    return { value: hour, label: displayClockHour(hour, format), ring: "outer", angle: index * HOUR_STEP_DEGREES };
  });
  if (format === "12h") return outer;
  const inner = Array.from({ length: 12 }, (_, index): ClockDialItem => ({
    value: index + 12,
    label: index + 12,
    ring: "inner",
    angle: index * HOUR_STEP_DEGREES,
  }));
  return [...outer, ...inner];
}

/** Where the hand points for a time on the given face. */
export function clockDialHand(time: ClockTime, mode: ClockDialMode, format: CalendarTimeFormat): ClockDialHand {
  if (mode === "minute") return { angle: time.minute * MINUTE_STEP_DEGREES, ring: "outer" };
  return {
    angle: (time.hour % 12) * HOUR_STEP_DEGREES,
    ring: format === "24h" && time.hour >= 12 ? "inner" : "outer",
  };
}

/**
 * The time a press or drag on the dial selects.
 *
 * Hours snap to the nearest hour mark, and in 24-hour time the distance from the center picks the ring. Minutes snap
 * to the nearest minute, so a drag reaches every minute and not only the labeled ones. A 12-hour face keeps the
 * current half of the day.
 *
 * @param input.dx Horizontal offset of the pointer from the dial center.
 * @param input.dy Vertical offset of the pointer from the dial center, positive downward.
 * @param input.radius The dial radius in the same units.
 * @param input.time The time before the press.
 */
export function pickClockDialTime(input: {
  dx: number;
  dy: number;
  radius: number;
  mode: ClockDialMode;
  format: CalendarTimeFormat;
  time: ClockTime;
}): ClockTime {
  const angle = clockDialAngle(input.dx, input.dy);
  if (input.mode === "minute") {
    return { hour: input.time.hour, minute: Math.round(angle / MINUTE_STEP_DEGREES) % 60 };
  }
  const index = Math.round(angle / HOUR_STEP_DEGREES) % 12;
  if (input.format === "12h") {
    return { hour: index + (input.time.hour >= 12 ? 12 : 0), minute: input.time.minute };
  }
  const distance = Math.hypot(input.dx, input.dy);
  const inner = input.radius > 0 && distance < input.radius * INNER_RING_BOUNDARY;
  return { hour: index + (inner ? 12 : 0), minute: input.time.minute };
}

/**
 * Steps the hour or minute by whole units, wrapping around the face. A 12-hour face keeps the half of the day, as its
 * dial does.
 */
export function stepClockTime(
  time: ClockTime,
  mode: ClockDialMode,
  format: CalendarTimeFormat,
  delta: number,
): ClockTime {
  if (mode === "minute") return { hour: time.hour, minute: wrap(time.minute + delta, 60) };
  if (format === "24h") return { hour: wrap(time.hour + delta, 24), minute: time.minute };
  const base = time.hour >= 12 ? 12 : 0;
  return { hour: base + wrap((time.hour % 12) + delta, 12), minute: time.minute };
}

/**
 * The rotation to animate a hand to, continuing from its previous rotation so it turns the short way across 12
 * o'clock instead of spinning back around.
 */
export function continuousClockRotation(previous: number, target: number): number {
  const delta = wrap(target - previous + 180, 360) - 180;
  return previous + delta;
}

/**
 * Reads hour and minute fields typed in the picker's text entry mode.
 *
 * @returns The time, or null while a field is empty or out of range for the format.
 */
export function parseTypedClockTime(
  hourText: string,
  minuteText: string,
  format: CalendarTimeFormat,
  period: ClockPeriod,
): ClockTime | null {
  if (!/^\d{1,2}$/.test(hourText) || !/^\d{1,2}$/.test(minuteText)) return null;
  const hour = Number(hourText);
  const minute = Number(minuteText);
  if (minute > 59) return null;
  if (format === "24h") return hour <= 23 ? { hour, minute } : null;
  if (hour < 1 || hour > 12) return null;
  return withClockPeriod({ hour, minute }, period);
}

function wrap(value: number, size: number): number {
  return ((value % size) + size) % size;
}
