/**
 * Timing for the welcome film: one continuous take of a day in the app. A work session is planned in the Calendar,
 * focused on through a blocked distraction and a break, then counted toward its project as the view pulls back to
 * the week. Every moment is derived from the milliseconds elapsed since the film started, so a frame is a pure
 * function of time and the reduced-motion still is simply the last frame.
 */

/** One point of a curve: `[x, y]`, with x strictly increasing across a curve. */
export type FilmKeyframe = readonly [x: number, y: number];

/** The three chapters the film is told in, shown under it as it plays. */
export type FilmAct = "plan" | "focus" | "progress";

/** Moments of the film, in milliseconds from its start. */
export const FILM_BEATS = {
  /** A press draws the first session onto the empty day. */
  createStart: 600,
  createEnd: 1250,
  /** The session's Focus rail appears along its left edge. */
  railIn: 1500,
  reviewIn: 1750,
  lunchIn: 1950,
  /** The current time reaches the session and the Focus panel appears. */
  focusStart: 2500,
  /** A distracting app starts to open, is blocked, and falls away. */
  distractionIn: 3850,
  distractionOpen: 4050,
  distractionBlocked: 4300,
  distractionOut: 4800,
  sessionDone: 6650,
  /** The panel turns into the project the session belongs to. */
  projectIn: 7100,
  /** The finished session flies into the project's progress. */
  sparkStart: 7300,
  sparkLand: 7950,
  /** The camera pulls back from the day to the week. */
  pullBack: 8350,
  end: 9700,
} as const;

/** How long the pull back from the day to the week takes. */
export const FILM_PULL_BACK_MS = 1200;

/** Where each chapter starts; each runs until the next one starts, and the last until the end. */
const ACT_STARTS: readonly (readonly [FilmAct, number])[] = [
  ["plan", 0],
  ["focus", FILM_BEATS.focusStart],
  ["progress", FILM_BEATS.projectIn],
];

/** The session's phases in minutes, as Focus runs them: work, a break, then work again. */
export const FILM_SESSION: readonly { kind: "focus" | "break"; minutes: number }[] = [
  { kind: "focus", minutes: 50 },
  { kind: "break", minutes: 10 },
  { kind: "focus", minutes: 50 },
];

/** The session's length in minutes. */
export const FILM_SESSION_MINUTES = FILM_SESSION.reduce((total, phase) => total + phase.minutes, 0);

/**
 * The session's minutes against film time. Time runs fast, slows almost to a stop while the distracting app is
 * blocked, slows again so the break can be read, then hurries to the end of the session.
 */
const SESSION_CLOCK: readonly FilmKeyframe[] = [
  [2800, 0],
  [3900, 28],
  [4500, 31],
  [5050, 50],
  [5650, 60],
  [6600, FILM_SESSION_MINUTES],
];

/** What the Focus panel shows at one moment of the session. */
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

/** The session's minutes since it started at a moment of the film, from 0 to the session's length. */
export function filmSessionMinute(elapsed: number): number {
  return monotoneCurve(SESSION_CLOCK, elapsed);
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

/** The break spans of the session that have begun by `minute`, clipped to it, in session minutes. */
export function filmBreakSpans(minute: number): { start: number; end: number }[] {
  const spans: { start: number; end: number }[] = [];
  let start = 0;
  for (const phase of FILM_SESSION) {
    const end = start + phase.minutes;
    if (phase.kind === "break" && minute > start) spans.push({ start, end: Math.min(end, minute) });
    start = end;
  }
  return spans;
}

/** The chapter playing at `elapsed`, and how far through it the film is, from 0 to 1. */
export function filmAct(elapsed: number): { act: FilmAct; progress: number } {
  let index = 0;
  while (index < ACT_STARTS.length - 1 && elapsed >= ACT_STARTS[index + 1][1]) index += 1;
  const [act, start] = ACT_STARTS[index];
  const end = index < ACT_STARTS.length - 1 ? ACT_STARTS[index + 1][1] : FILM_BEATS.end;
  return { act, progress: filmSpan(elapsed, start, end) };
}

/** Formats seconds as a countdown, `m:ss` under an hour. */
export function formatFilmCountdown(seconds: number): string {
  const whole = Math.max(0, Math.round(seconds));
  const minutes = Math.floor(whole / 60);
  return `${minutes}:${String(whole % 60).padStart(2, "0")}`;
}
