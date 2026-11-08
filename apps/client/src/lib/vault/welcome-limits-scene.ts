/**
 * Timing for the welcome scene on daily limits: one social media limit groups the Y and Bluedit websites and Egogram's
 * phone app, and counts time from both devices. Y's feed scrolls in a laptop's browser beside the inbox until Y's tab
 * is closed; then Egogram opened on a phone uses the rest of the day's budget, returns Home, and posts the limit
 * notification. Each time Egogram is opened again it returns Home at once, and back on the laptop, every site of the
 * limit typed into the browser opens the extension's block page instead.
 */

import {
  BLOCKER_APP_HOSTS,
  BLOCKER_APP_OPEN_MS,
  BLOCKER_CURSOR,
  BLOCKER_MAIL,
  blockerAddressClick,
  blockerAppOpen,
  blockerCaretOn,
  blockerCursorClose,
  blockerNoticeShown,
  blockerPageIn,
  blockerPageRest,
  blockerPan,
  blockerTyped,
  type BlockerApp,
  type BlockerBrowserFrame,
  type BlockerBrowserPage,
  type BlockerNotice,
  type BlockerPanMove,
  type BlockerPhoneApp,
  type BlockerTab,
  type BlockerTabContent,
} from "./welcome-blocker";
import { cursorClick, WELCOME_CURSOR_TRAVEL_MS, type WelcomeCursorScript } from "./welcome-cursor";
import { easeInOutCubic, filmSpan, filmTap } from "./welcome-film";

/** The daily limit's budget, and how much of it each device uses, in minutes. */
export const LIMITS_BUDGET_MINUTES = 60;
export const LIMITS_BROWSER_MINUTES = 40;
export const LIMITS_PHONE_MINUTES = LIMITS_BUDGET_MINUTES - LIMITS_BROWSER_MINUTES;

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const LIMITS_BEATS = {
  /** Y's feed scrolls in the browser, using part of the budget. */
  browserUseStart: 400,
  browserUseEnd: 3400,
  /** A tap closes Y's tab, so the inbox shows. */
  yCloseTap: 3900,
  /** A tap opens Egogram on the phone, which uses the rest of the budget. */
  egogramTap: 5200,
  /** The budget runs out: Egogram returns Home and its notification arrives, then retracts. */
  limitReached: 8000,
  limitNotice: 8350,
  limitNoticeOut: 9600,
  /** Egogram is opened again twice, and returns Home each time; the notification arrives again after the first. */
  retryTap: 10000,
  retryHome: 10550,
  retryNotice: 10900,
  retryAgainTap: 11500,
  retryAgainHome: 12050,
  /** The camera turns back to the browser on a narrow canvas. */
  browserReturn: 12500,
  /** A tap opens a new tab in the browser, Y's address is typed, and the page loads into the block page. */
  newTabTap: 13300,
  yTypeStart: 13550,
  yTypeEnd: 14000,
  yLoadStart: 14150,
  yBlocked: 14450,
  /** A tap on the address bar, Bluedit's address is typed, and the page loads into its block page. */
  addressTap: 15400,
  blueditTypeStart: 15600,
  blueditTypeEnd: 16250,
  blueditLoadStart: 16400,
  blueditBlocked: 16700,
  end: 18600,
} as const;

const B = LIMITS_BEATS;

/** Where the camera points on a narrow canvas: the phone for Egogram, then the browser for the sites. */
const PAN_MOVES: readonly BlockerPanMove[] = [
  { start: 4400, to: 1 },
  { start: B.browserReturn, to: 0 },
];

/** When the phone's use of the budget starts, once the app has opened. */
const PHONE_USE_START = B.egogramTap + BLOCKER_APP_OPEN_MS;

/** A site of the limit typed into the browser after it ran out: the tap that starts it, its typing, and its loading. */
interface SiteAttempt {
  app: BlockerApp;
  tap: number;
  typeStart: number;
  typeEnd: number;
  loadStart: number;
  blocked: number;
}

const SITE_ATTEMPTS: readonly SiteAttempt[] = [
  { app: "y", tap: B.newTabTap, typeStart: B.yTypeStart, typeEnd: B.yTypeEnd, loadStart: B.yLoadStart, blocked: B.yBlocked },
  {
    app: "bluedit",
    tap: B.addressTap,
    typeStart: B.blueditTypeStart,
    typeEnd: B.blueditTypeEnd,
    loadStart: B.blueditLoadStart,
    blocked: B.blueditBlocked,
  },
];

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

/** The new tab's page and tab before `attempt` commits: empty for the first site, the last site's block page after. */
function beforeAttempt(index: number): { page: BlockerBrowserPage; tab: BlockerTabContent } {
  const previous = SITE_ATTEMPTS[index - 1];
  return previous ? { page: { kind: "block", app: previous.app, reason: "limit" }, tab: "blocked" } : { page: { kind: "blank" }, tab: "blank" };
}

/** The browser while a site of the limit is typed into it, loads, and is blocked. */
function attemptFrame(elapsed: number, index: number, attempt: SiteAttempt): Omit<BlockerBrowserFrame, "pageIn"> {
  const mail: BlockerTab = { content: "mail", active: false };
  const host = BLOCKER_APP_HOSTS[attempt.app];
  if (elapsed >= attempt.blocked) {
    return { tabs: [mail, { content: "blocked", active: true }], address: { kind: "extension" }, page: { kind: "block", app: attempt.app, reason: "limit" }, loading: 0 };
  }
  const before = beforeAttempt(index);
  const tabs: BlockerTab[] = [mail, { content: before.tab, active: true }];
  if (elapsed >= attempt.loadStart) {
    return { tabs, address: { kind: "host", host }, page: before.page, loading: filmSpan(elapsed, attempt.loadStart, attempt.blocked) };
  }
  const text = blockerTyped(host, attempt.typeStart, attempt.typeEnd, elapsed);
  const typing = elapsed >= attempt.typeStart && elapsed < attempt.typeEnd;
  return { tabs, address: { kind: "typed", text, caret: typing || blockerCaretOn(elapsed - attempt.tap) }, page: before.page, loading: 0 };
}

/**
 * The browser at this moment: the inbox beside Y's tab until it is closed, then a new tab that each site of the limit
 * is typed into, one after another.
 */
export function limitsBrowser(elapsed: number): BlockerBrowserFrame {
  const pageIn = blockerPageIn(elapsed, [B.yCloseTap, B.newTabTap, ...SITE_ATTEMPTS.map((attempt) => attempt.blocked)]);
  if (elapsed < B.yCloseTap) {
    const scroll = easeInOutCubic(filmSpan(elapsed, B.browserUseStart, B.browserUseEnd));
    const tabs: BlockerTab[] = [
      { content: "mail", active: false },
      { content: "y", active: true },
    ];
    return { tabs, address: { kind: "host", host: BLOCKER_APP_HOSTS.y }, page: { kind: "feed", app: "y", scroll }, loading: 0, pageIn };
  }
  const index = SITE_ATTEMPTS.findLastIndex((attempt) => elapsed >= attempt.tap);
  const attempt = SITE_ATTEMPTS[index];
  if (!attempt) {
    return { tabs: [{ content: "mail", active: true }], address: { kind: "host", host: BLOCKER_MAIL.host }, page: { kind: "mail" }, loading: 0, pageIn };
  }
  return { ...attemptFrame(elapsed, index, attempt), pageIn };
}

/**
 * The browser's mouse pointer: it rests on Y's feed as it scrolls, closes Y's tab once the scrolling stops, and rests
 * on the inbox while the phone is in use. Back on the laptop, it opens a new tab, moves off the tab strip as the tabs
 * shift, and clicks the address bar for Bluedit's address.
 */
export const LIMITS_CURSOR: WelcomeCursorScript = [
  blockerPageRest(0, [0.6, 0.55], [{ start: B.browserUseStart, end: B.browserUseEnd }]),
  cursorClick(blockerCursorClose("y"), B.yCloseTap, { after: B.browserUseEnd }),
  blockerPageRest(B.yCloseTap + WELCOME_CURSOR_TRAVEL_MS, [0.55, 0.45]),
  cursorClick(BLOCKER_CURSOR.newTab, B.newTabTap, { after: B.browserReturn }),
  blockerPageRest(B.newTabTap + WELCOME_CURSOR_TRAVEL_MS, [0.5, 0.35]),
  blockerAddressClick(B.addressTap, B.yBlocked),
];

/** The app open on the phone at this moment, or null on the home screen. */
export function limitsPhoneApp(elapsed: number): BlockerPhoneApp | null {
  const used = blockerAppOpen(elapsed, B.egogramTap, B.limitReached);
  if (used > 0) {
    return { app: "egogram", open: used, scroll: easeInOutCubic(filmSpan(elapsed, PHONE_USE_START + 200, B.limitReached - 100)) };
  }
  const retried = Math.max(blockerAppOpen(elapsed, B.retryTap, B.retryHome), blockerAppOpen(elapsed, B.retryAgainTap, B.retryAgainHome));
  return retried > 0 ? { app: "egogram", open: retried, scroll: 0 } : null;
}

/** The phone's limit notification at this moment, or null. */
export function limitsNotice(elapsed: number): BlockerNotice | null {
  const shown = Math.max(blockerNoticeShown(elapsed, B.limitNotice, B.limitNoticeOut), blockerNoticeShown(elapsed, B.retryNotice));
  return shown > 0 ? { app: "egogram", reason: "limit", shown } : null;
}

/** The taps on Egogram's icon, from 0 to 1 as each touch shows. */
export function limitsEgogramTouch(elapsed: number): number {
  return Math.max(filmTap(elapsed, B.egogramTap), filmTap(elapsed, B.retryTap), filmTap(elapsed, B.retryAgainTap));
}
