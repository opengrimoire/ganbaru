/**
 * Timing for the welcome scene about the app itself: the app's public source repository shows in a browser from the
 * start, with the pointer already on Star, and a mouse click stars it. Every moment is derived from the milliseconds elapsed since the scene
 * started, so a frame is a pure function of time and the reduced-motion still is simply the last frame.
 */

import { cursorClick, type WelcomeCursorScript } from "./welcome-cursor";
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

/** How long the page rests, with the pointer on Star, before the button goes down. */
const STAR_TAP = 1000;
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
  starTap: STAR_TAP,
  starred: STARRED,
  end: STARRED + SPARKS_MS + END_REST_MS,
} as const;

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

/** The `data-cursor` name of the Star button, where the mouse pointer rests and clicks. */
export const ABOUT_CURSOR_TARGETS = { star: "about-star" } as const;

/**
 * The mouse pointer, drawn on every build since the browser is a desktop one: it rests on Star from the start and
 * clicks it in place, its button held for a tap's length. Star is a link-like button on the repository host, so the
 * pointer shows the hand over it.
 */
export const ABOUT_CURSOR: WelcomeCursorScript = [
  cursorClick(ABOUT_CURSOR_TARGETS.star, STAR_TAP + FILM_TAP_MS, { glyph: "pointer", rest: STAR_TAP }),
];
