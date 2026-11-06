import { describe, expect, it } from "vitest";
import {
  FILM_BEATS,
  FILM_BREAKS,
  FILM_FOCUS_PRESET,
  FILM_LEAD_IN_MINUTES,
  FILM_SESSION,
  FILM_SESSION_MINUTES,
  easeInOutCubic,
  filmClockMinute,
  filmFocusSpans,
  filmSessionMinute,
  filmSessionMoment,
  filmSpan,
  monotoneCurve,
} from "./welcome-film";
import { COUNT_PRESET_RHYTHMS } from "$lib/pomodoro/rhythm";

describe("monotoneCurve", () => {
  const points = [[0, 0], [100, 10], [200, 12], [300, 40]] as const;

  it("passes through every keyframe", () => {
    for (const [x, y] of points) expect(monotoneCurve(points, x)).toBeCloseTo(y, 9);
  });

  it("holds the first and last values outside the curve", () => {
    expect(monotoneCurve(points, -50)).toBe(0);
    expect(monotoneCurve(points, 999)).toBe(40);
  });

  it("never runs backwards or overshoots between keyframes", () => {
    let previous = monotoneCurve(points, 0);
    for (let x = 1; x <= 300; x += 1) {
      const value = monotoneCurve(points, x);
      expect(value).toBeGreaterThanOrEqual(previous - 1e-9);
      expect(value).toBeLessThanOrEqual(40 + 1e-9);
      previous = value;
    }
  });

  it("stays flat across a flat segment", () => {
    const plateau = [[0, 0], [10, 5], [20, 5], [30, 9]] as const;
    for (let x = 10; x <= 20; x += 1) expect(monotoneCurve(plateau, x)).toBeCloseTo(5, 9);
  });

  it("starts and ends at rest", () => {
    expect(monotoneCurve(points, 1) - monotoneCurve(points, 0)).toBeLessThan(0.01);
    expect(monotoneCurve(points, 300) - monotoneCurve(points, 299)).toBeLessThan(0.01);
  });

  it("returns 0 for an empty curve", () => {
    expect(monotoneCurve([], 5)).toBe(0);
  });
});

describe("filmSpan and easing", () => {
  it("clamps progress through a span", () => {
    expect(filmSpan(0, 100, 200)).toBe(0);
    expect(filmSpan(150, 100, 200)).toBe(0.5);
    expect(filmSpan(400, 100, 200)).toBe(1);
  });

  it("treats an empty span as a step", () => {
    expect(filmSpan(99, 100, 100)).toBe(0);
    expect(filmSpan(100, 100, 100)).toBe(1);
  });

  it("eases symmetrically between 0 and 1", () => {
    expect(easeInOutCubic(0)).toBe(0);
    expect(easeInOutCubic(0.5)).toBe(0.5);
    expect(easeInOutCubic(1)).toBe(1);
    expect(easeInOutCubic(0.25) + easeInOutCubic(0.75)).toBeCloseTo(1, 9);
  });
});

describe("the film's session", () => {
  it("is still before Focus starts and complete when the session ends", () => {
    expect(filmSessionMinute(FILM_BEATS.focusStart)).toBe(0);
    expect(filmSessionMinute(FILM_BEATS.sessionEnd)).toBe(FILM_SESSION_MINUTES);
  });

  it("walks the current time up to the session as Focus starts, then follows it", () => {
    expect(filmClockMinute(0)).toBe(-FILM_LEAD_IN_MINUTES);
    expect(filmClockMinute(FILM_BEATS.focusStart)).toBe(0);
    let previous = filmClockMinute(0);
    for (let elapsed = 0; elapsed <= FILM_BEATS.end; elapsed += 25) {
      const minute = filmClockMinute(elapsed);
      expect(minute).toBeGreaterThanOrEqual(previous);
      previous = minute;
    }
    expect(filmClockMinute(FILM_BEATS.breakEnd)).toBe(filmSessionMinute(FILM_BEATS.breakEnd));
    expect(filmClockMinute(FILM_BEATS.end)).toBe(FILM_SESSION_MINUTES);
  });

  it("spans the break exactly while the break screen is up", () => {
    expect(filmSessionMinute(FILM_BEATS.breakStart)).toBeCloseTo(FILM_BREAKS[0].start, 9);
    expect(filmSessionMinute(FILM_BEATS.breakEnd)).toBeCloseTo(FILM_BREAKS[0].end, 9);
  });

  it("slows down during the break so its countdown can be read", () => {
    const breakRate = filmSessionMinute(FILM_BEATS.breakStart + 600) - filmSessionMinute(FILM_BEATS.breakStart + 500);
    const focusRate = filmSessionMinute(3100) - filmSessionMinute(3000);
    expect(breakRate).toBeLessThan(focusRate / 3);
  });

  it("counts down each phase and finishes as done", () => {
    expect(filmSessionMoment(0)).toEqual({ kind: "focus", remainingSeconds: 1500, totalSeconds: 1500 });
    expect(filmSessionMoment(24.5)).toEqual({ kind: "focus", remainingSeconds: 30, totalSeconds: 1500 });
    expect(filmSessionMoment(25)).toEqual({ kind: "break", remainingSeconds: 300, totalSeconds: 300 });
    expect(filmSessionMoment(35)).toEqual({ kind: "focus", remainingSeconds: 900, totalSeconds: 1200 });
    expect(filmSessionMoment(FILM_SESSION_MINUTES)).toEqual({ kind: "done" });
  });

  it("runs the creative preset's rhythm and ends on focus", () => {
    const creative = COUNT_PRESET_RHYTHMS[FILM_FOCUS_PRESET];
    expect(FILM_FOCUS_PRESET).toBe("creative");
    expect(FILM_SESSION).toEqual([
      { kind: "focus", minutes: creative.focusDurationMinutes },
      { kind: "break", minutes: creative.shortBreakMinutes },
      { kind: "focus", minutes: FILM_SESSION_MINUTES - creative.focusDurationMinutes - creative.shortBreakMinutes },
    ]);
  });

  it("marks its break before it happens", () => {
    expect(FILM_BREAKS).toEqual([{ start: 25, end: 30 }]);
  });

  it("records focus up to the current minute and skips the break", () => {
    expect(filmFocusSpans(0)).toEqual([]);
    expect(filmFocusSpans(20)).toEqual([{ start: 0, end: 20 }]);
    expect(filmFocusSpans(28)).toEqual([{ start: 0, end: 25 }]);
    expect(filmFocusSpans(40)).toEqual([{ start: 0, end: 25 }, { start: 30, end: 40 }]);
  });
});
