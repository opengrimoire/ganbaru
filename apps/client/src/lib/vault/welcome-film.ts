/**
 * Timing for the welcome film: one continuous take of a day in the app. A Focus session is planned in the Calendar
 * ahead of another, runs through its first break, is paused for a while from the Focus ring, runs to its end, carries
 * on through the next event to the end of the day's Focus, then is seen among the week's other sessions as the view
 * pulls back. Every moment is derived from the milliseconds elapsed since the film started, so a frame is a pure
 * function of time and the reduced-motion still is simply the last frame.
 */

import { createPresetPomodoroConfig, type PomodoroPresetKey } from "$lib/pomodoro/rhythm";
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
  createEnd: 1300,
  createSaved: 1450,
  /** The current time reaches the session and Focus starts. */
  focusStart: 2700,
  /** The break screen covers the app for the length of the break. */
  breakStart: 4100,
  breakEnd: 5200,
  /**
   * A tap on the Focus ring opens its menu, and a tap on its first item pauses Focus. A while later the same two taps
   * resume it.
   */
  pauseTap: 5800,
  pauseStart: 6880,
  resumeTap: 8380,
  resumeAt: 9460,
  /** The drawn event ends, its finished screen shows, and the planned event carries on once it lifts. */
  eventEnd: 10960,
  eventOut: 11710,
  /** The planned event ends the day's Focus, the day's completed screen shows, and the app returns. */
  sessionEnd: 13560,
  finishedOut: 14410,
  /** The camera pulls back from the day to the week. */
  pullBack: 14660,
  end: 16960,
} as const;

/** How long a tap on the Focus ring or its menu takes, and how quickly the touch fades in and out. */
export const FILM_TAP_MS = 260;
const FILM_TAP_FADE_MS = 60;

/** How long the pull back from the day to the week takes. */
export const FILM_PULL_BACK_MS = 1200;

/** How long the planned event's breaks take to slide to where the rail replans them once the drawn event joins it. */
export const FILM_REPLAN_MS = 700;

/**
 * The film's session, in session minutes: a long Extended event drawn in the film, ending partway through its third focus,
 * then a Creative event already planned right after it. Back to back, they share one rail and the rhythm carries on
 * from one into the other, so the planned event's breaks move well ahead once the drawn event joins it. Both end on
 * the half hour or the hour, as people plan events.
 */
export const FILM_CHAIN: readonly FilmFocusEvent[] = [
  { start: 0, end: 150, preset: "extended" },
  { start: 150, end: 240, preset: "creative" },
];

/** The length of the event drawn in the film. */
export const FILM_DRAWN_MINUTES = FILM_CHAIN[0].end;

/** The session's length: both events, end to end. */
export const FILM_SESSION_MINUTES = FILM_CHAIN[FILM_CHAIN.length - 1].end;

/** The session's rail, planned by the Calendar's own rhythm across both events. */
export const FILM_RAIL: FilmRail = filmDayRails(FILM_CHAIN)[0];

/** The planned event's rail on its own, before the drawn event joins it. */
export const FILM_PLANNED_RAIL: FilmRail = filmDayRails(FILM_CHAIN.slice(1))[0];

/** The session's phases in order, in minutes. */
export const FILM_SESSION: readonly { kind: "focus" | "break"; minutes: number }[] = [
  ...FILM_RAIL.focus.map((span) => ({ kind: "focus" as const, span })),
  ...FILM_RAIL.breaks.map((span) => ({ kind: "break" as const, span })),
]
  .sort((left, right) => left.span.start - right.span.start)
  .map(({ kind, span }) => ({ kind, minutes: span.end - span.start }));

/** The session's breaks, which the Focus rail marks before they happen. */
export const FILM_BREAKS: readonly FilmBreak[] = FILM_RAIL.breaks;

/** How far into the focus after the first break Focus is paused, and for how long, in minutes. */
const FILM_PAUSE_INTO_FOCUS_MINUTES = 10;
const FILM_PAUSE_MINUTES = 5;

/** The manual pause, in session minutes: partway through the focus that follows the first break. */
export const FILM_PAUSE: FilmSpan = {
  start: (FILM_RAIL.focus[1]?.start ?? 0) + FILM_PAUSE_INTO_FOCUS_MINUTES,
  end: (FILM_RAIL.focus[1]?.start ?? 0) + FILM_PAUSE_INTO_FOCUS_MINUTES + FILM_PAUSE_MINUTES,
};

/**
 * The session's minutes against film time, as the current time reads them. Work hurries along, the break slows down
 * so its screen can be read and reaches its end exactly as the break screen lifts, then the time holds while the Focus
 * ring's menu is open, walks through the pause, and hurries to the drawn event's end, holds while its finished screen
 * is up, and hurries on through the planned event to the session's end.
 */
const FILM_BREAK: FilmSpan = FILM_BREAKS[0] ?? { start: FILM_SESSION_MINUTES, end: FILM_SESSION_MINUTES };
const SESSION_CLOCK: readonly FilmKeyframe[] = [
  [FILM_BEATS.focusStart + 200, 0],
  [FILM_BEATS.breakStart - 300, FILM_BREAK.start - 2],
  [FILM_BEATS.breakStart, FILM_BREAK.start],
  [FILM_BEATS.breakEnd, FILM_BREAK.end],
  [FILM_BEATS.pauseTap, FILM_PAUSE.start],
  [FILM_BEATS.pauseStart, FILM_PAUSE.start],
  [FILM_BEATS.resumeTap, FILM_PAUSE.end],
  [FILM_BEATS.resumeAt, FILM_PAUSE.end],
  [FILM_BEATS.eventEnd - 250, FILM_DRAWN_MINUTES - 2],
  [FILM_BEATS.eventEnd, FILM_DRAWN_MINUTES],
  [FILM_BEATS.eventOut, FILM_DRAWN_MINUTES],
  [FILM_BEATS.sessionEnd - 250, FILM_SESSION_MINUTES - 2],
  [FILM_BEATS.sessionEnd, FILM_SESSION_MINUTES],
];

/** How many minutes before the session the current time starts, so it can be seen walking up to it. */
export const FILM_LEAD_IN_MINUTES = 14;

/**
 * The current time against film time before Focus starts, in minutes relative to the session's start. It holds still
 * while the event is drawn and the rail replans, since drawing an event takes far less than a minute, then walks up
 * to the session.
 */
const LEAD_IN_CLOCK: readonly FilmKeyframe[] = [
  [0, -FILM_LEAD_IN_MINUTES],
  [FILM_BEATS.createSaved + FILM_REPLAN_MS, -FILM_LEAD_IN_MINUTES],
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

/** How many minutes Focus has spent paused once `minute` minutes of the session have passed. */
export function filmPausedMinutes(minute: number): number {
  return Math.min(FILM_PAUSE.end, Math.max(FILM_PAUSE.start, minute)) - FILM_PAUSE.start;
}

/**
 * The session's breaks once `minute` minutes of it have passed, as the running Focus projects them. A pause freezes
 * what is left of the focus while the current time moves on, so the drawn event's later breaks wait for it: they start
 * later by the minutes paused so far and stay clipped to the event. The planned event keeps the rhythm the Calendar
 * planned for it from the drawn event's schedule, so its breaks stay where they are.
 */
function runBreaks(minute: number): FilmSpan[] {
  const paused = filmPausedMinutes(minute);
  return FILM_BREAKS.flatMap(({ start, end }) => {
    if (start < FILM_PAUSE.start || start >= FILM_DRAWN_MINUTES) return [{ start, end }];
    if (start + paused >= FILM_DRAWN_MINUTES) return [];
    return [{ start: start + paused, end: Math.min(FILM_DRAWN_MINUTES, end + paused) }];
  });
}

/** The session's phases once `minute` minutes of it have passed: focus fills the room between the run's breaks. */
function runPhases(minute: number): { kind: "focus" | "break"; span: FilmSpan }[] {
  const phases: { kind: "focus" | "break"; span: FilmSpan }[] = [];
  let cursor = 0;
  for (const pause of runBreaks(minute)) {
    if (pause.start > cursor) phases.push({ kind: "focus", span: { start: cursor, end: pause.start } });
    phases.push({ kind: "break", span: pause });
    cursor = pause.end;
  }
  if (cursor < FILM_SESSION_MINUTES) phases.push({ kind: "focus", span: { start: cursor, end: FILM_SESSION_MINUTES } });
  return phases;
}

/** The pause so far once `minute` minutes of the session have passed, in session minutes. */
function pauseSoFar(minute: number): FilmSpan {
  return { start: FILM_PAUSE.start, end: FILM_PAUSE.start + filmPausedMinutes(minute) };
}

/** How many minutes `left` and `right` share. */
function overlap(left: FilmSpan, right: FilmSpan): number {
  return Math.max(0, Math.min(left.end, right.end) - Math.max(left.start, right.start));
}

/**
 * The Focus phase and its remaining time once `minute` minutes of the session have passed. A paused focus keeps its
 * remaining time while the current time moves on, and its length leaves the pause out.
 */
export function filmSessionMoment(minute: number): FilmSessionMoment {
  const pause = pauseSoFar(minute);
  for (const { kind, span } of runPhases(minute)) {
    if (minute < span.end) {
      const paused = kind === "focus" ? overlap(span, pause) : 0;
      return {
        kind,
        remainingSeconds: Math.max(0, Math.ceil((span.end - minute) * 60)),
        totalSeconds: Math.round((span.end - span.start - paused) * 60),
      };
    }
  }
  return { kind: "done" };
}

/** Whether Focus is paused at `elapsed`: from the tap that pauses it to the tap that resumes it. */
export function filmPaused(elapsed: number): boolean {
  return elapsed >= FILM_BEATS.pauseStart && elapsed < FILM_BEATS.resumeAt;
}

/** The Focus ring's menu at `elapsed`: closed, open with its pause item, or open with its resume item. */
export function filmRingMenu(elapsed: number): "pause" | "resume" | null {
  if (elapsed >= FILM_BEATS.pauseTap + FILM_TAP_MS && elapsed < FILM_BEATS.pauseStart) return "pause";
  if (elapsed >= FILM_BEATS.resumeTap + FILM_TAP_MS && elapsed < FILM_BEATS.resumeAt) return "resume";
  return null;
}

/** How visible the touch of a tap ending at `end` is at `elapsed`, from 0 to 1. */
export function filmTap(elapsed: number, end: number): number {
  const start = end - FILM_TAP_MS;
  return filmSpan(elapsed, start, start + FILM_TAP_FADE_MS) * (1 - filmSpan(elapsed, end - FILM_TAP_FADE_MS, end));
}

/**
 * The focus the session has recorded by `minute`, as the rail fills it in, in session minutes. The pause leaves a
 * gap in the focus it interrupted.
 */
export function filmFocusSpans(minute: number): FilmSpan[] {
  const pause = pauseSoFar(minute);
  return runPhases(minute).flatMap(({ kind, span }) => {
    if (kind !== "focus" || minute <= span.start) return [];
    const recorded = { start: span.start, end: Math.min(span.end, minute) };
    if (overlap(recorded, pause) === 0) return [recorded];
    return [
      { start: recorded.start, end: pause.start },
      { start: pause.end, end: recorded.end },
    ].filter((part) => part.end > part.start);
  });
}

/**
 * The session's breaks as the rail shows them at `elapsed`, in session minutes. Once the drawn event is saved, the
 * planned event's breaks slide from where they fell on its own rail to where the joined rail plans them, growing or
 * shrinking into their new lengths. The Calendar replans at once; the film eases the move so the change can be seen.
 * The drawn event's own breaks appear with it, and those after the pause move later with the current time while it
 * lasts, as the Calendar moves them.
 */
export function filmRailBreaks(elapsed: number): FilmSpan[] {
  const progress = easeInOutCubic(filmSpan(elapsed, FILM_BEATS.createSaved, FILM_BEATS.createSaved + FILM_REPLAN_MS));
  const breaks = runBreaks(filmClockMinute(elapsed));
  const drawn = breaks.filter((pause) => pause.start < FILM_DRAWN_MINUTES);
  const planned = breaks.filter((pause) => pause.start >= FILM_DRAWN_MINUTES).map((target, index) => {
    const from = FILM_PLANNED_RAIL.breaks[index] ?? target;
    return {
      start: from.start + (target.start - from.start) * progress,
      end: from.end + (target.end - from.end) * progress,
    };
  });
  return [...drawn, ...planned];
}

/** `Date.getDay()` for Sunday, Monday, and Friday. */
const SUNDAY = 0;
const MONDAY = 1;
const FRIDAY = 5;

/**
 * The day the film plays out on: `now`'s own day from Tuesday to Friday, or that week's Friday on a Saturday, Sunday,
 * or Monday, so the film's day always sits inside a Monday to Friday week with at least one day before it.
 */
export function filmWorkday(now: Date): Date {
  const weekday = now.getDay();
  // Weeks run Monday to Sunday, so Sunday counts as day seven.
  const dayOfWeek = weekday === SUNDAY ? 7 : weekday;
  const offset = dayOfWeek > MONDAY && dayOfWeek <= FRIDAY ? 0 : FRIDAY - dayOfWeek;
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() + offset);
}
