/**
 * Timing for the welcome scene about the app itself: the app's public source repository opens in a browser, its files
 * come into view, and a mouse click stars it. Every moment is derived from the milliseconds elapsed since the scene
 * started, so a frame is a pure function of time and the reduced-motion still is simply the last frame.
 */

import { cursorClick, cursorRest, type WelcomeCursorScript } from "./welcome-cursor";
import { easeInOutCubic, FILM_TAP_MS, filmSpan } from "./welcome-film";

/** The app's public source repository. */
export const ABOUT_REPOSITORY = { host: "github.com", owner: "opengrimoire", name: "ganbaru" } as const;

/** The repository's web address. */
export const ABOUT_REPOSITORY_URL = `https://${ABOUT_REPOSITORY.host}/${ABOUT_REPOSITORY.owner}/${ABOUT_REPOSITORY.name}`;

/** One entry of the repository's top-level file list. */
export interface AboutFile {
  name: string;
  folder: boolean;
}

/** The top-level entries the repository page lists, folders first as the repository host sorts them. */
export const ABOUT_FILES: readonly AboutFile[] = [
  { name: ".github", folder: true },
  { name: "apps", folder: true },
  { name: "crates", folder: true },
  { name: "docs", folder: true },
  { name: "extensions", folder: true },
  { name: "LICENSE", folder: false },
  { name: "README.md", folder: false },
];

/** How many rows the repository's file list shows. */
export const ABOUT_FILE_ROWS = ABOUT_FILES.length;

const PAGE_IN = 150;
const PAGE_IN_MS = 450;
/** The file list's rows come in one after another once the page is mostly in place. */
const ROWS_START = PAGE_IN + 250;
const ROW_STAGGER_MS = 70;
const ROW_MS = 260;
const ROWS_END = ROWS_START + (ABOUT_FILE_ROWS - 1) * ROW_STAGGER_MS + ROW_MS;
/** How long the page rests once it is shown before the tap on Star lands. */
const STAR_HOLD_MS = 800;
const STAR_TAP = ROWS_END + STAR_HOLD_MS;
/** The button sinks as the tap lands and comes back up as the star fills. */
const PRESS_IN_MS = 80;
const PRESS_OUT_MS = 180;
const STARRED = STAR_TAP + PRESS_IN_MS;
/** The star pops as it fills, and sparks burst from it. */
const POP_MS = 420;
const SPARKS_MS = 560;
const END_REST_MS = 1800;

/** Moments of the scene, in milliseconds from its start. */
export const ABOUT_BEATS = {
  rowsEnd: ROWS_END,
  starTap: STAR_TAP,
  starred: STARRED,
  end: STARRED + SPARKS_MS + END_REST_MS,
} as const;

/** How far the repository page has come into view, from 0 to 1. */
export function aboutPage(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, PAGE_IN, PAGE_IN + PAGE_IN_MS));
}

/** How far row `row` of the file list has come into view, from 0 to 1. */
export function aboutRow(elapsed: number, row: number): number {
  const start = ROWS_START + row * ROW_STAGGER_MS;
  return easeInOutCubic(filmSpan(elapsed, start, start + ROW_MS));
}

/** How far the Star button is pressed in, from 0 to 1. */
export function aboutPress(elapsed: number): number {
  if (elapsed < STARRED) return easeInOutCubic(filmSpan(elapsed, STAR_TAP, STARRED));
  return 1 - easeInOutCubic(filmSpan(elapsed, STARRED, STARRED + PRESS_OUT_MS));
}

/** Whether the repository is starred at this moment. */
export function aboutStarred(elapsed: number): boolean {
  return elapsed >= STARRED;
}

/** How far the star's pop has run as it fills, from 0 to 1. */
export function aboutPop(elapsed: number): number {
  return filmSpan(elapsed, STARRED, STARRED + POP_MS);
}

/** How far the sparks have flown from the star, from 0 to 1. */
export function aboutSparks(elapsed: number): number {
  return filmSpan(elapsed, STARRED, STARRED + SPARKS_MS);
}

/** The `data-cursor` names of the places the mouse pointer goes to: the address bar and the Star button. */
export const ABOUT_CURSOR_TARGETS = { address: "about-address", star: "about-star" } as const;

/**
 * The mouse pointer, drawn on every build since the browser is a desktop one: it rests on the address bar the
 * repository was just opened from, with the text beam, then clicks Star once every row is shown, its button held for a
 * tap's length. Star is a link-like button on the repository host, so the pointer shows the hand over it.
 */
export const ABOUT_CURSOR: WelcomeCursorScript = [
  cursorRest(ABOUT_CURSOR_TARGETS.address, 0, { anchor: [0.62, 0.5], glyph: "text" }),
  cursorClick(ABOUT_CURSOR_TARGETS.star, STAR_TAP + FILM_TAP_MS, { glyph: "pointer", after: ROWS_END }),
];
