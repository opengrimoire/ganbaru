import { describe, expect, it } from "vitest";
import {
  LIMITS_BEATS,
  LIMITS_BROWSER_MINUTES,
  LIMITS_BUDGET_MINUTES,
  LIMITS_CURSOR,
  LIMITS_PHONE_MINUTES,
  limitsBrowser,
  limitsEgogramTouch,
  limitsNotice,
  limitsPan,
  limitsPhoneApp,
  limitsUsage,
} from "./welcome-limits-scene";
import { BLOCKER_CURSOR, blockerCursorClose } from "./welcome-blocker";
import { WELCOME_CURSOR_TRAVEL_MS, welcomeCursorHovered, welcomeCursorScriptOrdered } from "./welcome-cursor";
import { FILM_TAP_MS } from "./welcome-film";

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

  it("starts with Y open beside the inbox and nothing used", () => {
    const browser = limitsBrowser(0);
    expect(browser.page).toEqual({ kind: "feed", app: "y", scroll: 0 });
    expect(browser.tabs.map((tab) => [tab.content, tab.active])).toEqual([
      ["mail", false],
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

  it("closes Y's tab back to the inbox before Egogram opens on the phone", () => {
    const closed = limitsBrowser(B.yCloseTap);
    expect(closed.tabs).toEqual([{ content: "mail", active: true }]);
    expect(closed.page).toEqual({ kind: "mail" });
    expect(closed.address).toEqual({ kind: "host", host: "qnail.com" });
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
      ["mail", false],
      ["blocked", true],
    ]);

    const typing = limitsBrowser(B.blueditTypeEnd);
    expect(typing.address).toMatchObject({ kind: "typed", text: "bluedit.com" });
    expect(typing.page).toEqual({ kind: "block", app: "y", reason: "limit" });
    expect(limitsBrowser(B.blueditLoadStart + 100).page).toEqual({ kind: "block", app: "y", reason: "limit" });
    expect(limitsBrowser(B.blueditBlocked).page).toEqual({ kind: "block", app: "bluedit", reason: "limit" });
    expect(limitsBrowser(B.blueditBlocked).tabs).toHaveLength(2);
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
    expect(browser.page).toEqual({ kind: "block", app: "bluedit", reason: "limit" });
    expect(browser.pageIn).toBe(1);
    expect(browser.loading).toBe(0);
    expect(limitsNotice(B.end)?.shown).toBe(1);
    expect(limitsPhoneApp(B.end)).toBeNull();
    expect(limitsPan(B.end)).toBe(0);
    expect(limitsEgogramTouch(B.end)).toBe(0);
  });

  it("moves the browser's pointer in order and lands each click exactly on its tap", () => {
    expect(welcomeCursorScriptOrdered(LIMITS_CURSOR)).toBe(true);
    const clicks = LIMITS_CURSOR.filter((stop) => stop.press);
    expect(clicks.map((stop) => [stop.target, stop.press?.end])).toEqual([
      [blockerCursorClose("y"), B.yCloseTap],
      [BLOCKER_CURSOR.newTab, B.newTabTap],
      [BLOCKER_CURSOR.address, B.addressTap],
    ]);
    for (const click of clicks) {
      expect(welcomeCursorHovered(LIMITS_CURSOR, click.press?.start ?? 0)).toBe(click.target);
      expect(welcomeCursorHovered(LIMITS_CURSOR, click.press?.end ?? 0)).toBe(click.target);
    }
  });

  it("leaves the feed alone while it scrolls, and comes back to the laptop only as the camera does", () => {
    const close = LIMITS_CURSOR.find((stop) => stop.target === blockerCursorClose("y"));
    const newTab = LIMITS_CURSOR.find((stop) => stop.target === BLOCKER_CURSOR.newTab);
    expect(close?.depart).toBeGreaterThanOrEqual(B.browserUseEnd);
    expect(newTab?.depart).toBeGreaterThanOrEqual(B.browserReturn);
    expect(welcomeCursorHovered(LIMITS_CURSOR, B.browserUseEnd - 1)).toBe(BLOCKER_CURSOR.page);
    expect(welcomeCursorHovered(LIMITS_CURSOR, B.yTypeEnd)).toBe(BLOCKER_CURSOR.page);
    expect(limitsPan(B.browserReturn)).toBe(1);
    expect(limitsPan(B.newTabTap)).toBe(0);
  });

  it("leaves past the screen's right edge toward the phone before the camera turns to it, and glides back in from there", () => {
    const index = LIMITS_CURSOR.findIndex((stop) => stop.target === BLOCKER_CURSOR.toPhone);
    const exit = LIMITS_CURSOR[index];
    expect(LIMITS_CURSOR.filter((stop) => stop.target === BLOCKER_CURSOR.toPhone)).toHaveLength(1);
    expect(exit.depart).toBeGreaterThanOrEqual(B.yCloseTap);
    expect(limitsPan(exit.depart)).toBe(0);
    expect(limitsPan(exit.arrive)).toBe(0);
    expect(exit.arrive).toBeLessThan(B.egogramTap - FILM_TAP_MS);
    const back = LIMITS_CURSOR[index + 1];
    expect(back.target).toBe(BLOCKER_CURSOR.newTab);
    expect(back.arrive - back.depart).toBe(WELCOME_CURSOR_TRAVEL_MS);
  });
});
