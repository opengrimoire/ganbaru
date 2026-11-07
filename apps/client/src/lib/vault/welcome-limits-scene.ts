/**
 * Timing for the welcome scene on daily limits: one social media limit groups Y's website and Egogram's phone app, and
 * counts time from both devices. Y's feed scrolls in a laptop's browser beside a work tab until Y's tab is closed;
 * then Egogram opened on a phone uses the rest of the day's budget, returns Home, and posts the limit notification.
 * Back on the laptop, Y typed in a new tab opens the extension's block page instead.
 */

import {
  BLOCKER_APP_HOSTS,
  BLOCKER_APP_OPEN_MS,
  blockerAppOpen,
  blockerCaretOn,
  blockerNoticeShown,
  blockerPageIn,
  blockerPan,
  blockerTyped,
  type BlockerBrowserFrame,
  type BlockerBrowserPage,
  type BlockerNotice,
  type BlockerPanMove,
  type BlockerPhoneApp,
  type BlockerTab,
} from "./welcome-blocker";
import { easeInOutCubic, filmSpan, filmTap } from "./welcome-film";

/** The daily limit's budget, and how much of it each device uses, in minutes. */
export const LIMITS_BUDGET_MINUTES = 60;
export const LIMITS_BROWSER_MINUTES = 40;
export const LIMITS_PHONE_MINUTES = LIMITS_BUDGET_MINUTES - LIMITS_BROWSER_MINUTES;

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const LIMITS_BEATS = {
  /** Y's feed scrolls in the browser, using part of the budget. */
  browserUseStart: 400,
  browserUseEnd: 3800,
  /** A tap closes Y's tab, so the work tab shows. */
  yCloseTap: 4300,
  /** A tap opens Egogram on the phone, which uses the rest of the budget. */
  egogramTap: 5800,
  /** The budget runs out: Egogram returns Home and its notification arrives. */
  limitReached: 9200,
  limitNotice: 9550,
  /** A tap opens a new tab in the browser, Y's address is typed, and the page loads into the block page. */
  newTabTap: 11300,
  typeStart: 11600,
  typeEnd: 12100,
  loadStart: 12300,
  blocked: 12650,
  end: 14600,
} as const;

const B = LIMITS_BEATS;

/** Where the camera points on a narrow canvas: the phone for Egogram, then the browser for the new tab. */
const PAN_MOVES: readonly BlockerPanMove[] = [
  { start: 4900, to: 1 },
  { start: 10400, to: 0 },
];

/** When the phone's use of the budget starts, once the app has opened. */
const PHONE_USE_START = B.egogramTap + BLOCKER_APP_OPEN_MS;

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

/** The active tab's page at this moment. */
function browserPage(elapsed: number): BlockerBrowserPage {
  if (elapsed < B.yCloseTap) return { kind: "feed", app: "y", scroll: easeInOutCubic(filmSpan(elapsed, B.browserUseStart, B.browserUseEnd)) };
  if (elapsed < B.newTabTap) return { kind: "work" };
  if (elapsed < B.blocked) return { kind: "blank" };
  return { kind: "block", app: "y", reason: "limit" };
}

/** The browser at this moment: a work tab beside Y's tab until it is closed, then a new tab that Y's address loads into. */
export function limitsBrowser(elapsed: number): BlockerBrowserFrame {
  const page = browserPage(elapsed);
  const pageIn = blockerPageIn(elapsed, [B.yCloseTap, B.newTabTap, B.blocked]);
  const newTabTouch = filmTap(elapsed, B.newTabTap);
  const host = BLOCKER_APP_HOSTS.y;
  if (elapsed < B.yCloseTap) {
    const tabs: BlockerTab[] = [
      { content: "work", active: false },
      { content: "y", active: true, closeTouch: filmTap(elapsed, B.yCloseTap) },
    ];
    return { tabs, address: { kind: "host", host }, page, pageIn, loading: 0, newTabTouch };
  }
  if (elapsed < B.newTabTap) {
    return { tabs: [{ content: "work", active: true }], address: { kind: "placeholder" }, page, pageIn, loading: 0, newTabTouch };
  }
  const work: BlockerTab = { content: "work", active: false };
  if (elapsed < B.loadStart) {
    const text = blockerTyped(host, B.typeStart, B.typeEnd, elapsed);
    const caret = elapsed < B.typeStart || elapsed >= B.typeEnd ? blockerCaretOn(elapsed - B.newTabTap) : true;
    return { tabs: [work, { content: "blank", active: true }], address: { kind: "typed", text, caret }, page, pageIn, loading: 0, newTabTouch };
  }
  if (elapsed < B.blocked) {
    const loading = filmSpan(elapsed, B.loadStart, B.blocked);
    return { tabs: [work, { content: "y", active: true }], address: { kind: "host", host }, page, pageIn, loading, newTabTouch };
  }
  return { tabs: [work, { content: "blocked", active: true }], address: { kind: "extension" }, page, pageIn, loading: 0, newTabTouch };
}

/** The app open on the phone at this moment, or null on the home screen. */
export function limitsPhoneApp(elapsed: number): BlockerPhoneApp | null {
  const open = blockerAppOpen(elapsed, B.egogramTap, B.limitReached);
  if (open <= 0) return null;
  return { app: "egogram", open, scroll: easeInOutCubic(filmSpan(elapsed, PHONE_USE_START + 200, B.limitReached - 100)) };
}

/** The phone's limit notification at this moment, or null. */
export function limitsNotice(elapsed: number): BlockerNotice | null {
  const shown = blockerNoticeShown(elapsed, B.limitNotice);
  return shown > 0 ? { app: "egogram", reason: "limit", shown } : null;
}

/** The tap on Egogram's icon, from 0 to 1 as its touch shows. */
export function limitsEgogramTouch(elapsed: number): number {
  return filmTap(elapsed, B.egogramTap);
}
