import { describe, expect, it } from "vitest";
import {
  BLOCKING_BEATS,
  BLOCKING_DAY_VIEW,
  BLOCKING_EVENT,
  BLOCKING_STILL,
  blockingBrowserPage,
  blockingClock,
  blockingEgogramTouch,
  blockingFocusing,
  blockingFocusRemaining,
  blockingNotice,
  blockingPageIn,
  blockingPan,
  blockingPhoneApp,
  blockingPulse,
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
    expect(blockingBrowserPage(0).kind).toBe("feed");
    expect(blockingBrowserPage(B.focusStart - 1).kind).toBe("feed");
    expect(blockingBrowserPage(B.focusStart)).toEqual({ kind: "block", reason: "focus", minutesLeft: 60 });
    expect(blockingBrowserPage(B.focusEnd - 1)).toMatchObject({ kind: "block", reason: "focus" });
    expect(blockingBrowserPage(B.focusEnd).kind).toBe("feed");
    expect(blockingBrowserPage(B.end).kind).toBe("feed");
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
    expect(blockingNotice(B.focusEnd)).toBeNull();
    expect(blockingFocusing(B.egogramAgainTap)).toBe(false);
    expect(blockingPhoneApp(B.end)).toMatchObject({ app: "egogram", open: 1 });
    expect(blockingNotice(B.end)).toBeNull();
  });

  it("pulses the event only right after each block", () => {
    expect(blockingPulse(B.focusStart - 1)).toBe(0);
    expect(blockingPulse(B.focusStart + 100)).toBeGreaterThan(0);
    expect(blockingPulse(B.egogramHome + 100)).toBeGreaterThan(0);
    expect(blockingPulse(B.focusEnd)).toBe(0);
  });

  it("keeps every animated value between 0 and 1", () => {
    for (const elapsed of moments()) {
      const page = blockingBrowserPage(elapsed);
      const values = [
        blockingPan(elapsed),
        blockingPulse(elapsed),
        blockingPageIn(elapsed),
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
    expect(blockingPageIn(BLOCKING_STILL)).toBe(1);
    expect(blockingNotice(BLOCKING_STILL)?.shown).toBe(1);
    expect(blockingPhoneApp(BLOCKING_STILL)).toBeNull();
    expect(blockingPulse(BLOCKING_STILL)).toBe(0);
  });
});
