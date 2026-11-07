import { describe, expect, it } from "vitest";
import {
  DISTRACTIONS_BEATS,
  DISTRACTIONS_BLOCK_MINUTES_LEFT,
  DISTRACTIONS_BROWSER_MINUTES,
  DISTRACTIONS_FOCUS_HOST,
  DISTRACTIONS_LIMIT_HOST,
  DISTRACTIONS_LIMIT_MINUTES,
  DISTRACTIONS_PHONE_MINUTES,
  distractionsAddress,
  distractionsBrowserPage,
  distractionsFeedScroll,
  distractionsFocusRemaining,
  distractionsFocusing,
  distractionsLimitShown,
  distractionsLoading,
  distractionsNewTabOpen,
  distractionsNotification,
  distractionsPageIn,
  distractionsPan,
  distractionsPhoneApp,
  distractionsPulse,
  distractionsTouches,
  distractionsUsage,
} from "./welcome-distractions-scene";

const B = DISTRACTIONS_BEATS;

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(B.end / step) + 1 }, (_, index) => index * step);
}

describe("welcome distractions scene", () => {
  it("keeps its beats in order within the scene", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
    expect(beats[0]).toBeGreaterThan(0);
  });

  it("starts on the work tab during Focus with nothing open on the phone", () => {
    expect(distractionsBrowserPage(0)).toBe("work");
    expect(distractionsAddress(0)).toEqual({ kind: "placeholder" });
    expect(distractionsNewTabOpen(0)).toBe(false);
    expect(distractionsFocusing(0)).toBe(true);
    expect(distractionsPhoneApp(0)).toBeNull();
    expect(distractionsNotification(0)).toBeNull();
    expect(distractionsPan(0)).toBe(0);
    expect(distractionsLimitShown(0)).toBe(0);
    expect(distractionsUsage(0)).toEqual({ browser: 0, phone: 0 });
  });

  it("types the blocked site into a new tab, then hands the tab to the extension", () => {
    expect(distractionsNewTabOpen(B.newTabTap)).toBe(true);
    const typing = distractionsAddress((B.typeFocusStart + B.typeFocusEnd) / 2);
    expect(typing.kind).toBe("typed");
    if (typing.kind === "typed") {
      expect(DISTRACTIONS_FOCUS_HOST.startsWith(typing.text)).toBe(true);
      expect(typing.text.length).toBeGreaterThan(0);
      expect(typing.text.length).toBeLessThan(DISTRACTIONS_FOCUS_HOST.length);
    }
    expect(distractionsAddress(B.focusLoad)).toEqual({ kind: "typed", text: DISTRACTIONS_FOCUS_HOST, editing: false });
    expect(distractionsLoading(B.focusLoad)).not.toBeNull();
    expect(distractionsBrowserPage(B.focusBlocked)).toBe("focusBlock");
    expect(distractionsAddress(B.focusBlocked)).toEqual({ kind: "extension" });
    expect(distractionsLoading(B.focusBlocked)).toBeNull();
  });

  it("says the minutes left in Focus as the timer shows them when the site is blocked", () => {
    expect(DISTRACTIONS_BLOCK_MINUTES_LEFT).toBe(Math.ceil(distractionsFocusRemaining(B.focusBlocked) / 60));
    expect(DISTRACTIONS_BLOCK_MINUTES_LEFT).toBeGreaterThan(0);
  });

  it("closes the blocked tab back to the work tab", () => {
    expect(distractionsNewTabOpen(B.closeTap)).toBe(false);
    expect(distractionsBrowserPage(B.closeTap)).toBe("work");
    expect(distractionsAddress(B.closeTap)).toEqual({ kind: "placeholder" });
  });

  it("returns the phone home from the blocked app before the notification arrives", () => {
    expect(distractionsPhoneApp(B.egogramTap + 400)?.app).toBe("egogram");
    expect(distractionsPhoneApp(B.egogramHome + 1000)).toBeNull();
    expect(distractionsNotification(B.egogramNotice - 1)).toBeNull();
    expect(distractionsNotification(B.egogramNotice + 1000)).toEqual({ app: "egogram", reason: "focus", shown: 1 });
    expect(distractionsNotification(B.focusDone)).toBeNull();
  });

  it("pulses the Focus timer only right after each block", () => {
    expect(distractionsPulse(B.focusBlocked - 1)).toBe(0);
    expect(distractionsPulse(B.focusBlocked + 100)).toBeGreaterThan(0);
    expect(distractionsPulse(B.egogramHome + 100)).toBeGreaterThan(0);
    expect(distractionsPulse(B.focusDone)).toBe(0);
  });

  it("runs the Focus timer down to zero as the day's Focus ends", () => {
    let previous = Infinity;
    for (const elapsed of moments()) {
      const remaining = distractionsFocusRemaining(elapsed);
      expect(remaining).toBeLessThanOrEqual(previous);
      previous = remaining;
    }
    expect(distractionsFocusRemaining(B.focusDone)).toBe(0);
    expect(distractionsFocusing(B.focusDone)).toBe(false);
  });

  it("opens the limited site only after Focus, then blocks it as the limit is reached", () => {
    expect(distractionsLimitShown(B.limitShown)).toBe(1);
    expect(distractionsLimitShown(B.focusDone)).toBe(0);
    expect(distractionsAddress(B.typeLimitEnd)).toEqual({ kind: "typed", text: DISTRACTIONS_LIMIT_HOST, editing: true });
    expect(distractionsBrowserPage(B.limitPage)).toBe("feed");
    expect(distractionsBrowserPage(B.limitReached)).toBe("limitBlock");
    expect(distractionsAddress(B.limitReached)).toEqual({ kind: "extension" });
  });

  it("counts the limit's minutes across both devices until they fill the budget", () => {
    expect(DISTRACTIONS_BROWSER_MINUTES + DISTRACTIONS_PHONE_MINUTES).toBe(DISTRACTIONS_LIMIT_MINUTES);
    expect(distractionsUsage(B.limitPage)).toEqual({ browser: 0, phone: 0 });
    expect(distractionsUsage(B.browserUseEnd)).toEqual({ browser: DISTRACTIONS_BROWSER_MINUTES, phone: 0 });
    const reached = distractionsUsage(B.limitReached);
    expect(reached.browser + reached.phone).toBe(DISTRACTIONS_LIMIT_MINUTES);
    let previous = 0;
    for (const elapsed of moments()) {
      const usage = distractionsUsage(elapsed);
      const total = usage.browser + usage.phone;
      expect(total).toBeGreaterThanOrEqual(previous);
      expect(total).toBeLessThanOrEqual(DISTRACTIONS_LIMIT_MINUTES);
      if (elapsed < B.limitReached) expect(total).toBeLessThan(DISTRACTIONS_LIMIT_MINUTES);
      previous = total;
    }
  });

  it("keeps every animated value between 0 and 1", () => {
    for (const elapsed of moments()) {
      const values = [
        distractionsPan(elapsed),
        distractionsPulse(elapsed),
        distractionsLimitShown(elapsed),
        distractionsPageIn(elapsed),
        distractionsFeedScroll(elapsed, "browser"),
        distractionsFeedScroll(elapsed, "phone"),
        distractionsLoading(elapsed) ?? 0,
        distractionsPhoneApp(elapsed)?.open ?? 0,
        distractionsNotification(elapsed)?.shown ?? 0,
        ...Object.values(distractionsTouches(elapsed)),
      ];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("ends with the site and the app blocked on both devices, as the reduced-motion still", () => {
    expect(distractionsBrowserPage(B.end)).toBe("limitBlock");
    expect(distractionsPageIn(B.end)).toBe(1);
    expect(distractionsPhoneApp(B.end)).toBeNull();
    expect(distractionsNotification(B.end)).toEqual({ app: "y", reason: "limit", shown: 1 });
    expect(distractionsLimitShown(B.end)).toBe(1);
    expect(distractionsPan(B.end)).toBe(0);
    expect(Object.values(distractionsTouches(B.end)).every((value) => value === 0)).toBe(true);
  });
});
