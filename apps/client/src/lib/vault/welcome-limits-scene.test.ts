import { describe, expect, it } from "vitest";
import {
  LIMITS_BEATS,
  LIMITS_BROWSER_MINUTES,
  LIMITS_BUDGET_MINUTES,
  LIMITS_PHONE_MINUTES,
  limitsBrowser,
  limitsEgogramTouch,
  limitsNotice,
  limitsPan,
  limitsPhoneApp,
  limitsUsage,
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

  it("starts with Y open beside a work tab and nothing used", () => {
    const browser = limitsBrowser(0);
    expect(browser.page).toEqual({ kind: "feed", app: "y", scroll: 0 });
    expect(browser.tabs.map((tab) => [tab.content, tab.active])).toEqual([
      ["work", false],
      ["y", true],
    ]);
    expect(browser.address).toEqual({ kind: "host", host: "y.com" });
    expect(limitsUsage(0)).toEqual({ browser: 0, phone: 0 });
    expect(limitsPhoneApp(0)).toBeNull();
    expect(limitsNotice(0)).toBeNull();
    expect(limitsPan(0)).toBe(0);
  });

  it("fills one budget from Y in the browser, then Egogram on the phone, reaching it exactly as the limit is reached", () => {
    expect(LIMITS_BROWSER_MINUTES + LIMITS_PHONE_MINUTES).toBe(LIMITS_BUDGET_MINUTES);
    expect(limitsUsage(B.browserUseEnd)).toEqual({ browser: LIMITS_BROWSER_MINUTES, phone: 0 });
    expect(limitsUsage(B.limitReached)).toEqual({ browser: LIMITS_BROWSER_MINUTES, phone: LIMITS_PHONE_MINUTES });
    let previous = 0;
    for (const elapsed of moments()) {
      const usage = limitsUsage(elapsed);
      const total = usage.browser + usage.phone;
      expect(total).toBeGreaterThanOrEqual(previous);
      if (elapsed < B.limitReached) expect(total).toBeLessThan(LIMITS_BUDGET_MINUTES);
      if (usage.browser > 0 && usage.browser < LIMITS_BROWSER_MINUTES) expect(limitsBrowser(elapsed).page.kind).toBe("feed");
      if (usage.phone > 0 && elapsed < B.limitReached) expect(limitsPhoneApp(elapsed)?.app).toBe("egogram");
      previous = total;
    }
  });

  it("closes Y's tab before Egogram opens on the phone", () => {
    expect(limitsBrowser(B.yCloseTap - 100).tabs[1]?.closeTouch).toBeGreaterThan(0);
    const closed = limitsBrowser(B.yCloseTap);
    expect(closed.tabs).toEqual([{ content: "work", active: true }]);
    expect(closed.page).toEqual({ kind: "work" });
    expect(closed.address).toEqual({ kind: "placeholder" });
    expect(B.yCloseTap).toBeLessThan(B.egogramTap);
    for (const elapsed of moments()) {
      if (limitsPhoneApp(elapsed)) expect(limitsBrowser(elapsed).tabs.some((tab) => tab.content === "y")).toBe(false);
    }
  });

  it("sends Egogram Home with the limit notification once the budget runs out", () => {
    expect(limitsPhoneApp(B.limitReached - 1)?.app).toBe("egogram");
    expect(limitsPhoneApp(B.limitReached + 1000)).toBeNull();
    expect(limitsNotice(B.limitNotice - 1)).toBeNull();
    expect(limitsNotice(B.limitNotice + 1000)).toEqual({ app: "egogram", reason: "limit", shown: 1 });
  });

  it("sends Egogram Home again each time it is opened after the limit, without using more of the budget", () => {
    for (const [tap, home] of [
      [B.retryTap, B.retryHome],
      [B.retryAgainTap, B.retryAgainHome],
    ] as const) {
      expect(limitsEgogramTouch(tap - 100)).toBeGreaterThan(0);
      expect(limitsPhoneApp(tap - 1)).toBeNull();
      expect(limitsPhoneApp(tap + 300)).toMatchObject({ app: "egogram", scroll: 0 });
      expect(limitsPhoneApp(home + 400)).toBeNull();
      expect(limitsUsage(home + 400)).toEqual(limitsUsage(B.limitReached));
    }
    expect(limitsNotice(B.retryTap - 1)).toBeNull();
    expect(limitsNotice(B.retryNotice + 1000)?.shown).toBe(1);
    expect(limitsNotice(B.retryAgainHome + 400)?.shown).toBe(1);
  });

  it("types each site of the limit into the browser after it ran out, and loads its block page", () => {
    expect(B.newTabTap).toBeGreaterThan(B.retryAgainHome);
    expect(limitsBrowser(B.newTabTap - 100).newTabTouch).toBeGreaterThan(0);
    expect(limitsBrowser(B.newTabTap).address).toMatchObject({ kind: "typed", text: "" });
    expect(limitsBrowser(B.yTypeStart + (B.yTypeEnd - B.yTypeStart) / 2).address).toMatchObject({ kind: "typed", text: "y." });
    expect(limitsBrowser(B.yTypeEnd).address).toMatchObject({ kind: "typed", text: "y.com" });
    const loading = limitsBrowser(B.yLoadStart + 100);
    expect(loading.page.kind).toBe("blank");
    expect(loading.loading).toBeGreaterThan(0);
    const blocked = limitsBrowser(B.yBlocked);
    expect(blocked.page).toEqual({ kind: "block", app: "y", reason: "limit" });
    expect(blocked.address).toEqual({ kind: "extension" });
    expect(blocked.tabs.map((tab) => [tab.content, tab.active])).toEqual([
      ["work", false],
      ["blocked", true],
    ]);

    expect(limitsBrowser(B.addressTap - 100).addressTouch).toBeGreaterThan(0);
    const typing = limitsBrowser(B.egogramTypeEnd);
    expect(typing.address).toMatchObject({ kind: "typed", text: "egogram.com" });
    expect(typing.page).toEqual({ kind: "block", app: "y", reason: "limit" });
    expect(limitsBrowser(B.egogramLoadStart + 100).page).toEqual({ kind: "block", app: "y", reason: "limit" });
    expect(limitsBrowser(B.egogramBlocked).page).toEqual({ kind: "block", app: "egogram", reason: "limit" });
    expect(limitsBrowser(B.egogramBlocked).tabs).toHaveLength(2);
  });

  it("keeps a single active tab and every animated value between 0 and 1", () => {
    for (const elapsed of moments()) {
      const browser = limitsBrowser(elapsed);
      expect(browser.tabs.filter((tab) => tab.active)).toHaveLength(1);
      const values = [
        limitsPan(elapsed),
        limitsEgogramTouch(elapsed),
        browser.pageIn,
        browser.loading,
        browser.newTabTouch,
        browser.addressTouch,
        ...browser.tabs.map((tab) => tab.closeTouch ?? 0),
        browser.page.kind === "feed" ? browser.page.scroll : 0,
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
    const browser = limitsBrowser(B.end);
    expect(browser.page).toEqual({ kind: "block", app: "egogram", reason: "limit" });
    expect(browser.pageIn).toBe(1);
    expect(browser.loading).toBe(0);
    expect(browser.newTabTouch).toBe(0);
    expect(limitsNotice(B.end)?.shown).toBe(1);
    expect(limitsPhoneApp(B.end)).toBeNull();
    expect(limitsPan(B.end)).toBe(0);
    expect(limitsEgogramTouch(B.end)).toBe(0);
  });
});
