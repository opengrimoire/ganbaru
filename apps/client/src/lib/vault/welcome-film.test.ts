import { describe, expect, it } from "vitest";
import {
  FILM_BEATS,
  FILM_BREAKS,
  FILM_LEAD_IN_MINUTES,
  FILM_SESSION_MINUTES,
  easeInOutCubic,
  filmAct,
  filmClockMinute,
  filmFocusSpans,
  filmSessionMinute,
  filmSessionMoment,
  filmSpan,
  monotoneCurve,
} from "./welcome-film";

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
    expect(filmSessionMinute(FILM_BEATS.breakStart)).toBeCloseTo(50, 9);
    expect(filmSessionMinute(FILM_BEATS.breakEnd)).toBeCloseTo(60, 9);
  });

  it("slows down during the break so its countdown can be read", () => {
    const breakRate = filmSessionMinute(FILM_BEATS.breakStart + 600) - filmSessionMinute(FILM_BEATS.breakStart + 500);
    const focusRate = filmSessionMinute(3100) - filmSessionMinute(3000);
    expect(breakRate).toBeLessThan(focusRate / 3);
  });

  it("counts down each phase and finishes as done", () => {
    expect(filmSessionMoment(0)).toEqual({ kind: "focus", remainingSeconds: 3000, totalSeconds: 3000 });
    expect(filmSessionMoment(49.5)).toEqual({ kind: "focus", remainingSeconds: 30, totalSeconds: 3000 });
    expect(filmSessionMoment(50)).toEqual({ kind: "break", remainingSeconds: 600, totalSeconds: 600 });
    expect(filmSessionMoment(65)).toEqual({ kind: "focus", remainingSeconds: 2700, totalSeconds: 3000 });
    expect(filmSessionMoment(FILM_SESSION_MINUTES)).toEqual({ kind: "done" });
  });

  it("marks its break before it happens", () => {
    expect(FILM_BREAKS).toEqual([{ start: 50, end: 60 }]);
  });

  it("records focus up to the current minute and skips the break", () => {
    expect(filmFocusSpans(0)).toEqual([]);
    expect(filmFocusSpans(30)).toEqual([{ start: 0, end: 30 }]);
    expect(filmFocusSpans(55)).toEqual([{ start: 0, end: 50 }]);
    expect(filmFocusSpans(80)).toEqual([{ start: 0, end: 50 }, { start: 60, end: 80 }]);
  });
});

describe("filmAct", () => {
  it("moves through plan, focus, and progress", () => {
    expect(filmAct(0)).toEqual({ act: "plan", progress: 0 });
    expect(filmAct(FILM_BEATS.focusStart / 2).act).toBe("plan");
    expect(filmAct(FILM_BEATS.focusStart)).toEqual({ act: "focus", progress: 0 });
    expect(filmAct(FILM_BEATS.pullBack).act).toBe("progress");
    expect(filmAct(FILM_BEATS.end)).toEqual({ act: "progress", progress: 1 });
  });
});
