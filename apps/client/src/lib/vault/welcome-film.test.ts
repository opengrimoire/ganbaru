import { describe, expect, it } from "vitest";
import {
  FILM_BEATS,
  FILM_BREAKS,
  FILM_CHAIN,
  FILM_DRAWN_MINUTES,
  FILM_LEAD_IN_MINUTES,
  FILM_PLANNED_RAIL,
  FILM_REPLAN_MS,
  FILM_SESSION,
  FILM_SESSION_MINUTES,
  easeInOutCubic,
  filmClockMinute,
  filmDayRails,
  filmFocusSpans,
  filmRailBreaks,
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
      expect(minute).toBeGreaterThanOrEqual(previous - 1e-9);
      previous = minute;
    }
    expect(filmClockMinute(FILM_BEATS.breakEnd)).toBe(filmSessionMinute(FILM_BEATS.breakEnd));
    expect(filmClockMinute(FILM_BEATS.end)).toBe(FILM_SESSION_MINUTES);
  });

  it("spans the break exactly while the break screen is up", () => {
    expect(filmSessionMinute(FILM_BEATS.breakStart)).toBeCloseTo(FILM_BREAKS[0].start, 9);
    expect(filmSessionMinute(FILM_BEATS.breakEnd)).toBeCloseTo(FILM_BREAKS[0].end, 9);
  });

  it("holds at the drawn event's end while its finished screen is up", () => {
    expect(filmSessionMinute(FILM_BEATS.eventEnd)).toBeCloseTo(FILM_DRAWN_MINUTES, 9);
    expect(filmSessionMinute((FILM_BEATS.eventEnd + FILM_BEATS.eventOut) / 2)).toBeCloseTo(FILM_DRAWN_MINUTES, 9);
    expect(filmSessionMinute(FILM_BEATS.eventOut)).toBeCloseTo(FILM_DRAWN_MINUTES, 9);
    expect(filmSessionMinute(FILM_BEATS.eventOut + 400)).toBeGreaterThan(FILM_DRAWN_MINUTES);
  });

  it("finishes the drawn event after its first break and before the session ends", () => {
    expect(FILM_BEATS.eventEnd).toBeGreaterThan(FILM_BEATS.breakEnd);
    expect(FILM_BEATS.eventOut).toBeLessThan(FILM_BEATS.sessionEnd);
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
    expect(filmSessionMoment(200)).toEqual({ kind: "break", remainingSeconds: 600, totalSeconds: 900 });
    expect(filmSessionMoment(FILM_SESSION_MINUTES)).toEqual({ kind: "done" });
  });

  it("draws an Extended event that ends just short of a break, then carries its rhythm into a long Creative event", () => {
    const extended = COUNT_PRESET_RHYTHMS.extended;
    const creative = COUNT_PRESET_RHYTHMS.creative;
    expect(FILM_CHAIN.map((event) => event.preset)).toEqual(["extended", "creative"]);
    const lastFocus = FILM_DRAWN_MINUTES - 2 * (extended.focusDurationMinutes + extended.shortBreakMinutes);
    expect(lastFocus).toBeGreaterThan(0);
    expect(lastFocus).toBeLessThan(extended.focusDurationMinutes);
    expect(FILM_SESSION.slice(0, 5)).toEqual([
      { kind: "focus", minutes: extended.focusDurationMinutes },
      { kind: "break", minutes: extended.shortBreakMinutes },
      { kind: "focus", minutes: extended.focusDurationMinutes },
      { kind: "break", minutes: extended.shortBreakMinutes },
      { kind: "focus", minutes: lastFocus },
    ]);
    expect(FILM_SESSION.slice(5)).toEqual([
      { kind: "break", minutes: creative.shortBreakMinutes },
      { kind: "focus", minutes: creative.focusDurationMinutes },
      { kind: "break", minutes: creative.longBreakMinutes },
      { kind: "focus", minutes: creative.focusDurationMinutes },
      { kind: "break", minutes: creative.shortBreakMinutes },
      { kind: "focus", minutes: creative.focusDurationMinutes },
      { kind: "break", minutes: creative.shortBreakMinutes },
      { kind: "focus", minutes: FILM_SESSION_MINUTES - 270 },
    ]);
  });

  it("moves the planned event's breaks once the drawn event joins its rail", () => {
    const planned = FILM_PLANNED_RAIL.breaks.map((pause) => pause.start);
    const joined = FILM_BREAKS.filter((pause) => pause.start >= FILM_DRAWN_MINUTES).map((pause) => pause.start);
    expect(planned).not.toEqual(joined);
    expect(joined[0]).toBe(FILM_DRAWN_MINUTES);
    expect(planned[0] - FILM_DRAWN_MINUTES).toBeGreaterThan(0);
  });

  it("marks its breaks before they happen, with one long break", () => {
    expect(FILM_BREAKS[0]).toEqual({ start: 50, end: 60, long: false });
    expect(FILM_BREAKS.filter((pause) => pause.long)).toEqual([{ start: 195, end: 210, long: true }]);
  });

  it("records focus up to the current minute and skips the break", () => {
    expect(filmFocusSpans(0)).toEqual([]);
    expect(filmFocusSpans(20)).toEqual([{ start: 0, end: 20 }]);
    expect(filmFocusSpans(55)).toEqual([{ start: 0, end: 50 }]);
    expect(filmFocusSpans(70)).toEqual([{ start: 0, end: 50 }, { start: 60, end: 70 }]);
  });

  it("slides the planned event's breaks earlier into the joined rail's plan once the drawn event is saved", () => {
    const settled = FILM_BEATS.createSaved + FILM_REPLAN_MS;
    const drawnBreaks = FILM_BREAKS.filter((pause) => pause.start < FILM_DRAWN_MINUTES).length;
    const plannedPart = (elapsed: number) => filmRailBreaks(elapsed).slice(drawnBreaks);
    expect(plannedPart(FILM_BEATS.createSaved)).toEqual(FILM_PLANNED_RAIL.breaks.map(({ start, end }) => ({ start, end })));
    expect(filmRailBreaks(settled)).toEqual(FILM_BREAKS.map(({ start, end }) => ({ start, end })));
    expect(filmRailBreaks(FILM_BEATS.end)).toEqual(filmRailBreaks(settled));
    let previous = plannedPart(FILM_BEATS.createSaved);
    for (let elapsed = FILM_BEATS.createSaved; elapsed <= settled; elapsed += 25) {
      const current = plannedPart(elapsed);
      current.forEach((pause, index) => {
        expect(pause.start).toBeLessThanOrEqual(previous[index].start + 1e-9);
        expect(pause.end).toBeGreaterThan(pause.start);
      });
      previous = current;
    }
  });

  it("finishes the replan before Focus starts", () => {
    expect(FILM_BEATS.createSaved + FILM_REPLAN_MS).toBeLessThan(FILM_BEATS.focusStart);
  });
});

describe("filmDayRails", () => {
  /** Balanced then Creative, back to back from nine: the second event inherits the third rhythm position. */
  const chain = [
    { start: 540, end: 610, preset: "balanced" },
    { start: 610, end: 705, preset: "creative" },
  ] as const;

  it("shares one rail between back-to-back events and keeps separate events apart", () => {
    const rails = filmDayRails([...chain, { start: 720, end: 750, preset: "creative" }]);
    expect(rails.map(({ start, end }) => ({ start, end }))).toEqual([{ start: 540, end: 705 }, { start: 720, end: 750 }]);
  });

  it("carries the rhythm across presets, so the fourth focus of the chain ends in a long break", () => {
    const [rail] = filmDayRails(chain);
    expect(rail.breaks).toEqual([
      { start: 570, end: 575, long: false },
      { start: 605, end: 610, long: false },
      { start: 635, end: 640, long: false },
      { start: 665, end: 680, long: true },
    ]);
    const [alone] = filmDayRails([chain[1]]);
    expect(alone.breaks.some((pause) => pause.long)).toBe(false);
  });

  it("fills the rail with focus everywhere a break is not", () => {
    for (const rail of filmDayRails(chain)) {
      const pieces = [...rail.focus, ...rail.breaks].sort((left, right) => left.start - right.start);
      expect(pieces[0].start).toBe(rail.start);
      expect(pieces.at(-1)?.end).toBe(rail.end);
      pieces.slice(1).forEach((piece, index) => expect(piece.start).toBe(pieces[index].end));
    }
  });
});
