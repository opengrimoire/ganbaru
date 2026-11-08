/**
 * Timing for the welcome scene on blocking during Focus: a Calendar event with Focus runs while a laptop's browser and a
 * phone are in use. Bluedit is open in the browser before the event; once the event's Focus starts, the extension swaps
 * the open tab for its block page, and Y's address typed into it is blocked as well. Egogram opened on the phone
 * returns Home with a notification, each of the three times it is tried. When the event ends, Y's page comes back and
 * Egogram opens as usual.
 */

import {
  BLOCKER_APP_HOSTS,
  BLOCKER_APP_OPEN_MS,
  blockerAddressClick,
  blockerAppOpen,
  blockerCaretOn,
  blockerNoticeShown,
  blockerPageIn,
  blockerPageRest,
  blockerPan,
  blockerSingleTab,
  blockerTyped,
  type BlockerApp,
  type BlockerBrowserFrame,
  type BlockerBrowserPage,
  type BlockerNotice,
  type BlockerPanMove,
  type BlockerPhoneApp,
} from "./welcome-blocker";
import type { WelcomeCursorScript } from "./welcome-cursor";
import {
  easeInOutCubic,
  filmDayRails,
  filmSpan,
  filmTap,
  monotoneCurve,
  type FilmKeyframe,
  type FilmRail,
  type FilmSpan,
} from "./welcome-film";

/** The Calendar event with Focus, in minutes of the day, and the part of the day the scene's strip shows. */
export const BLOCKING_EVENT = { start: 10 * 60, end: 12 * 60 + 30 } as const;
export const BLOCKING_DAY_VIEW = { start: 9 * 60 + 40, end: 12 * 60 + 50 } as const;

/** The event's Focus rail, planned by the Calendar's own rhythm for the Creative preset. */
export const BLOCKING_RAIL: FilmRail = filmDayRails([{ ...BLOCKING_EVENT, preset: "creative" }])[0];

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const BLOCKING_BEATS = {
  /** The event's Focus starts, and the open tab gives way to the block page. */
  focusStart: 2200,
  /** A tap on the address bar, Y's address is typed, and the page loads into its block page. */
  addressTap: 3000,
  yTypeStart: 3200,
  yTypeEnd: 3550,
  yLoadStart: 3700,
  yBlocked: 4000,
  /** A tap opens Egogram on the phone, Android returns Home, and the block notification arrives, then leaves. */
  egogramTap: 5500,
  egogramHome: 6200,
  egogramNotice: 6550,
  egogramNoticeOut: 7700,
  /** Egogram is opened again twice, and returns Home each time; the notification arrives again after the first. */
  retryTap: 8100,
  retryHome: 8650,
  retryNotice: 9000,
  retryAgainTap: 9600,
  retryAgainHome: 10150,
  retryNoticeOut: 11000,
  /** The event ends, so Y's page comes back, and its feed starts scrolling. */
  focusEnd: 11600,
  yScrollStart: 12100,
  /** Egogram opens as usual. */
  egogramAgainTap: 13600,
  end: 15600,
} as const;

const B = BLOCKING_BEATS;

/** The frame shown as a still with reduced motion: both devices blocked during the event. */
export const BLOCKING_STILL = B.egogramNotice + 600;

/** How long the event pulses as its Focus starts. */
const PULSE_MS = 520;

/** The current time against scene time, in minutes of the day: it reaches the event, slows through its first minutes, then hurries to its end. */
const CLOCK: readonly FilmKeyframe[] = [
  [0, 9 * 60 + 52],
  [B.focusStart, BLOCKING_EVENT.start],
  [B.retryAgainHome, BLOCKING_EVENT.start + 8],
  [B.focusEnd, BLOCKING_EVENT.end],
  [B.end, BLOCKING_EVENT.end + 8],
];

/** Where the camera points on a narrow canvas: the phone for Egogram, the browser for the event's end, then the phone again. */
const PAN_MOVES: readonly BlockerPanMove[] = [
  { start: 4700, to: 1 },
  { start: 10900, to: 0 },
  { start: 12700, to: 1 },
];

/** The current time, in minutes of the day. */
export function blockingClock(elapsed: number): number {
  return monotoneCurve(CLOCK, elapsed);
}

/** Whether the event's Focus is running. */
export function blockingFocusing(elapsed: number): boolean {
  return elapsed >= B.focusStart && elapsed < B.focusEnd;
}

/** The event's Focus time left, in seconds; the whole event before it starts, and 0 once it ends. */
export function blockingFocusRemaining(elapsed: number): number {
  const minutes = Math.min(BLOCKING_EVENT.end - BLOCKING_EVENT.start, Math.max(0, BLOCKING_EVENT.end - blockingClock(elapsed)));
  return elapsed >= B.focusEnd ? 0 : minutes * 60;
}

/**
 * The focus the rail has recorded, in minutes of the day: its focus spans up to the current time, as the Calendar
 * fills its rail while Focus runs, and all of them once the event ends.
 */
export function blockingRailRecorded(elapsed: number): FilmSpan[] {
  const now = Math.min(blockingClock(elapsed), BLOCKING_RAIL.end);
  return BLOCKING_RAIL.focus.filter((span) => span.start < now).map((span) => ({ start: span.start, end: Math.min(span.end, now) }));
}

/**
 * The Focus phase the current time is in, as the Focus ring drains it: the seconds left in that focus or break and its
 * whole length. Null outside the event's Focus.
 */
export function blockingPhase(elapsed: number): { remainingSeconds: number; totalSeconds: number } | null {
  if (!blockingFocusing(elapsed)) return null;
  const now = blockingClock(elapsed);
  const phase = [...BLOCKING_RAIL.focus, ...BLOCKING_RAIL.breaks].find((span) => now >= span.start && now < span.end);
  if (!phase) return null;
  return { remainingSeconds: (phase.end - now) * 60, totalSeconds: (phase.end - phase.start) * 60 };
}

/** How far the event has pulsed as its Focus starts and the open site is blocked, from 0 to 1; 0 when it is not pulsing. */
export function blockingPulse(elapsed: number): number {
  const progress = filmSpan(elapsed, B.focusStart, B.focusStart + PULSE_MS);
  return progress > 0 && progress < 1 ? progress : 0;
}

/** Where the camera points on a narrow canvas, from 0 (the browser) to 1 (the phone). */
export function blockingPan(elapsed: number): number {
  return blockerPan(elapsed, PAN_MOVES);
}

/** The extension's block page for `app` during the event, with the minutes of Focus left. */
function focusBlock(elapsed: number, app: BlockerApp): BlockerBrowserPage {
  return { kind: "block", app, reason: "focus", minutesLeft: Math.ceil(blockingFocusRemaining(elapsed) / 60) };
}

/**
 * What the browser's tab shows at this moment: Bluedit's feed scrolling before the event, its block page once Focus
 * starts, Y's block page once its address loads, and Y's feed scrolling after the event.
 */
export function blockingBrowserPage(elapsed: number): BlockerBrowserPage {
  if (elapsed < B.focusStart) return { kind: "feed", app: "bluedit", scroll: easeInOutCubic(filmSpan(elapsed, 200, B.focusStart - 150)) };
  if (elapsed >= B.focusEnd) return { kind: "feed", app: "y", scroll: 0.45 * easeInOutCubic(filmSpan(elapsed, B.yScrollStart, B.end - 300)) };
  return focusBlock(elapsed, elapsed >= B.yBlocked ? "y" : "bluedit");
}

/** The browser at this moment: one tab, with Y's address typed into it and loading while Bluedit's block page shows. */
export function blockingBrowser(elapsed: number): BlockerBrowserFrame {
  const page = blockingBrowserPage(elapsed);
  const frame = blockerSingleTab(page, blockerPageIn(elapsed, [B.focusStart, B.yBlocked, B.focusEnd]));
  if (elapsed < B.addressTap || elapsed >= B.yBlocked) return frame;
  const host = BLOCKER_APP_HOSTS.y;
  if (elapsed >= B.yLoadStart) return { ...frame, address: { kind: "host", host }, loading: filmSpan(elapsed, B.yLoadStart, B.yBlocked) };
  const typing = elapsed >= B.yTypeStart && elapsed < B.yTypeEnd;
  const text = blockerTyped(host, B.yTypeStart, B.yTypeEnd, elapsed);
  return { ...frame, address: { kind: "typed", text, caret: typing || blockerCaretOn(elapsed - B.addressTap) } };
}

/**
 * The browser's mouse pointer: it rests on Bluedit's feed as it scrolls, clicks the address bar once the block page
 * shows, and goes back to the page as Y's feed returns after the event.
 */
export const BLOCKING_CURSOR: WelcomeCursorScript = [
  blockerPageRest(0, [0.6, 0.55]),
  blockerAddressClick(B.addressTap, B.focusStart),
  blockerPageRest(B.yScrollStart, [0.55, 0.6]),
];

/** The app open on the phone at this moment, or null on the home screen. */
export function blockingPhoneApp(elapsed: number): BlockerPhoneApp | null {
  const blocked = Math.max(
    blockerAppOpen(elapsed, B.egogramTap, B.egogramHome),
    blockerAppOpen(elapsed, B.retryTap, B.retryHome),
    blockerAppOpen(elapsed, B.retryAgainTap, B.retryAgainHome),
  );
  if (blocked > 0) return { app: "egogram", open: blocked, scroll: 0 };
  const open = blockerAppOpen(elapsed, B.egogramAgainTap);
  if (open > 0) {
    const scroll = easeInOutCubic(filmSpan(elapsed, B.egogramAgainTap + BLOCKER_APP_OPEN_MS + 300, B.end - 200));
    return { app: "egogram", open, scroll };
  }
  return null;
}

/** The phone's block notification at this moment, or null. */
export function blockingNotice(elapsed: number): BlockerNotice | null {
  const shown = Math.max(blockerNoticeShown(elapsed, B.egogramNotice, B.egogramNoticeOut), blockerNoticeShown(elapsed, B.retryNotice, B.retryNoticeOut));
  return shown > 0 ? { app: "egogram", reason: "focus", shown } : null;
}

/** The taps on Egogram's icon, from 0 to 1 as each touch shows. */
export function blockingEgogramTouch(elapsed: number): number {
  return Math.max(filmTap(elapsed, B.egogramTap), filmTap(elapsed, B.retryTap), filmTap(elapsed, B.retryAgainTap), filmTap(elapsed, B.egogramAgainTap));
}
