/**
 * Timing for the welcome scene about the app itself. A payment terminal rises into view, its screen lights up with a
 * total that is free and confirms there is nothing to pay, and it then prints a receipt line by line, every feature
 * priced free and the total free as well. The license then stamps the receipt as the reason it stays that way. Every
 * moment is derived from the milliseconds elapsed since the scene started, so a frame is a pure function of time and the
 * reduced-motion still is simply the last frame.
 */

import { easeInOutCubic, filmSpan } from "./welcome-film";

/** How many features the receipt lists. */
export const ABOUT_RECEIPT_ITEMS = 8;

/**
 * One stretch of paper the printer feeds: the blank lead, the header, a rule, a feature, the total, the barcode, or the
 * blank tail.
 */
export type AboutSegment = "lead" | "header" | "rule" | "item" | "total" | "barcode" | "tail";

/** The receipt from top to bottom, in the order the printer feeds it. */
export const ABOUT_SEGMENTS: readonly AboutSegment[] = [
  "lead",
  "header",
  "rule",
  ...Array.from({ length: ABOUT_RECEIPT_ITEMS }, (): AboutSegment => "item"),
  "rule",
  "total",
  "barcode",
  "tail",
];

/** How long the printer takes to feed each kind of segment. */
const FEED_MS: Readonly<Record<AboutSegment, number>> = {
  lead: 220,
  header: 340,
  rule: 130,
  item: 200,
  total: 300,
  barcode: 340,
  tail: 220,
};
/** The pause after each segment, so the paper feeds in steps as a printer's does. */
const FEED_GAP_MS = 30;

const PRINTER_IN = 150;
const PRINTER_IN_MS = 400;
/** The screen lights up with the total once the terminal is in place. */
const SCREEN_ON = PRINTER_IN + PRINTER_IN_MS;
const SCREEN_ON_MS = 260;
/** How long the total stays on screen before it gives way to the confirmation. */
const TOTAL_HOLD_MS = 520;
const TOTAL_OUT_MS = 200;
/** The confirmation: a ring that draws around a check, which draws in once the ring is mostly closed. */
const RING_DELAY_MS = 90;
const RING_MS = 320;
const CHECK_DELAY_MS = 240;
const CHECK_MS = 220;
/** How long the confirmation rests on screen before the receipt starts to print. */
const CONFIRM_HOLD_MS = 260;
/** How long after the last segment the stamp starts to come down, how long it takes, and how long the receipt shakes. */
const STAMP_DELAY_MS = 350;
const STAMP_MS = 240;
const IMPACT_MS = 220;
const END_REST_MS = 600;

const TOTAL_OUT = SCREEN_ON + SCREEN_ON_MS + TOTAL_HOLD_MS;
const RING_START = TOTAL_OUT + RING_DELAY_MS;
const CHECK_START = RING_START + CHECK_DELAY_MS;
const CONFIRMED = CHECK_START + CHECK_MS;
const PRINT_START = CONFIRMED + CONFIRM_HOLD_MS;

/** When each segment starts to feed: right after the one before it and its pause. */
function segmentStarts(): number[] {
  const starts: number[] = [];
  let next = PRINT_START;
  for (const segment of ABOUT_SEGMENTS) {
    starts.push(next);
    next += FEED_MS[segment] + FEED_GAP_MS;
  }
  return starts;
}

const SEGMENT_STARTS = segmentStarts();
const PRINT_END = SEGMENT_STARTS[SEGMENT_STARTS.length - 1] + FEED_MS[ABOUT_SEGMENTS[ABOUT_SEGMENTS.length - 1]];
const STAMP_START = PRINT_END + STAMP_DELAY_MS;
const STAMP_LANDED = STAMP_START + STAMP_MS;

/** Moments of the scene, in milliseconds from its start. */
export const ABOUT_BEATS = {
  screenOn: SCREEN_ON,
  confirmed: CONFIRMED,
  printStart: PRINT_START,
  printEnd: PRINT_END,
  stampStart: STAMP_START,
  stampLanded: STAMP_LANDED,
  end: STAMP_LANDED + IMPACT_MS + END_REST_MS,
} as const;

/** How far the terminal has risen into view, from 0 to 1. */
export function aboutPrinter(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, PRINTER_IN, PRINTER_IN + PRINTER_IN_MS));
}

/** How brightly the terminal's screen is lit, from 0 to 1. */
export function aboutScreen(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, SCREEN_ON, SCREEN_ON + SCREEN_ON_MS));
}

/** How visible the total is on the screen, from 0 to 1: it appears as the screen lights and leaves for the confirmation. */
export function aboutTotal(elapsed: number): number {
  return aboutScreen(elapsed) * (1 - easeInOutCubic(filmSpan(elapsed, TOTAL_OUT, TOTAL_OUT + TOTAL_OUT_MS)));
}

/** How much of the confirmation's ring has been drawn, from 0 to 1. */
export function aboutConfirmRing(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, RING_START, RING_START + RING_MS));
}

/** How much of the confirmation's check has been drawn, from 0 to 1. */
export function aboutConfirmCheck(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, CHECK_START, CONFIRMED));
}

/** How much of segment `segment` the printer has fed out, from 0 to 1. */
export function aboutFeed(elapsed: number, segment: number): number {
  const start = SEGMENT_STARTS[segment];
  return easeInOutCubic(filmSpan(elapsed, start, start + FEED_MS[ABOUT_SEGMENTS[segment]]));
}

/** Whether the printer is printing at this moment. */
export function aboutPrinting(elapsed: number): boolean {
  return elapsed >= PRINT_START && elapsed < PRINT_END;
}

/** How far the stamp has come down onto the receipt, from 0 to 1, speeding up as a stamp does. */
export function aboutStamp(elapsed: number): number {
  return filmSpan(elapsed, STAMP_START, STAMP_LANDED) ** 2;
}

/** How hard the receipt shakes from the stamp's impact, from 1 as it lands down to 0. */
export function aboutImpact(elapsed: number): number {
  if (elapsed < STAMP_LANDED) return 0;
  return 1 - easeInOutCubic(filmSpan(elapsed, STAMP_LANDED, STAMP_LANDED + IMPACT_MS));
}
