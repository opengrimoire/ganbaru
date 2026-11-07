import { describe, expect, it } from "vitest";
import {
  LIMITS_BEATS,
  LIMITS_BROWSER_MINUTES,
  LIMITS_BUDGET_MINUTES,
  LIMITS_PHONE_MINUTES,
  limitsBrowserPage,
  limitsNotice,
  limitsPageIn,
  limitsPan,
  limitsPhoneApp,
  limitsUsage,
  limitsYTouch,
} from "./welcome-limits-scene";

const B = LIMITS_BEATS;

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(B.end / step) + 1 }, (_, index) => index * step);
}

describe("welcome limits scene", () => {
  it("keeps its beats in order", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
  });

  it("starts with Y open in the browser and nothing used", () => {
    expect(limitsBrowserPage(0)).toEqual({ kind: "feed", scroll: 0 });
    expect(limitsUsage(0)).toEqual({ browser: 0, phone: 0 });
    expect(limitsPhoneApp(0)).toBeNull();
    expect(limitsNotice(0)).toBeNull();
    expect(limitsPan(0)).toBe(0);
  });

  it("fills one budget from the browser, then the phone, reaching it exactly as the limit is reached", () => {
    expect(LIMITS_BROWSER_MINUTES + LIMITS_PHONE_MINUTES).toBe(LIMITS_BUDGET_MINUTES);
    expect(limitsUsage(B.browserUseEnd)).toEqual({ browser: LIMITS_BROWSER_MINUTES, phone: 0 });
    expect(limitsUsage(B.limitReached)).toEqual({ browser: LIMITS_BROWSER_MINUTES, phone: LIMITS_PHONE_MINUTES });
    let previous = 0;
    for (const elapsed of moments()) {
      const usage = limitsUsage(elapsed);
      const total = usage.browser + usage.phone;
      expect(total).toBeGreaterThanOrEqual(previous);
      if (elapsed < B.limitReached) expect(total).toBeLessThan(LIMITS_BUDGET_MINUTES);
      if (usage.phone > 0 && elapsed < B.limitReached) expect(limitsPhoneApp(elapsed)?.app).toBe("y");
      previous = total;
    }
  });

  it("blocks the site and the app together once the budget runs out", () => {
    expect(limitsBrowserPage(B.limitReached - 1).kind).toBe("feed");
    expect(limitsBrowserPage(B.limitReached)).toEqual({ kind: "block", reason: "limit" });
    expect(limitsPhoneApp(B.limitReached + 1000)).toBeNull();
    expect(limitsNotice(B.limitNotice - 1)).toBeNull();
    expect(limitsNotice(B.limitNotice + 1000)).toEqual({ app: "y", reason: "limit", shown: 1 });
  });

  it("keeps every animated value between 0 and 1", () => {
    for (const elapsed of moments()) {
      const page = limitsBrowserPage(elapsed);
      const values = [
        limitsPan(elapsed),
        limitsPageIn(elapsed),
        limitsYTouch(elapsed),
        page.kind === "feed" ? page.scroll : 0,
        limitsPhoneApp(elapsed)?.open ?? 0,
        limitsPhoneApp(elapsed)?.scroll ?? 0,
        limitsNotice(elapsed)?.shown ?? 0,
      ];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("ends on both devices blocked, as the reduced-motion still", () => {
    expect(limitsBrowserPage(B.end)).toEqual({ kind: "block", reason: "limit" });
    expect(limitsPageIn(B.end)).toBe(1);
    expect(limitsNotice(B.end)?.shown).toBe(1);
    expect(limitsPhoneApp(B.end)).toBeNull();
    expect(limitsPan(B.end)).toBe(0);
    expect(limitsYTouch(B.end)).toBe(0);
  });
});
