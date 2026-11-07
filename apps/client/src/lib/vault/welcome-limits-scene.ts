/**
 * Timing for the welcome scene on daily limits: one limit links Y's website and its phone app, and counts time from
 * both devices. Y's feed scrolls in a laptop's browser, then in the app on a phone, until the day's budget is used up;
 * then the app returns Home with a notification and the extension swaps the browser's tab for its block page.
 */

import {
  BLOCKER_APP_OPEN_MS,
  blockerAppOpen,
  blockerNoticeShown,
  blockerPageIn,
  blockerPan,
  type BlockerBrowserPage,
  type BlockerNotice,
  type BlockerPanMove,
  type BlockerPhoneApp,
} from "./welcome-blocker";
import { easeInOutCubic, filmSpan, filmTap } from "./welcome-film";

/** The daily limit's budget, and how much of it each device uses, in minutes. */
export const LIMITS_BUDGET_MINUTES = 60;
export const LIMITS_BROWSER_MINUTES = 40;
export const LIMITS_PHONE_MINUTES = LIMITS_BUDGET_MINUTES - LIMITS_BROWSER_MINUTES;

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const LIMITS_BEATS = {
  /** Y's feed scrolls in the browser, using part of the budget. */
  browserUseStart: 500,
  browserUseEnd: 4300,
  /** A tap opens Y on the phone, which uses the rest of the budget. */
  yTap: 5600,
  /** The budget runs out: the app returns Home, its notification arrives, and the browser's tab is blocked. */
  limitReached: 9000,
  limitNotice: 9350,
  end: 12000,
} as const;

const B = LIMITS_BEATS;

/** Where the camera points on a narrow canvas: the phone for the app, then the browser for its block page. */
const PAN_MOVES: readonly BlockerPanMove[] = [
  { start: 4700, to: 1 },
  { start: 10300, to: 0 },
];

/** When the phone's use of the budget starts, once the app has opened. */
const PHONE_USE_START = B.yTap + BLOCKER_APP_OPEN_MS;

/** Where the camera points on a narrow canvas, from 0 (the browser) to 1 (the phone). */
export function limitsPan(elapsed: number): number {
  return blockerPan(elapsed, PAN_MOVES);
}

/** The limit's minutes used so far on each device; together they reach the budget exactly as the limit is reached. */
export function limitsUsage(elapsed: number): { browser: number; phone: number } {
  return {
    browser: LIMITS_BROWSER_MINUTES * filmSpan(elapsed, B.browserUseStart, B.browserUseEnd),
    phone: LIMITS_PHONE_MINUTES * filmSpan(elapsed, PHONE_USE_START, B.limitReached),
  };
}

/** What the browser's tab shows at this moment. */
export function limitsBrowserPage(elapsed: number): BlockerBrowserPage {
  if (elapsed >= B.limitReached) return { kind: "block", reason: "limit" };
  return { kind: "feed", scroll: easeInOutCubic(filmSpan(elapsed, B.browserUseStart, B.browserUseEnd)) };
}

/** How far the browser's page has faded in since it last changed, from 0 to 1. */
export function limitsPageIn(elapsed: number): number {
  return blockerPageIn(elapsed, [B.limitReached]);
}

/** The app open on the phone at this moment, or null on the home screen. */
export function limitsPhoneApp(elapsed: number): BlockerPhoneApp | null {
  const open = blockerAppOpen(elapsed, B.yTap, B.limitReached);
  if (open <= 0) return null;
  return { app: "y", open, scroll: easeInOutCubic(filmSpan(elapsed, PHONE_USE_START + 200, B.limitReached - 100)) };
}

/** The phone's block notification at this moment, or null. */
export function limitsNotice(elapsed: number): BlockerNotice | null {
  const shown = blockerNoticeShown(elapsed, B.limitNotice);
  return shown > 0 ? { app: "y", reason: "limit", shown } : null;
}

/** The tap on Y's icon, from 0 to 1 as its touch shows. */
export function limitsYTouch(elapsed: number): number {
  return filmTap(elapsed, B.yTap);
}
