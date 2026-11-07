/**
 * Timing for the welcome scene about the app itself. One person's three devices appear holding the same things, link
 * directly to each other, and then each in turn adds something that travels straight to the other two, so the data
 * lives on the devices and moves only between them. Every moment is derived from the milliseconds elapsed since the
 * scene started, so a frame is a pure function of time and the reduced-motion still is simply the last frame.
 */

import { easeInOutCubic, filmSpan } from "./welcome-film";

/** How many devices the person has. */
export const ABOUT_DEVICE_COUNT = 3;

/** How many things every device already holds when it appears. */
export const ABOUT_INITIAL_ROWS = 2;

/** The direct links between the devices, one for every pair, so no device stands between two others. */
export const ABOUT_LINKS: readonly (readonly [number, number])[] = [
  [0, 1],
  [0, 2],
  [1, 2],
];

/** The device that adds each new thing, in order: every device writes, and each sends to the other two. */
export const ABOUT_EDIT_ORIGINS: readonly number[] = [2, 0, 1];

/** How many things every device holds once the scene ends. */
export const ABOUT_ROW_COUNT = ABOUT_INITIAL_ROWS + ABOUT_EDIT_ORIGINS.length;

/** Moments of the scene, in milliseconds from its start. */
export const ABOUT_BEATS = {
  /** The devices appear, one after another. */
  devicesIn: 200,
  /** The links between them draw themselves. */
  linksStart: 1100,
  /** The first new thing is added. */
  editsStart: 2100,
  end: 6600,
} as const;

const DEVICE_STAGGER_MS = 180;
const DEVICE_IN_MS = 450;
const LINK_STAGGER_MS = 150;
const LINK_MS = 450;
const EDIT_STAGGER_MS = 1350;
const ROW_IN_MS = 300;
const TRAVEL_MS = 600;
const GLOW_MS = 600;

/** When edit `edit` starts on its origin. */
function editStart(edit: number): number {
  return ABOUT_BEATS.editsStart + edit * EDIT_STAGGER_MS;
}

/** When edit `edit` leaves its origin for the other devices. */
function departure(edit: number): number {
  return editStart(edit) + ROW_IN_MS;
}

/** When row `row` starts to appear on device `device`. */
function rowStart(device: number, row: number): number {
  if (row < ABOUT_INITIAL_ROWS) return ABOUT_BEATS.devicesIn + device * DEVICE_STAGGER_MS;
  const edit = row - ABOUT_INITIAL_ROWS;
  return ABOUT_EDIT_ORIGINS[edit] === device ? editStart(edit) : departure(edit) + TRAVEL_MS;
}

/** How far device `device` has appeared, from 0 to 1. */
export function aboutDevice(elapsed: number, device: number): number {
  const start = ABOUT_BEATS.devicesIn + device * DEVICE_STAGGER_MS;
  return easeInOutCubic(filmSpan(elapsed, start, start + DEVICE_IN_MS));
}

/** How much of link `link` is drawn, from 0 to 1. */
export function aboutLink(elapsed: number, link: number): number {
  const start = ABOUT_BEATS.linksStart + link * LINK_STAGGER_MS;
  return easeInOutCubic(filmSpan(elapsed, start, start + LINK_MS));
}

/** How far the thing added by edit `edit` has travelled from its origin to the other devices, from 0 to 1. */
export function aboutTravel(elapsed: number, edit: number): number {
  return easeInOutCubic(filmSpan(elapsed, departure(edit), departure(edit) + TRAVEL_MS));
}

/** How far row `row` has appeared on device `device`, from 0 to 1. */
export function aboutRow(elapsed: number, device: number, row: number): number {
  if (row < ABOUT_INITIAL_ROWS) return aboutDevice(elapsed, device);
  const start = rowStart(device, row);
  return easeInOutCubic(filmSpan(elapsed, start, start + ROW_IN_MS));
}

/** How strongly a new row marks itself as new on device `device`, from 0 to 1: it lights up as it lands, then fades. */
export function aboutRowGlow(elapsed: number, device: number, row: number): number {
  if (row < ABOUT_INITIAL_ROWS) return 0;
  const start = rowStart(device, row);
  if (elapsed < start) return 0;
  const rise = filmSpan(elapsed, start, start + ROW_IN_MS);
  const fade = filmSpan(elapsed, start + ROW_IN_MS, start + ROW_IN_MS + GLOW_MS);
  return rise * (1 - easeInOutCubic(fade));
}
