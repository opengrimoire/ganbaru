/**
 * The mouse pointer the desktop welcome scenes draw in place of touches. A scene lists the places the pointer goes, each
 * a named element of the drawing and the moments it leaves for it and reaches it, so the pointer's path and clicks are a
 * pure function of the scene's time, as the rest of the scene is. The drawing measures where each named element is, so
 * the pointer lands on it at whatever size the scene is drawn. Phone screens keep their touches.
 */

import { easeInOutCubic, FILM_TAP_MS, filmPress, filmSpan } from "./welcome-film";

/** The pointer's shapes: the arrow, the hand over a link-like control, and the text beam over a text field. */
export type WelcomeCursorGlyph = "arrow" | "pointer" | "text";

/** A span of scene time, in milliseconds from the scene's start. */
export interface WelcomeCursorSpan {
  start: number;
  end: number;
}

/** One place the pointer goes. */
export interface WelcomeCursorStop {
  /** The `data-cursor` name of the element the pointer goes to. */
  target: string;
  /** Where on the element it lands, as fractions of its width and height from its top left corner. */
  anchor?: readonly [x: number, y: number];
  /** When the pointer leaves the previous place for this one, and when it gets here. */
  depart: number;
  arrive: number;
  /** The pointer's shape while it rests here. */
  glyph?: WelcomeCursorGlyph;
  /**
   * When the button is held, such as a click's, or a drag's while the pointer follows the element it drags out. A
   * held button may start before `depart`, while the pointer still rests on the place before.
   */
  press?: WelcomeCursorSpan;
  /**
   * Whether the pointer stays on the element while it travels here and after, as when it drags the element's edge
   * out. Otherwise the pointer stays where it lands, and the drawing may scroll under it as it does under a mouse.
   */
  follow?: boolean;
  /** When the pointer is in use while it rests here, such as while it scrolls what is under it, so it does not hide then. */
  busy?: readonly WelcomeCursorSpan[];
}

/** The places a scene's pointer goes, in order. The first is where it rests as the scene starts. */
export type WelcomeCursorScript = readonly WelcomeCursorStop[];

/** The pointer at one moment: travelling from `from` to `to` by `progress`, with its shape and how far it is held. */
export interface WelcomeCursorFrame {
  from: WelcomeCursorStop | null;
  to: WelcomeCursorStop;
  /** How far along the path from `from` to `to` the pointer is, eased, from 0 to 1. */
  progress: number;
  glyph: WelcomeCursorGlyph;
  /** How far the button shows as held, from 0 to 1. */
  pressed: number;
  /** How far the pointer shows, from 0 to 1: it fades in as the scene starts, and out while it rests idle. */
  shown: number;
  /** Whether the pointer has hidden while resting idle, so it hovers nothing. */
  away: boolean;
}

/** The pointer's drawn size in logical pixels, and its smaller size in a drawn browser, whose page is drawn smaller. */
export const WELCOME_CURSOR_SIZE_PX = 20;
export const WELCOME_CURSOR_BROWSER_SIZE_PX = 17;

/** How long the pointer takes to reach the next place by default. */
export const WELCOME_CURSOR_TRAVEL_MS = 520;

/** How long the pointer rests on a control before its click by default, as a person aims before clicking. */
export const WELCOME_CURSOR_REST_MS = 260;

/** The shortest travel a click allows when it has to wait for its control, so the pointer never jumps. */
export const WELCOME_CURSOR_MIN_TRAVEL_MS = 200;

/** How long before a scroll ends the pointer may leave for the control it brings into view, chasing it as it settles. */
export const WELCOME_CURSOR_SCROLL_OVERLAP_MS = 200;

/**
 * How long the pointer rests after its last move or click before it hides, when nothing needs it for a while, as a
 * system pointer left alone does. It shows again in place just before it next moves.
 */
export const WELCOME_CURSOR_IDLE_MS = 600;

/** How long the pointer takes to fade out as it hides, and back in as it shows. */
export const WELCOME_CURSOR_FADE_MS = 250;

/** The shortest time the pointer stays hidden, so it never blinks out and straight back in. */
const WELCOME_CURSOR_AWAY_MIN_MS = 600;

/** How far along a path the pointer takes the next place's shape, or still keeps the last one's, as it crosses edges. */
const GLYPH_EDGE = 0.9;

/** Options for {@link cursorClick}. */
export interface WelcomeCursorClickOptions {
  anchor?: readonly [x: number, y: number];
  glyph?: WelcomeCursorGlyph;
  /** How long the pointer rests on the control before the button goes down. */
  rest?: number;
  /** How long the pointer travels to the control. */
  travel?: number;
  /** The earliest the pointer may leave for the control, such as once a scroll that brings the control into view ends. */
  after?: number;
  /** When the pointer is in use while it rests on the place, such as while it scrolls what is under it. */
  busy?: readonly WelcomeCursorSpan[];
}

/**
 * A click on `target` landing at `tap`, as the scenes time their taps: the button is down for the tap's length and
 * comes up as the tap lands, after the pointer has travelled to the control and rested on it.
 */
export function cursorClick(target: string, tap: number, options: WelcomeCursorClickOptions = {}): WelcomeCursorStop {
  const { anchor, glyph, rest = WELCOME_CURSOR_REST_MS, travel = WELCOME_CURSOR_TRAVEL_MS, after, busy } = options;
  const pressStart = tap - FILM_TAP_MS;
  const latestArrive = pressStart - rest;
  // A late start eats into the rest before the travel, so the pointer still glides rather than jumps.
  const depart = Math.min(Math.max(after ?? -Infinity, latestArrive - travel), pressStart - WELCOME_CURSOR_MIN_TRAVEL_MS);
  const arrive = Math.min(pressStart, Math.max(latestArrive, depart + WELCOME_CURSOR_MIN_TRAVEL_MS));
  return { target, anchor, glyph, depart, arrive, press: { start: pressStart, end: tap }, busy };
}

/** A place the pointer moves to and rests on without clicking, such as a control it hovers or a neutral spot. */
export function cursorRest(
  target: string,
  arrive: number,
  options: Omit<WelcomeCursorClickOptions, "rest" | "after"> = {},
): WelcomeCursorStop {
  const { anchor, glyph, travel = WELCOME_CURSOR_TRAVEL_MS, busy } = options;
  return { target, anchor, glyph, depart: Math.max(0, arrive - travel), arrive, busy };
}

/** The pointer at `elapsed`. */
export function welcomeCursorFrame(script: WelcomeCursorScript, elapsed: number): WelcomeCursorFrame {
  let index = 0;
  while (index + 1 < script.length && elapsed >= script[index + 1].depart) index += 1;
  const to = script[index];
  const from = index > 0 ? script[index - 1] : null;
  const progress = from === null ? 1 : easeInOutCubic(filmSpan(elapsed, to.depart, to.arrive));
  let glyph: WelcomeCursorGlyph = "arrow";
  if (progress >= GLYPH_EDGE) glyph = to.glyph ?? "arrow";
  else if (from !== null && progress <= 1 - GLYPH_EDGE) glyph = from.glyph ?? "arrow";
  const pressed = Math.max(0, ...script.map((stop) => (stop.press ? filmPress(elapsed, stop.press.start, stop.press.end) : 0)));
  const present = welcomeCursorPresent(to, script[index + 1]?.depart ?? Infinity, elapsed);
  return { from, to, progress, glyph, pressed, shown: Math.min(present, filmSpan(elapsed, 0, WELCOME_CURSOR_FADE_MS)), away: present === 0 };
}

/**
 * How far the pointer shows while it is at `stop` until it leaves at `leave`, from 0 to 1. When a wait between its
 * moves, clicks, and other uses is long enough, it fades out once it has rested idle and fades back in just before it
 * is next used.
 */
function welcomeCursorPresent(stop: WelcomeCursorStop, leave: number, elapsed: number): number {
  const uses: WelcomeCursorSpan[] = [{ start: stop.depart, end: stop.arrive }, ...(stop.press ? [stop.press] : []), ...(stop.busy ?? [])];
  const idleFrom = Math.max(...uses.filter((use) => use.start <= elapsed).map((use) => use.end));
  const nextUse = Math.min(leave, ...uses.filter((use) => use.start > elapsed).map((use) => use.start));
  if (elapsed < idleFrom || nextUse - idleFrom < WELCOME_CURSOR_IDLE_MS + 2 * WELCOME_CURSOR_FADE_MS + WELCOME_CURSOR_AWAY_MIN_MS) {
    return 1;
  }
  const returns = nextUse - WELCOME_CURSOR_FADE_MS;
  if (elapsed >= returns) return filmSpan(elapsed, returns, nextUse);
  const hides = idleFrom + WELCOME_CURSOR_IDLE_MS;
  return 1 - filmSpan(elapsed, hides, hides + WELCOME_CURSOR_FADE_MS);
}

/** Whether the pointer rests on `target` at `elapsed`: it has reached it and not yet left, as a hover sees it. */
export function welcomeCursorOn(script: WelcomeCursorScript, elapsed: number, target: string): boolean {
  return welcomeCursorHovered(script, elapsed) === target;
}

/**
 * Whether a script's places come one after another: each arrives after it leaves, and leaves only once the last one
 * has arrived and its press has let go.
 */
export function welcomeCursorScriptOrdered(script: WelcomeCursorScript): boolean {
  return script.every((stop, index) => {
    if (stop.arrive < stop.depart) return false;
    if (stop.press && stop.press.end < stop.press.start) return false;
    if (index === 0) return true;
    const last = script[index - 1];
    return stop.depart >= last.arrive && stop.depart >= (last.press?.end ?? last.arrive);
  });
}

/** The `data-cursor` name of the place the pointer rests on at `elapsed`, as a hover sees it, or null while it travels or has hidden. */
export function welcomeCursorHovered(script: WelcomeCursorScript, elapsed: number): string | null {
  const { from, to, progress, away } = welcomeCursorFrame(script, elapsed);
  if (away) return null;
  if (progress >= GLYPH_EDGE) return to.target;
  return from !== null && progress <= 1 - GLYPH_EDGE ? from.target : null;
}
