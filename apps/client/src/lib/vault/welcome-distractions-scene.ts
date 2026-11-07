/**
 * Timing for the distraction blocker's welcome scene: a laptop's browser and a phone share one Focus session. During
 * Focus, a site opened in the browser gives way to the extension's block page, and an app opened on the phone returns
 * Home with a notification. Once the day's Focus is done, a daily limit linked to a site and its app counts time from
 * both devices, and when its hour is used up the site and the app are blocked on both. Every moment is derived from the
 * milliseconds elapsed since the scene started, so a frame is a pure function of time and the reduced-motion still is
 * simply the last frame.
 */

import { easeInOutCubic, filmSpan, filmTap, monotoneCurve, type FilmKeyframe } from "./welcome-film";

/** The parody apps the scene draws, each with its website. */
export type DistractionsApp = "y" | "bluedit" | "egogram";

/** The site opened in the browser during Focus. */
export const DISTRACTIONS_FOCUS_HOST = "bluedit.com";
/** The site the daily limit links, with the phone app of the same name. */
export const DISTRACTIONS_LIMIT_HOST = "y.com";
/** The daily limit's budget, and how much of it each device uses, in minutes. */
export const DISTRACTIONS_LIMIT_MINUTES = 60;
export const DISTRACTIONS_BROWSER_MINUTES = 40;
export const DISTRACTIONS_PHONE_MINUTES = DISTRACTIONS_LIMIT_MINUTES - DISTRACTIONS_BROWSER_MINUTES;
/** The length of the Focus phase the scene opens in, in seconds. */
export const DISTRACTIONS_FOCUS_SECONDS = 25 * 60;

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const DISTRACTIONS_BEATS = {
  /** A tap opens a new tab, the blocked site's address is typed, and the block page replaces it once it loads. */
  newTabTap: 700,
  typeFocusStart: 950,
  typeFocusEnd: 1950,
  focusLoad: 2200,
  focusBlocked: 2650,
  /** A tap on Close tab returns to the work tab. */
  closeTap: 4600,
  /** A tap opens Egogram on the phone, Android returns Home, and the block notification arrives, then leaves. */
  egogramTap: 6200,
  egogramHome: 7250,
  egogramNotice: 7600,
  egogramNoticeOut: 9500,
  /** The day's Focus ends, and the daily limit takes the Focus timer's place. */
  focusDone: 10500,
  limitShown: 11500,
  /** The limited site is typed into the work tab's address and opens, as no Focus runs now. */
  addressTap: 12300,
  typeLimitStart: 12500,
  typeLimitEnd: 12950,
  limitLoad: 13150,
  limitPage: 13450,
  browserUseEnd: 15300,
  /** The limited app is opened on the phone and used until the budget runs out, which blocks it on both devices. */
  yTap: 16400,
  limitReached: 18300,
  limitNotice: 18650,
  end: 22200,
} as const;

const B = DISTRACTIONS_BEATS;

/** How long an app takes to open from its icon, and to shrink back into it. */
const APP_OPEN_MS = 320;
const APP_CLOSE_MS = 320;
/** How long a page takes to fade in once it replaces another. */
const PAGE_FADE_MS = 220;
/** How long a notification takes to slide in, and away. */
const NOTICE_IN_MS = 280;
const NOTICE_OUT_MS = 220;
/** How long the Focus timer takes to give way to the daily limit. */
const HEADER_SWAP_MS = 450;
/** How long the Focus timer pulses when it blocks something. */
const PULSE_MS = 520;
/** How long the camera takes to move between the devices on a narrow canvas. */
const PAN_MS = 650;
/** How long the text caret stays on, and off, as it blinks. */
const CARET_BLINK_MS = 530;

/**
 * The Focus phase's remaining time against scene time, in seconds. It runs at about real speed while the blocks are
 * shown, so the block page's minutes match the timer, then hurries to the end of the day's Focus.
 */
const FOCUS_CLOCK: readonly FilmKeyframe[] = [
  [0, 21 * 60 + 40],
  [B.egogramNoticeOut, 21 * 60 + 30],
  [B.focusDone, 0],
];

/**
 * Where the camera points on a narrow canvas, from 0 (the browser) to 1 (the phone): each move starts once the last has
 * ended, toward the device about to act.
 */
const PAN_MOVES: readonly { start: number; to: number }[] = [
  { start: 4900, to: 1 },
  { start: 11500, to: 0 },
  { start: 15500, to: 1 },
  { start: 19700, to: 0 },
];

/** Where the camera points on a narrow canvas, from 0 (the browser) to 1 (the phone). */
export function distractionsPan(elapsed: number): number {
  let value = 0;
  for (const move of PAN_MOVES) {
    value += (move.to - value) * easeInOutCubic(filmSpan(elapsed, move.start, move.start + PAN_MS));
  }
  return value;
}

/** The Focus phase's remaining time, in seconds. */
export function distractionsFocusRemaining(elapsed: number): number {
  return Math.max(0, monotoneCurve(FOCUS_CLOCK, elapsed));
}

/** Whether the day's Focus is still running. */
export function distractionsFocusing(elapsed: number): boolean {
  return elapsed < B.focusDone;
}

/** The minutes the block page says are left in Focus, rounded up as the extension rounds them. */
export const DISTRACTIONS_BLOCK_MINUTES_LEFT = Math.ceil(distractionsFocusRemaining(B.focusBlocked) / 60);

/** How far the Focus timer's pulse has run as it blocks something, from 0 to 1; 0 when it is not pulsing. */
export function distractionsPulse(elapsed: number): number {
  for (const beat of [B.focusBlocked, B.egogramHome]) {
    const progress = filmSpan(elapsed, beat, beat + PULSE_MS);
    if (progress > 0 && progress < 1) return progress;
  }
  return 0;
}

/** How far the Focus timer has given way to the daily limit, from 0 to 1. */
export function distractionsLimitShown(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, B.limitShown - HEADER_SWAP_MS, B.limitShown));
}

/** What the browser's active tab shows. */
export type DistractionsBrowserPage = "work" | "blank" | "focusBlock" | "feed" | "limitBlock";

/** What the browser's active tab shows at this moment. */
export function distractionsBrowserPage(elapsed: number): DistractionsBrowserPage {
  if (elapsed < B.newTabTap) return "work";
  if (elapsed < B.focusBlocked) return "blank";
  if (elapsed < B.closeTap) return "focusBlock";
  if (elapsed < B.limitPage) return "work";
  if (elapsed < B.limitReached) return "feed";
  return "limitBlock";
}

/** How far the active tab's page has faded in since it last changed, from 0 to 1. */
export function distractionsPageIn(elapsed: number): number {
  const changes = [B.newTabTap, B.focusBlocked, B.closeTap, B.limitPage, B.limitReached].filter((beat) => beat <= elapsed);
  const last = changes.at(-1);
  return last === undefined ? 1 : easeInOutCubic(filmSpan(elapsed, last, last + PAGE_FADE_MS));
}

/** Whether the new tab for the blocked site is open, and active. */
export function distractionsNewTabOpen(elapsed: number): boolean {
  return elapsed >= B.newTabTap && elapsed < B.closeTap;
}

/** What the active tab's address bar shows. */
export type DistractionsAddress =
  | { kind: "placeholder" }
  | { kind: "typed"; text: string; editing: boolean }
  | { kind: "extension" };

/** The prefix of `text` typed by `elapsed`, one character at a time from `start` to `end`. */
function typed(text: string, start: number, end: number, elapsed: number): string {
  return text.slice(0, Math.floor(text.length * filmSpan(elapsed, start, end)));
}

/** What the active tab's address bar shows at this moment. */
export function distractionsAddress(elapsed: number): DistractionsAddress {
  if (distractionsNewTabOpen(elapsed)) {
    if (elapsed >= B.focusBlocked) return { kind: "extension" };
    return { kind: "typed", text: typed(DISTRACTIONS_FOCUS_HOST, B.typeFocusStart, B.typeFocusEnd, elapsed), editing: elapsed < B.focusLoad };
  }
  if (elapsed < B.addressTap) return { kind: "placeholder" };
  if (elapsed >= B.limitReached) return { kind: "extension" };
  return { kind: "typed", text: typed(DISTRACTIONS_LIMIT_HOST, B.typeLimitStart, B.typeLimitEnd, elapsed), editing: elapsed < B.limitLoad };
}

/** Whether the text caret is drawn at this moment, as it blinks. */
export function distractionsCaretOn(elapsed: number): boolean {
  return Math.floor(elapsed / CARET_BLINK_MS) % 2 === 0;
}

/** How far the active tab's page has loaded, from 0 to 1, or null while nothing loads. */
export function distractionsLoading(elapsed: number): number | null {
  for (const [start, end] of [[B.focusLoad, B.focusBlocked], [B.limitLoad, B.limitPage]] as const) {
    if (elapsed >= start && elapsed < end) return easeInOutCubic(filmSpan(elapsed, start, end));
  }
  return null;
}

/** The taps the scene draws, each from 0 to 1 as its touch shows. */
export function distractionsTouches(elapsed: number): {
  newTab: number;
  close: number;
  address: number;
  egogram: number;
  y: number;
} {
  return {
    newTab: filmTap(elapsed, B.newTabTap),
    close: filmTap(elapsed, B.closeTap),
    address: filmTap(elapsed, B.addressTap),
    egogram: filmTap(elapsed, B.egogramTap),
    y: filmTap(elapsed, B.yTap),
  };
}

/** The app open on the phone and how far it has opened from its icon, from 0 to 1, or null on the home screen. */
export function distractionsPhoneApp(elapsed: number): { app: DistractionsApp; open: number } | null {
  const runs = [
    { app: "egogram", tap: B.egogramTap, home: B.egogramHome },
    { app: "y", tap: B.yTap, home: B.limitReached },
  ] as const;
  for (const run of runs) {
    if (elapsed < run.tap || elapsed >= run.home + APP_CLOSE_MS) continue;
    const opened = easeInOutCubic(filmSpan(elapsed, run.tap, run.tap + APP_OPEN_MS));
    const closed = easeInOutCubic(filmSpan(elapsed, run.home, run.home + APP_CLOSE_MS));
    return { app: run.app, open: opened * (1 - closed) };
  }
  return null;
}

/** The block notification on the phone, why it was posted, and how far it has slid in, from 0 to 1, or null. */
export function distractionsNotification(
  elapsed: number,
): { app: DistractionsApp; reason: "focus" | "limit"; shown: number } | null {
  if (elapsed >= B.egogramNotice && elapsed < B.egogramNoticeOut + NOTICE_OUT_MS) {
    const shown = easeInOutCubic(filmSpan(elapsed, B.egogramNotice, B.egogramNotice + NOTICE_IN_MS))
      * (1 - easeInOutCubic(filmSpan(elapsed, B.egogramNoticeOut, B.egogramNoticeOut + NOTICE_OUT_MS)));
    return { app: "egogram", reason: "focus", shown };
  }
  if (elapsed >= B.limitNotice) {
    return { app: "y", reason: "limit", shown: easeInOutCubic(filmSpan(elapsed, B.limitNotice, B.limitNotice + NOTICE_IN_MS)) };
  }
  return null;
}

/** How far a feed has scrolled, from 0 to 1: the browser's while the limited site is open, and the phone's in its app. */
export function distractionsFeedScroll(elapsed: number, device: "browser" | "phone"): number {
  return device === "browser"
    ? easeInOutCubic(filmSpan(elapsed, B.limitPage + 300, B.browserUseEnd))
    : easeInOutCubic(filmSpan(elapsed, B.yTap + APP_OPEN_MS + 200, B.limitReached - 100));
}

/** The daily limit's minutes used so far on each device; together they reach the budget exactly as the limit is reached. */
export function distractionsUsage(elapsed: number): { browser: number; phone: number } {
  return {
    browser: DISTRACTIONS_BROWSER_MINUTES * filmSpan(elapsed, B.limitPage, B.browserUseEnd),
    phone: DISTRACTIONS_PHONE_MINUTES * filmSpan(elapsed, B.yTap + APP_OPEN_MS, B.limitReached),
  };
}
