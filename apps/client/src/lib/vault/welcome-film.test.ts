import { describe, expect, it } from "vitest";
import {
  FILM_BEATS,
  FILM_BREAKS,
  FILM_CHAIN,
  FILM_DEFAULT_RAIL,
  FILM_DRAWN_MINUTES,
  FILM_LEAD_IN_MINUTES,
  FILM_PAUSE,
  FILM_PLANNED_RAIL,
  FILM_REPLAN_MS,
  FILM_SESSION,
  FILM_SESSION_MINUTES,
  FILM_SHEET_RISE_MS,
  FILM_TAP_MS,
  easeInOutCubic,
  filmClockMinute,
  filmCursorScript,
  filmDayRails,
  filmFocusSpans,
  filmPanelOpen,
  filmPaused,
  filmPausedMinutes,
  filmProjectPicker,
  filmRailBreaks,
  filmNotification,
  filmRingMenuOpen,
  filmSheetRise,
  filmSessionMinute,
  filmSessionMoment,
  filmSpan,
  filmTap,
  filmTypedTitle,
  filmWorkday,
  monotoneCurve,
} from "./welcome-film";
import { COUNT_PRESET_RHYTHMS } from "$lib/pomodoro/rhythm";
import { welcomeCursorFrame, welcomeCursorOn, welcomeCursorScriptOrdered } from "./welcome-cursor";

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

  it("holds the current time still while the event is drawn, set up, and saved", () => {
    for (let elapsed = 0; elapsed <= FILM_BEATS.createSaved; elapsed += 50) {
      expect(filmClockMinute(elapsed)).toBeCloseTo(-FILM_LEAD_IN_MINUTES, 9);
    }
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
    const focusRate = filmSessionMinute(FILM_BEATS.focusStart + 500) - filmSessionMinute(FILM_BEATS.focusStart + 400);
    expect(breakRate).toBeLessThan(focusRate / 3);
  });

  it("counts down each phase and finishes as done", () => {
    expect(filmSessionMoment(0)).toEqual({ kind: "focus", remainingSeconds: 3000, totalSeconds: 3000 });
    expect(filmSessionMoment(49.5)).toEqual({ kind: "focus", remainingSeconds: 30, totalSeconds: 3000 });
    expect(filmSessionMoment(50)).toEqual({ kind: "break", remainingSeconds: 600, totalSeconds: 600 });
    expect(filmSessionMoment(185)).toEqual({ kind: "break", remainingSeconds: 600, totalSeconds: 900 });
    expect(filmSessionMoment(FILM_SESSION_MINUTES)).toEqual({ kind: "done" });
  });

  it("draws an Extended event that ends during its third focus, then carries its rhythm into a Creative event", () => {
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
      { kind: "focus", minutes: FILM_SESSION_MINUTES - (FILM_BREAKS.at(-1)?.end ?? 0) },
    ]);
  });

  it("ends both events on the half hour or the hour", () => {
    for (const event of FILM_CHAIN) expect(event.end % 30).toBe(0);
  });

  it("keeps as many breaks in the planned event on its own rail as on the default and joined rails", () => {
    const planned = (breaks: readonly { start: number }[]) => breaks.filter((pause) => pause.start >= FILM_DRAWN_MINUTES).length;
    expect(planned(FILM_DEFAULT_RAIL.breaks)).toBe(FILM_PLANNED_RAIL.breaks.length);
    expect(planned(FILM_BREAKS)).toBe(FILM_PLANNED_RAIL.breaks.length);
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
    expect(FILM_BREAKS.filter((pause) => pause.long)).toEqual([{ start: 180, end: 195, long: true }]);
  });

  it("records focus up to the current minute and skips the break", () => {
    expect(filmFocusSpans(0)).toEqual([]);
    expect(filmFocusSpans(20)).toEqual([{ start: 0, end: 20 }]);
    expect(filmFocusSpans(55)).toEqual([{ start: 0, end: 50 }]);
    expect(filmFocusSpans(70)).toEqual([{ start: 0, end: 50 }, { start: 60, end: 70 }]);
  });

  it("replans the rail with the default rhythm once the drawn event joins it, then with the project's", () => {
    const spans = (breaks: readonly { start: number; end: number }[]) => breaks.map(({ start, end }) => ({ start, end }));
    expect(filmRailBreaks(FILM_BEATS.panelOpen)).toEqual(spans(FILM_PLANNED_RAIL.breaks));
    expect(filmRailBreaks(FILM_BEATS.projectPick)).toEqual(spans(FILM_DEFAULT_RAIL.breaks));
    expect(filmRailBreaks(FILM_BEATS.projectPick + FILM_REPLAN_MS)).toEqual(spans(FILM_BREAKS));
    expect(filmRailBreaks(FILM_BEATS.pauseStart)).toEqual(spans(FILM_BREAKS));
  });

  it("gives the drawn event a different rhythm by default than its project's", () => {
    const drawn = (breaks: readonly { start: number }[]) => breaks.filter((pause) => pause.start < FILM_DRAWN_MINUTES);
    expect(drawn(FILM_DEFAULT_RAIL.breaks).length).toBeGreaterThan(drawn(FILM_BREAKS).length);
    expect(FILM_DEFAULT_RAIL.breaks.map((pause) => pause.start)).not.toEqual(FILM_BREAKS.map((pause) => pause.start));
  });

  it("slides the planned event's breaks earlier through both replans and never leaves an empty break", () => {
    const planned = (elapsed: number) => filmRailBreaks(elapsed).filter((pause) => pause.start >= FILM_DRAWN_MINUTES);
    let previous = planned(FILM_BEATS.panelOpen);
    for (let elapsed = FILM_BEATS.panelOpen; elapsed <= FILM_BEATS.projectPick + FILM_REPLAN_MS; elapsed += 25) {
      const current = planned(elapsed);
      expect(current).toHaveLength(previous.length);
      current.forEach((pause, index) => {
        expect(pause.start).toBeLessThanOrEqual(previous[index].start + 1e-9);
      });
      for (const pause of filmRailBreaks(elapsed)) expect(pause.end).toBeGreaterThan(pause.start);
      previous = current;
    }
  });

  it("finishes both replans before the event is saved", () => {
    expect(FILM_BEATS.panelOpen + FILM_REPLAN_MS).toBeLessThan(FILM_BEATS.projectPick);
    expect(FILM_BEATS.projectPick + FILM_REPLAN_MS).toBeLessThan(FILM_BEATS.createSaved);
  });
});

describe("the film's event panel", () => {
  it("opens when the drawn event is let go and closes when it is saved", () => {
    expect(filmPanelOpen(FILM_BEATS.panelOpen - 1)).toBe(false);
    expect(filmPanelOpen(FILM_BEATS.panelOpen)).toBe(true);
    expect(filmPanelOpen(FILM_BEATS.createSaved - 1)).toBe(true);
    expect(filmPanelOpen(FILM_BEATS.createSaved)).toBe(false);
    expect(FILM_BEATS.createEnd).toBeLessThan(FILM_BEATS.panelOpen);
  });

  it("raises the phone's sheet to its peek as the panel opens, settling well before the first tap", () => {
    expect(filmSheetRise(FILM_BEATS.panelOpen)).toBe(0);
    const half = filmSheetRise(FILM_BEATS.panelOpen + FILM_SHEET_RISE_MS / 2);
    expect(half).toBeGreaterThan(0.5);
    expect(half).toBeLessThan(1);
    expect(filmSheetRise(FILM_BEATS.panelOpen + FILM_SHEET_RISE_MS)).toBe(1);
    expect(FILM_BEATS.panelOpen + FILM_SHEET_RISE_MS).toBeLessThan(FILM_BEATS.projectTap);
  });

  it("opens the project picker's groups, then a group's projects, and closes it on the pick", () => {
    expect(filmProjectPicker(FILM_BEATS.projectTap - 1)).toBeNull();
    expect(filmProjectPicker(FILM_BEATS.projectTap)).toBe("groups");
    expect(filmProjectPicker(FILM_BEATS.groupTap)).toBe("projects");
    expect(filmProjectPicker(FILM_BEATS.projectPick)).toBeNull();
    expect(FILM_BEATS.projectPick).toBeLessThan(FILM_BEATS.typeStart);
    expect(FILM_BEATS.typeEnd).toBeLessThan(FILM_BEATS.createSaved);
  });

  it("types the title a whole character at a time", () => {
    const title = "Écrire ✍️";
    expect(filmTypedTitle(FILM_BEATS.typeStart, title)).toBe("");
    expect(filmTypedTitle(FILM_BEATS.typeEnd, title)).toBe(title);
    let previous = "";
    for (let elapsed = FILM_BEATS.typeStart; elapsed <= FILM_BEATS.typeEnd; elapsed += 20) {
      const typed = filmTypedTitle(elapsed, title);
      expect(title.startsWith(typed)).toBe(true);
      expect(Array.from(typed).length - Array.from(previous).length).toBeLessThanOrEqual(1);
      previous = typed;
    }
  });
});

describe("the film's pause", () => {
  it("pauses ten minutes into the focus after the first break, for five minutes", () => {
    expect(FILM_PAUSE).toEqual({ start: FILM_BREAKS[0].end + 12.25, end: FILM_BREAKS[0].end + 17.25 });
    expect(FILM_PAUSE.end).toBeLessThan(FILM_BREAKS[1].start);
    expect(FILM_BEATS.breakEnd).toBeLessThan(FILM_BEATS.pauseTap);
    expect(FILM_BEATS.resumeAt).toBeLessThan(FILM_BEATS.eventEnd);
  });

  it("holds the current time while the ring's menu is open or the notification is read, and walks it through the pause", () => {
    for (let elapsed = FILM_BEATS.pauseTap; elapsed <= FILM_BEATS.pauseStart; elapsed += 50) {
      expect(filmSessionMinute(elapsed)).toBeCloseTo(FILM_PAUSE.start, 9);
    }
    for (let elapsed = FILM_BEATS.notificationIn; elapsed <= FILM_BEATS.resumeAt; elapsed += 50) {
      expect(filmSessionMinute(elapsed)).toBeCloseTo(FILM_PAUSE.end, 9);
    }
    expect(filmPausedMinutes(FILM_PAUSE.start - 1)).toBe(0);
    expect(filmPausedMinutes(FILM_PAUSE.start + 2)).toBe(2);
    expect(filmPausedMinutes(FILM_DRAWN_MINUTES)).toBe(FILM_PAUSE.end - FILM_PAUSE.start);
  });

  it("pauses from the ring's menu and resumes from the paused notification", () => {
    expect(filmRingMenuOpen(FILM_BEATS.pauseTap)).toBe(false);
    expect(filmRingMenuOpen(FILM_BEATS.pauseTap + FILM_TAP_MS)).toBe(true);
    expect(filmRingMenuOpen(FILM_BEATS.pauseStart)).toBe(false);
    expect(filmRingMenuOpen(FILM_BEATS.resumeAt)).toBe(false);
    expect(filmNotification(FILM_BEATS.notificationIn)).toBe(0);
    expect(filmNotification(FILM_BEATS.resumeAt - FILM_TAP_MS)).toBe(1);
    expect(filmNotification(FILM_BEATS.resumeAt)).toBe(1);
    expect(filmNotification(FILM_BEATS.eventEnd)).toBe(0);
    expect(filmNotification(FILM_BEATS.pauseStart)).toBe(0);
    expect(filmPaused(FILM_BEATS.pauseStart - 1)).toBe(false);
    expect(filmPaused(FILM_BEATS.pauseStart)).toBe(true);
    expect(filmPaused(FILM_BEATS.resumeAt - 1)).toBe(true);
    expect(filmPaused(FILM_BEATS.resumeAt)).toBe(false);
  });

  it("shows a touch only while its tap lasts", () => {
    const end = FILM_BEATS.pauseStart;
    expect(filmTap(end - FILM_TAP_MS - 1, end)).toBe(0);
    expect(filmTap(end - FILM_TAP_MS / 2, end)).toBe(1);
    expect(filmTap(end, end)).toBe(0);
  });

  it("keeps the focus's remaining time while paused and leaves the pause out of its length", () => {
    const left = (FILM_BREAKS[1].start - FILM_PAUSE.start) * 60;
    const total = (FILM_BREAKS[1].start - FILM_BREAKS[0].end) * 60;
    for (const minute of [FILM_PAUSE.start, FILM_PAUSE.start + 2.5, FILM_PAUSE.end]) {
      expect(filmSessionMoment(minute)).toEqual({ kind: "focus", remainingSeconds: left, totalSeconds: total });
    }
    expect(filmSessionMoment(FILM_PAUSE.end + 5)).toEqual({ kind: "focus", remainingSeconds: left - 300, totalSeconds: total });
  });

  it("starts the next break later by the pause and shortens the drawn event's last focus to its end", () => {
    const paused = FILM_PAUSE.end - FILM_PAUSE.start;
    const breakStart = FILM_BREAKS[1].start + paused;
    expect(filmSessionMoment(breakStart - 1).kind).toBe("focus");
    expect(filmSessionMoment(breakStart)).toEqual({ kind: "break", remainingSeconds: 600, totalSeconds: 600 });
    const lastFocus = FILM_DRAWN_MINUTES - FILM_BREAKS[1].end - paused;
    expect(filmSessionMoment(FILM_BREAKS[1].end + paused)).toEqual({
      kind: "focus", remainingSeconds: lastFocus * 60, totalSeconds: lastFocus * 60,
    });
  });

  it("leaves a gap in the recorded focus while paused", () => {
    const before = [{ start: 0, end: 50 }, { start: 60, end: FILM_PAUSE.start }];
    expect(filmFocusSpans(FILM_PAUSE.start + 3)).toEqual(before);
    expect(filmFocusSpans(FILM_PAUSE.end)).toEqual(before);
    expect(filmFocusSpans(FILM_PAUSE.end + 5)).toEqual([...before, { start: FILM_PAUSE.end, end: FILM_PAUSE.end + 5 }]);
    expect(filmFocusSpans(130)).toEqual([...before, { start: FILM_PAUSE.end, end: 115 }, { start: 125, end: 130 }]);
  });

  it("moves the drawn event's next break later while paused and leaves the planned event's breaks in place", () => {
    const planned = (elapsed: number) => filmRailBreaks(elapsed).filter((pause) => pause.start >= FILM_DRAWN_MINUTES);
    let previous = filmRailBreaks(FILM_BEATS.pauseStart)[1].start;
    expect(previous).toBe(FILM_BREAKS[1].start);
    for (let elapsed = FILM_BEATS.pauseStart; elapsed <= FILM_BEATS.notificationIn; elapsed += 25) {
      const current = filmRailBreaks(elapsed);
      expect(current[0].start).toBe(FILM_BREAKS[0].start);
      expect(current[1].start).toBeGreaterThanOrEqual(previous - 1e-9);
      expect(current[1].end - current[1].start).toBeCloseTo(FILM_BREAKS[1].end - FILM_BREAKS[1].start, 9);
      expect(planned(elapsed)).toEqual(planned(FILM_BEATS.pauseStart));
      previous = current[1].start;
    }
    expect(previous).toBeCloseTo(FILM_BREAKS[1].start + FILM_PAUSE.end - FILM_PAUSE.start, 9);
    expect(filmRailBreaks(FILM_BEATS.end)).toEqual(filmRailBreaks(FILM_BEATS.notificationIn));
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

describe("filmWorkday", () => {
  /** The week of Monday 2026-11-02, in local time. */
  const day = (date: number, hour = 10) => new Date(2026, 10, date, hour);

  it("keeps Tuesday to Friday as they are", () => {
    for (const date of [3, 4, 5, 6]) expect(filmWorkday(day(date))).toEqual(new Date(2026, 10, date));
  });

  it("moves Saturday, Sunday, and Monday to that week's Friday", () => {
    expect(filmWorkday(day(7))).toEqual(new Date(2026, 10, 6));
    expect(filmWorkday(day(8, 23))).toEqual(new Date(2026, 10, 6));
    expect(filmWorkday(day(2, 0))).toEqual(new Date(2026, 10, 6));
  });

  it("crosses a month boundary", () => {
    expect(filmWorkday(new Date(2026, 10, 30))).toEqual(new Date(2026, 11, 4));
  });
});

describe("the film's desktop pointer", () => {
  const script = filmCursorScript();
  const stop = (target: string) => {
    const found = script.find((entry) => entry.target === target);
    if (!found) throw new Error(`no stop for ${target}`);
    return found;
  };
  /** Each control the pointer clicks, the beat its tap lands on, and the beat that draws the control. */
  const clicks = [
    { target: "panel-project", tap: FILM_BEATS.projectTap, shown: FILM_BEATS.panelOpen },
    { target: "panel-group", tap: FILM_BEATS.groupTap, shown: FILM_BEATS.projectTap },
    { target: "panel-project-pick", tap: FILM_BEATS.projectPick, shown: FILM_BEATS.groupTap },
    { target: "panel-save", tap: FILM_BEATS.createSaved, shown: FILM_BEATS.panelOpen },
    { target: "film-ring", tap: FILM_BEATS.pauseTap + FILM_TAP_MS, shown: FILM_BEATS.breakEnd },
    { target: "film-ring-pause", tap: FILM_BEATS.pauseStart, shown: FILM_BEATS.pauseTap + FILM_TAP_MS },
    { target: "notification-action-0", tap: FILM_BEATS.resumeAt, shown: FILM_BEATS.notificationIn },
  ];

  it("goes from place to place in order", () => {
    expect(welcomeCursorScriptOrdered(script)).toBe(true);
  });

  it("releases each click exactly as the tap it replaces lands, after holding it for a tap's length", () => {
    for (const { target, tap } of clicks) expect(stop(target).press).toEqual({ start: tap - FILM_TAP_MS, end: tap });
  });

  it("rests on each control from the moment its button goes down until its tap lands", () => {
    for (const { target, tap } of clicks) {
      expect(welcomeCursorOn(script, tap - FILM_TAP_MS, target)).toBe(true);
      expect(welcomeCursorOn(script, tap, target)).toBe(true);
    }
  });

  it("leaves for each control only once it is drawn", () => {
    for (const { target, shown } of clicks) expect(stop(target).depart).toBeGreaterThanOrEqual(shown);
  });

  it("holds the button through the drag, following the drawn event's edge until it is let go", () => {
    const drag = stop("film-drag");
    expect(drag).toMatchObject({ follow: true, depart: FILM_BEATS.createStart, arrive: FILM_BEATS.createEnd });
    expect(drag.press).toEqual({ start: FILM_BEATS.createStart, end: FILM_BEATS.createEnd });
    expect(stop("film-slot").arrive).toBeLessThan(FILM_BEATS.createStart);
    expect(welcomeCursorFrame(script, (FILM_BEATS.createStart + FILM_BEATS.createEnd) / 2).pressed).toBe(1);
  });

  it("shows the text beam in the title while it is typed", () => {
    for (const elapsed of [FILM_BEATS.typeStart, FILM_BEATS.typeEnd]) {
      expect(welcomeCursorOn(script, elapsed, "panel-title")).toBe(true);
      expect(welcomeCursorFrame(script, elapsed).glyph).toBe("text");
    }
  });

  it("hovers the ring menu's pause item while the menu is open", () => {
    expect(filmRingMenuOpen(stop("film-ring-pause").arrive)).toBe(true);
  });

  it("presses nothing outside the drag and the clicks", () => {
    expect(welcomeCursorFrame(script, FILM_BEATS.focusStart).pressed).toBe(0);
    expect(welcomeCursorFrame(script, FILM_BEATS.end).pressed).toBe(0);
    expect(welcomeCursorFrame(script, FILM_BEATS.end).to.target).toBe("film-grid");
  });
});
