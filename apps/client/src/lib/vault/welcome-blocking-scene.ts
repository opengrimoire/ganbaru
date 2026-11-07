/**
 * Timing for the welcome scene on blocking during Focus: a Calendar event with Focus runs while a laptop's browser and a
 * phone are in use. Bluedit is open in the browser before the event; once the event's Focus starts, the extension swaps
 * the open tab for its block page, and Egogram opened on the phone returns Home with a notification. When the event
 * ends, the tab comes back and Egogram opens as usual.
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
import { easeInOutCubic, filmSpan, filmTap, monotoneCurve, type FilmKeyframe } from "./welcome-film";

/** The Calendar event with Focus, in minutes of the day, and the part of the day the scene's strip shows. */
export const BLOCKING_EVENT = { start: 10 * 60, end: 11 * 60 } as const;
export const BLOCKING_DAY_VIEW = { start: 9 * 60 + 40, end: 11 * 60 + 20 } as const;

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const BLOCKING_BEATS = {
  /** The event's Focus starts, and the open tab gives way to the block page. */
  focusStart: 2200,
  /** A tap opens Egogram on the phone, Android returns Home, and the block notification arrives, then leaves. */
  egogramTap: 5200,
  egogramHome: 5900,
  egogramNotice: 6250,
  egogramNoticeOut: 8000,
  /** The event ends, so the tab comes back. */
  focusEnd: 9300,
  /** Egogram opens as usual. */
  egogramAgainTap: 11300,
  end: 13300,
} as const;

const B = BLOCKING_BEATS;

/** The frame shown as a still with reduced motion: both devices blocked during the event. */
export const BLOCKING_STILL = B.egogramNotice + 600;

/** How long the event's Focus timer pulses when it blocks something. */
const PULSE_MS = 520;

/** The current time against scene time, in minutes of the day: it reaches the event, slows through its first minutes, then hurries to its end. */
const CLOCK: readonly FilmKeyframe[] = [
  [0, 9 * 60 + 52],
  [B.focusStart, BLOCKING_EVENT.start],
  [B.egogramNoticeOut, BLOCKING_EVENT.start + 6],
  [B.focusEnd, BLOCKING_EVENT.end],
  [B.end, BLOCKING_EVENT.end + 8],
];

/** Where the camera points on a narrow canvas: the phone for Egogram, the browser for the event's end, then the phone again. */
const PAN_MOVES: readonly BlockerPanMove[] = [
  { start: 4300, to: 1 },
  { start: 8400, to: 0 },
  { start: 10400, to: 1 },
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

/** How far the event's Focus timer has pulsed as it blocks something, from 0 to 1; 0 when it is not pulsing. */
export function blockingPulse(elapsed: number): number {
  for (const beat of [B.focusStart, B.egogramHome]) {
    const progress = filmSpan(elapsed, beat, beat + PULSE_MS);
    if (progress > 0 && progress < 1) return progress;
  }
  return 0;
}

/** Where the camera points on a narrow canvas, from 0 (the browser) to 1 (the phone). */
export function blockingPan(elapsed: number): number {
  return blockerPan(elapsed, PAN_MOVES);
}

/** How far the Bluedit feed has scrolled: a little before the event, and on after it. */
function feedScroll(elapsed: number): number {
  return 0.35 * easeInOutCubic(filmSpan(elapsed, 300, B.focusStart - 200))
    + 0.45 * easeInOutCubic(filmSpan(elapsed, B.focusEnd + 500, B.end - 300));
}

/** What the browser's tab shows at this moment. */
export function blockingBrowserPage(elapsed: number): BlockerBrowserPage {
  if (!blockingFocusing(elapsed)) return { kind: "feed", app: "bluedit", scroll: feedScroll(elapsed) };
  return { kind: "block", app: "bluedit", reason: "focus", minutesLeft: Math.ceil(blockingFocusRemaining(elapsed) / 60) };
}

/** How far the browser's page has faded in since it last changed, from 0 to 1. */
export function blockingPageIn(elapsed: number): number {
  return blockerPageIn(elapsed, [B.focusStart, B.focusEnd]);
}

/** The app open on the phone at this moment, or null on the home screen. */
export function blockingPhoneApp(elapsed: number): BlockerPhoneApp | null {
  const blocked = blockerAppOpen(elapsed, B.egogramTap, B.egogramHome);
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
  const shown = blockerNoticeShown(elapsed, B.egogramNotice, B.egogramNoticeOut);
  return shown > 0 ? { app: "egogram", reason: "focus", shown } : null;
}

/** The taps on Egogram's icon, from 0 to 1 as each touch shows. */
export function blockingEgogramTouch(elapsed: number): number {
  return Math.max(filmTap(elapsed, B.egogramTap), filmTap(elapsed, B.egogramAgainTap));
}
