import { describe, expect, it } from "vitest";
import {
  BLOCKING_BEATS,
  BLOCKING_DAY_VIEW,
  BLOCKING_EVENT,
  BLOCKING_RAIL,
  BLOCKING_STILL,
  blockingBrowser,
  blockingBrowserPage,
  blockingClock,
  blockingEgogramTouch,
  blockingFocusing,
  blockingFocusRemaining,
  blockingNotice,
  blockingPan,
  blockingPhase,
  blockingPhoneApp,
  blockingPulse,
  blockingRailRecorded,
} from "./welcome-blocking-scene";

const B = BLOCKING_BEATS;

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(B.end / step) + 1 }, (_, index) => index * step);
}

describe("welcome blocking scene", () => {
  it("keeps its beats in order", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
  });

  it("walks the current time across the event within the strip", () => {
    let previous = -Infinity;
    for (const elapsed of moments()) {
      const clock = blockingClock(elapsed);
      expect(clock).toBeGreaterThanOrEqual(previous);
      expect(clock).toBeGreaterThan(BLOCKING_DAY_VIEW.start);
      expect(clock).toBeLessThan(BLOCKING_DAY_VIEW.end);
      previous = clock;
    }
    expect(blockingClock(0)).toBeLessThan(BLOCKING_EVENT.start);
    expect(blockingClock(B.focusStart)).toBe(BLOCKING_EVENT.start);
    expect(blockingClock(B.focusEnd)).toBe(BLOCKING_EVENT.end);
    expect(blockingClock(B.end)).toBeGreaterThan(BLOCKING_EVENT.end);
  });

  it("runs Focus exactly while the current time is inside the event", () => {
    for (const elapsed of moments()) {
      const clock = blockingClock(elapsed);
      expect(blockingFocusing(elapsed)).toBe(clock >= BLOCKING_EVENT.start && clock < BLOCKING_EVENT.end);
    }
  });

  it("leaves the site open before and after the event, and blocks it during", () => {
    expect(blockingBrowserPage(0)).toMatchObject({ kind: "feed", app: "bluedit" });
    expect(blockingBrowserPage(B.focusStart - 1).kind).toBe("feed");
    expect(blockingBrowserPage(B.focusStart)).toEqual({
      kind: "block",
      app: "bluedit",
      reason: "focus",
      minutesLeft: BLOCKING_EVENT.end - BLOCKING_EVENT.start,
    });
    expect(blockingBrowserPage(B.focusEnd - 1)).toMatchObject({ kind: "block", app: "y", reason: "focus" });
    expect(blockingBrowserPage(B.focusEnd)).toMatchObject({ kind: "feed", app: "y" });
    expect(blockingBrowserPage(B.end).kind).toBe("feed");
  });

  it("blocks Y's address typed into the browser during the event", () => {
    expect(blockingBrowser(B.addressTap - 100).addressTouch).toBeGreaterThan(0);
    expect(blockingBrowser(B.addressTap).address).toMatchObject({ kind: "typed", text: "" });
    const typed = blockingBrowser(B.yTypeEnd);
    expect(typed.address).toMatchObject({ kind: "typed", text: "y.com" });
    expect(typed.page).toMatchObject({ kind: "block", app: "bluedit" });
    expect(typed.tabs).toEqual([{ content: "blocked", active: true }]);
    const loading = blockingBrowser(B.yLoadStart + 100);
    expect(loading.address).toEqual({ kind: "host", host: "y.com" });
    expect(loading.loading).toBeGreaterThan(0);
    expect(loading.page).toMatchObject({ kind: "block", app: "bluedit" });
    const blocked = blockingBrowser(B.yBlocked);
    expect(blocked.page).toMatchObject({ kind: "block", app: "y", reason: "focus" });
    expect(blocked.address).toEqual({ kind: "extension" });
    expect(blocked.loading).toBe(0);
    expect(blockingFocusing(B.yBlocked)).toBe(true);
  });

  it("says the minutes left in Focus as the timer rounds them", () => {
    for (const elapsed of moments()) {
      const page = blockingBrowserPage(elapsed);
      if (page.kind === "block" && page.reason === "focus") {
        expect(page.minutesLeft).toBe(Math.ceil(blockingFocusRemaining(elapsed) / 60));
        expect(page.minutesLeft).toBeGreaterThan(0);
      }
    }
    expect(blockingFocusRemaining(0)).toBe((BLOCKING_EVENT.end - BLOCKING_EVENT.start) * 60);
    expect(blockingFocusRemaining(B.focusEnd)).toBe(0);
  });

  it("sends Egogram Home during the event and lets it open after", () => {
    expect(blockingPhoneApp(B.egogramTap + 400)?.app).toBe("egogram");
    expect(blockingFocusing(B.egogramTap)).toBe(true);
    expect(blockingPhoneApp(B.egogramHome + 1000)).toBeNull();
    expect(blockingNotice(B.egogramNotice + 1000)).toEqual({ app: "egogram", reason: "focus", shown: 1 });
    for (const [tap, home] of [
      [B.retryTap, B.retryHome],
      [B.retryAgainTap, B.retryAgainHome],
    ] as const) {
      expect(blockingFocusing(home)).toBe(true);
      expect(blockingEgogramTouch(tap - 100)).toBeGreaterThan(0);
      expect(blockingPhoneApp(tap - 1)).toBeNull();
      expect(blockingPhoneApp(tap + 300)).toMatchObject({ app: "egogram", scroll: 0 });
      expect(blockingPhoneApp(home + 400)).toBeNull();
    }
    expect(blockingNotice(B.retryNotice + 1000)?.shown).toBe(1);
    expect(blockingNotice(B.focusEnd)).toBeNull();
    expect(blockingFocusing(B.egogramAgainTap)).toBe(false);
    expect(blockingPhoneApp(B.end)).toMatchObject({ app: "egogram", open: 1 });
    expect(blockingNotice(B.end)).toBeNull();
  });

  it("fills the event's rail with focus as the current time crosses it, around its planned breaks", () => {
    expect(BLOCKING_RAIL).toMatchObject({ start: BLOCKING_EVENT.start, end: BLOCKING_EVENT.end });
    expect(BLOCKING_RAIL.breaks.length).toBeGreaterThan(0);
    expect(blockingRailRecorded(0)).toEqual([]);
    expect(blockingRailRecorded(B.focusStart)).toEqual([]);
    const now = blockingClock(B.retryAgainHome);
    expect(blockingRailRecorded(B.retryAgainHome)).toEqual([{ start: BLOCKING_EVENT.start, end: now }]);
    expect(blockingRailRecorded(B.end)).toEqual(BLOCKING_RAIL.focus);
    let previous = 0;
    for (const elapsed of moments()) {
      const recorded = blockingRailRecorded(elapsed);
      const minutes = recorded.reduce((sum, span) => sum + span.end - span.start, 0);
      expect(minutes).toBeGreaterThanOrEqual(previous);
      for (const span of recorded) {
        expect(BLOCKING_RAIL.breaks.some((pause) => span.start < pause.end && span.end > pause.start)).toBe(false);
      }
      previous = minutes;
    }
  });

  it("pulses the event only as its Focus starts", () => {
    expect(blockingPulse(B.focusStart - 1)).toBe(0);
    expect(blockingPulse(B.focusStart + 100)).toBeGreaterThan(0);
    for (const beat of [B.yBlocked, B.egogramHome, B.retryHome, B.retryAgainHome, B.focusEnd]) {
      expect(blockingPulse(beat + 100)).toBe(0);
    }
  });

  it("drains the Focus ring through each focus and break on its own", () => {
    expect(blockingPhase(B.focusStart - 1)).toBeNull();
    expect(blockingPhase(B.focusEnd)).toBeNull();
    const [first] = BLOCKING_RAIL.focus;
    expect(blockingPhase(B.focusStart)).toEqual({ remainingSeconds: (first.end - first.start) * 60, totalSeconds: (first.end - first.start) * 60 });
    for (const elapsed of moments()) {
      const phase = blockingPhase(elapsed);
      expect(phase !== null).toBe(blockingFocusing(elapsed));
      if (!phase) continue;
      const spans = [...BLOCKING_RAIL.focus, ...BLOCKING_RAIL.breaks];
      expect(spans.some((span) => (span.end - span.start) * 60 === phase.totalSeconds)).toBe(true);
      expect(phase.remainingSeconds).toBeGreaterThan(0);
      expect(phase.remainingSeconds).toBeLessThanOrEqual(phase.totalSeconds);
    }
    const pause = BLOCKING_RAIL.breaks[0];
    const inBreak = moments(1).find((elapsed) => blockingClock(elapsed) >= pause.start && blockingClock(elapsed) < pause.end);
    expect(inBreak).toBeDefined();
    expect(blockingPhase(inBreak ?? 0)?.totalSeconds).toBe((pause.end - pause.start) * 60);
  });

  it("keeps every animated value between 0 and 1", () => {
    for (const elapsed of moments()) {
      const browser = blockingBrowser(elapsed);
      const page = browser.page;
      expect(browser.tabs).toHaveLength(1);
      const values = [
        blockingPan(elapsed),
        blockingPulse(elapsed),
        browser.pageIn,
        browser.loading,
        browser.addressTouch,
        blockingEgogramTouch(elapsed),
        page.kind === "feed" ? page.scroll : 0,
        blockingPhoneApp(elapsed)?.open ?? 0,
        blockingPhoneApp(elapsed)?.scroll ?? 0,
        blockingNotice(elapsed)?.shown ?? 0,
      ];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("shows both devices blocked in its still", () => {
    expect(BLOCKING_STILL).toBeLessThan(B.end);
    expect(blockingBrowserPage(BLOCKING_STILL)).toMatchObject({ kind: "block", reason: "focus" });
    expect(blockingBrowser(BLOCKING_STILL)).toMatchObject({ pageIn: 1, loading: 0, address: { kind: "extension" } });
    expect(blockingNotice(BLOCKING_STILL)?.shown).toBe(1);
    expect(blockingPhoneApp(BLOCKING_STILL)).toBeNull();
    expect(blockingPulse(BLOCKING_STILL)).toBe(0);
  });
});
