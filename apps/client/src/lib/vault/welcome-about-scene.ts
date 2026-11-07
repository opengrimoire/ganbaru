/**
 * Timing for the welcome scene about the app itself. A receipt printer rises into view and prints a receipt line by
 * line, every feature priced free and the total free as well, and the license then stamps the receipt as the reason it
 * stays that way. Every moment is derived from the milliseconds elapsed since the scene started, so a frame is a pure
 * function of time and the reduced-motion still is simply the last frame.
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
const PRINT_START = 700;
/** How long after the last segment the stamp starts to come down, how long it takes, and how long the receipt shakes. */
const STAMP_DELAY_MS = 350;
const STAMP_MS = 240;
const IMPACT_MS = 220;
const END_REST_MS = 600;

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
  printStart: PRINT_START,
  printEnd: PRINT_END,
  stampStart: STAMP_START,
  stampLanded: STAMP_LANDED,
  end: STAMP_LANDED + IMPACT_MS + END_REST_MS,
} as const;

/** How far the printer has risen into view, from 0 to 1. */
export function aboutPrinter(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, PRINTER_IN, PRINTER_IN + PRINTER_IN_MS));
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
