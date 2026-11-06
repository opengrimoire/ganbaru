/**
 * Timing for the welcome scene about the app itself. The app's code writes itself out in the open, people's devices
 * gather around it, and each one receives its own copy, which then fills with that person's own things. Every moment
 * is derived from the milliseconds elapsed since the scene started, so a frame is a pure function of time and the
 * reduced-motion still is simply the last frame.
 */

import { easeInOutCubic, filmSpan } from "./welcome-film";

/** How many lines of code the public code shows. */
export const ABOUT_CODE_LINES = 6;

/** How many people's devices receive a copy. */
export const ABOUT_RECIPIENT_COUNT = 5;

/** How many of a person's own items fill a device once its copy arrives. */
export const ABOUT_SCREEN_ROWS = 3;

/** The order in which the devices, left to right, appear and receive their copies: from the middle outwards. */
export const ABOUT_COPY_ORDER: readonly number[] = [2, 1, 3, 0, 4];

/** Moments of the scene, in milliseconds from its start. */
export const ABOUT_BEATS = {
  /** The code appears, then writes itself out line by line. */
  cardIn: 200,
  codeStart: 600,
  /** People's devices gather around the code. */
  devicesIn: 1500,
  /** A copy of the code travels to each device. */
  copiesStart: 2300,
  end: 5900,
} as const;

const CARD_IN_MS = 500;
const CODE_STAGGER_MS = 110;
const CODE_LINE_MS = 260;
const DEVICE_STAGGER_MS = 220;
const DEVICE_IN_MS = 400;
const COPY_STAGGER_MS = 420;
const COPY_MS = 750;
const ROW_STAGGER_MS = 120;
const ROW_IN_MS = 260;

/** Where device `index` falls in the order of arrival. */
function rank(index: number): number {
  const position = ABOUT_COPY_ORDER.indexOf(index);
  return position < 0 ? index : position;
}

/** When the copy for device `index` leaves the code. */
function copyDeparture(index: number): number {
  return ABOUT_BEATS.copiesStart + rank(index) * COPY_STAGGER_MS;
}

/** How far the code has appeared, from 0 to 1. */
export function aboutCard(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, ABOUT_BEATS.cardIn, ABOUT_BEATS.cardIn + CARD_IN_MS));
}

/** How much of code line `line` is written, from 0 to 1. */
export function aboutCodeLine(elapsed: number, line: number): number {
  const start = ABOUT_BEATS.codeStart + line * CODE_STAGGER_MS;
  return easeInOutCubic(filmSpan(elapsed, start, start + CODE_LINE_MS));
}

/** How far device `index` has appeared, from 0 to 1. */
export function aboutRecipient(elapsed: number, index: number): number {
  const start = ABOUT_BEATS.devicesIn + rank(index) * DEVICE_STAGGER_MS;
  return easeInOutCubic(filmSpan(elapsed, start, start + DEVICE_IN_MS));
}

/** How far the copy for device `index` has travelled from the code, from 0 to 1 once it has arrived. */
export function aboutCopy(elapsed: number, index: number): number {
  const departure = copyDeparture(index);
  return easeInOutCubic(filmSpan(elapsed, departure, departure + COPY_MS));
}

/** How far item `row` has appeared on device `index`, from 0 to 1: the device fills only once its copy arrives. */
export function aboutScreenRow(elapsed: number, index: number, row: number): number {
  const start = copyDeparture(index) + COPY_MS + row * ROW_STAGGER_MS;
  return easeInOutCubic(filmSpan(elapsed, start, start + ROW_IN_MS));
}
