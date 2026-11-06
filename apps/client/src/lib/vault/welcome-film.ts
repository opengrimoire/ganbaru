/**
 * Timing for the welcome film: one continuous take of a day in the app. A Focus session is planned in the Calendar,
 * run through its break and its finish, then seen among the week's other sessions as the view pulls back. Every
 * moment is derived from the milliseconds elapsed since the film started, so a frame is a pure function of time and
 * the reduced-motion still is simply the last frame.
 */

import { createPresetPomodoroConfig, deriveRhythmPlan, type PomodoroPresetKey } from "$lib/pomodoro/rhythm";
import { computeDayTimelineBands } from "$lib/pomodoro/segments";

/** One point of a curve: `[x, y]`, with x strictly increasing across a curve. */
export type FilmKeyframe = readonly [x: number, y: number];

/** A span of session minutes. */
export interface FilmSpan {
  start: number;
  end: number;
}

/** Moments of the film, in milliseconds from its start. */
export const FILM_BEATS = {
  /** A press drags the session out on the empty day, and letting go saves it. */
  createStart: 500,
  createEnd: 1150,
  createSaved: 1300,
  /** The session's Focus rail appears beside it, with its planned break marked. */
  railIn: 1450,
  /** The current time reaches the session and Focus starts. */
  focusStart: 2400,
  /** The break screen covers the app for the length of the break. */
  breakStart: 3800,
  breakEnd: 4900,
  /** The session ends, the finished screen shows, and the app returns. */
  sessionEnd: 6150,
  finishedOut: 7000,
  /** The camera pulls back from the day to the week. */
  pullBack: 7250,
  end: 9600,
} as const;

/** How long the pull back from the day to the week takes. */
export const FILM_PULL_BACK_MS = 1200;

/** The Focus preset the film's sessions run with. */
export const FILM_FOCUS_PRESET: PomodoroPresetKey = "creative";

/** The session's length in minutes: long enough for one break of the preset, ending on focus. */
export const FILM_SESSION_MINUTES = 50;

/** The session's phases in minutes, planned by Focus's own rhythm for the preset and the session's length. */
export const FILM_SESSION: readonly { kind: "focus" | "break"; minutes: number }[] = deriveRhythmPlan(
  createPresetPomodoroConfig(FILM_FOCUS_PRESET),
  FILM_SESSION_MINUTES,
).segments.map((segment) => ({
  kind: segment.phase === "focus" ? "focus" : "break",
  minutes: segment.endOffsetMinutes - segment.startOffsetMinutes,
}));

/** Where each phase of `kind` falls in the session, in session minutes. */
function phaseSpans(kind: "focus" | "break"): FilmSpan[] {
  const spans: FilmSpan[] = [];
  let start = 0;
  for (const phase of FILM_SESSION) {
    if (phase.kind === kind) spans.push({ start, end: start + phase.minutes });
    start += phase.minutes;
  }
  return spans;
}

/** The session's breaks, which the Focus rail marks before they happen. */
export const FILM_BREAKS: readonly FilmSpan[] = phaseSpans("break");

/**
 * The session's minutes against film time. Work hurries along, the break slows down so its screen can be read and
 * reaches its end exactly as the break screen lifts, and the second half hurries to the end of the session.
 */
const FILM_BREAK: FilmSpan = FILM_BREAKS[0] ?? { start: FILM_SESSION_MINUTES, end: FILM_SESSION_MINUTES };
const SESSION_CLOCK: readonly FilmKeyframe[] = [
  [FILM_BEATS.focusStart + 200, 0],
  [FILM_BEATS.breakStart - 300, FILM_BREAK.start - 2],
  [FILM_BEATS.breakStart, FILM_BREAK.start],
  [FILM_BEATS.breakEnd, FILM_BREAK.end],
  [FILM_BEATS.sessionEnd - 250, FILM_SESSION_MINUTES - 2],
  [FILM_BEATS.sessionEnd, FILM_SESSION_MINUTES],
];

/** How many minutes before the session the current time starts, so it can be seen walking up to it. */
export const FILM_LEAD_IN_MINUTES = 14;

/** The current time against film time before Focus starts, in minutes relative to the session's start. */
const LEAD_IN_CLOCK: readonly FilmKeyframe[] = [
  [0, -FILM_LEAD_IN_MINUTES],
  [FILM_BEATS.focusStart, 0],
];

/** The Focus phase running at one moment of the session, and what is left of it. */
export type FilmSessionMoment =
  | { kind: "focus" | "break"; remainingSeconds: number; totalSeconds: number }
  | { kind: "done" };

/**
 * Interpolates a curve through `points` with a monotone cubic (PCHIP), so motion never overshoots or runs backwards
 * between keyframes. The curve starts and ends at rest, and holds its first and last values outside its range.
 */
export function monotoneCurve(points: readonly FilmKeyframe[], x: number): number {
  const count = points.length;
  if (count === 0) return 0;
  if (x <= points[0][0]) return points[0][1];
  if (x >= points[count - 1][0]) return points[count - 1][1];

  const widths: number[] = [];
  const slopes: number[] = [];
  for (let index = 0; index < count - 1; index += 1) {
    const width = points[index + 1][0] - points[index][0];
    widths.push(width);
    slopes.push((points[index + 1][1] - points[index][1]) / width);
  }
  const tangents = points.map((_, index) => {
    if (index === 0 || index === count - 1) return 0;
    const before = slopes[index - 1];
    const after = slopes[index];
    if (before * after <= 0) return 0;
    const weightBefore = 2 * widths[index] + widths[index - 1];
    const weightAfter = widths[index] + 2 * widths[index - 1];
    return (weightBefore + weightAfter) / (weightBefore / before + weightAfter / after);
  });

  let segment = 0;
  while (x > points[segment + 1][0]) segment += 1;
  const width = widths[segment];
  const t = (x - points[segment][0]) / width;
  const t2 = t * t;
  const t3 = t2 * t;
  return (2 * t3 - 3 * t2 + 1) * points[segment][1]
    + (t3 - 2 * t2 + t) * width * tangents[segment]
    + (-2 * t3 + 3 * t2) * points[segment + 1][1]
    + (t3 - t2) * width * tangents[segment + 1];
}

/** How far `elapsed` is through the span from `start` to `end`, from 0 to 1. */
export function filmSpan(elapsed: number, start: number, end: number): number {
  if (end <= start) return elapsed >= end ? 1 : 0;
  return Math.min(1, Math.max(0, (elapsed - start) / (end - start)));
}

/** Eases a 0 to 1 progress so it starts and ends gently. */
export function easeInOutCubic(progress: number): number {
  return progress < 0.5 ? 4 * progress ** 3 : 1 - (-2 * progress + 2) ** 3 / 2;
}

/** A Focus event elsewhere in the film's week, in minutes of its day, with the preset it runs. */
export interface FilmFocusEvent {
  start: number;
  end: number;
  preset: PomodoroPresetKey;
}

/** A break on a rail, in minutes of the day. */
export interface FilmBreak extends FilmSpan {
  long: boolean;
}

/** One Focus rail: back-to-back Focus events share it, as in the Calendar. Spans are in minutes of the day. */
export interface FilmRail extends FilmSpan {
  breaks: FilmBreak[];
  /** The focus between the breaks, which the rail fills in once it is recorded. */
  focus: FilmSpan[];
}

/**
 * The Focus rails of one day, planned exactly as the Calendar plans them: back-to-back events share a rail and carry
 * their rhythm into the next event, whatever its preset, so a long break can fall in the second event of a chain.
 */
export function filmDayRails(events: readonly FilmFocusEvent[]): FilmRail[] {
  const bands = computeDayTimelineBands(
    events.map((event, index) => ({
      id: `film-${index}`,
      config: createPresetPomodoroConfig(event.preset),
      startMs: event.start * 60_000,
      endMs: event.end * 60_000,
      startMinute: event.start,
      endMinute: event.end,
    })),
    null,
    0,
    0,
  );
  const rails: FilmRail[] = [];
  for (const event of [...events].sort((left, right) => left.start - right.start)) {
    const last = rails.at(-1);
    if (last && event.start <= last.end) last.end = Math.max(last.end, event.end);
    else rails.push({ start: event.start, end: event.end, breaks: [], focus: [] });
  }
  for (const rail of rails) {
    rail.breaks = bands
      .filter((band) => band.phase !== "focus" && band.topMinute >= rail.start && band.topMinute < rail.end)
      .map((band) => ({ start: band.topMinute, end: band.topMinute + band.heightMinutes, long: band.phase === "long_break" }));
    let cursor = rail.start;
    for (const pause of rail.breaks) {
      if (pause.start > cursor) rail.focus.push({ start: cursor, end: pause.start });
      cursor = pause.end;
    }
    if (cursor < rail.end) rail.focus.push({ start: cursor, end: rail.end });
  }
  return rails;
}

/** The session's minutes since it started at a moment of the film, from 0 to the session's length. */
export function filmSessionMinute(elapsed: number): number {
  return monotoneCurve(SESSION_CLOCK, elapsed);
}

/**
 * The current time at a moment of the film, in minutes relative to the session's start: it walks up to the session,
 * reaches it as Focus starts, and then follows the session's clock.
 */
export function filmClockMinute(elapsed: number): number {
  return elapsed < FILM_BEATS.focusStart ? monotoneCurve(LEAD_IN_CLOCK, elapsed) : filmSessionMinute(elapsed);
}

/** The Focus phase and its remaining time once `minute` minutes of the session have passed. */
export function filmSessionMoment(minute: number): FilmSessionMoment {
  let start = 0;
  for (const phase of FILM_SESSION) {
    const end = start + phase.minutes;
    if (minute < end) {
      return {
        kind: phase.kind,
        remainingSeconds: Math.max(0, Math.ceil((end - minute) * 60)),
        totalSeconds: phase.minutes * 60,
      };
    }
    start = end;
  }
  return { kind: "done" };
}

/** The focus the session has recorded by `minute`, as the rail fills it in, in session minutes. */
export function filmFocusSpans(minute: number): FilmSpan[] {
  return phaseSpans("focus")
    .filter((span) => minute > span.start)
    .map((span) => ({ start: span.start, end: Math.min(span.end, minute) }));
}
