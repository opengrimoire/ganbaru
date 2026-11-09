/**
 * Timing for the welcome scene on Quick notes, where the notes already carry several tags, Trip among them. With a
 * pointer, a note's tag picker tags it Trip, which gives it the tag's color, another note's picker tags it Trip too, and
 * the Trip chip then filters the notes down to the ones tagged Trip. A phone's cards leave their actions to the editor,
 * so its scene starts with both notes tagged and shows the filter alone.
 */

import type { EventColor } from "$lib/calendar/types";
import { cursorClick, cursorRest, type WelcomeCursorScript } from "./welcome-cursor";
import { easeInOutCubic, filmSpan, filmTap } from "./welcome-film";
import { PROJECTS_CURSOR_REST_ANCHOR } from "./welcome-projects-scene";

/** The scene's tags, all of which exist from the start. */
export type QuickNotesSceneTag = "trip" | "ideas" | "home" | "health";

/** The tags in the order the tag bar and the tag pickers list them, Trip first so it stays in view when the rest collapse. */
export const QUICK_NOTES_TAG_ORDER: readonly QuickNotesSceneTag[] = ["trip", "ideas", "home", "health"];

/** The colors a scene note can have besides the default one new notes start with. */
export type QuickNotesSceneTone = QuickNotesSceneTag | "accent";

/** The scene's notes, by the keys of their text. */
export type QuickNotesSceneNote =
  | "gift"
  | "passport"
  | "pitch"
  | "packing"
  | "dentist"
  | "hotel"
  | "plants"
  | "sintra"
  | "groceries"
  | "recipe";

/** A note as the scene starts: its tag, and its own color while it has none, the default color when null. */
export interface QuickNotesSceneCard {
  key: QuickNotesSceneNote;
  tag: QuickNotesSceneTag | null;
  tone: QuickNotesSceneTone | null;
}

/** The notes in their order, which places the Trip notes apart, so the filter brings them together. */
export const QUICK_NOTES_CAST: readonly QuickNotesSceneCard[] = [
  { key: "gift", tag: null, tone: "accent" },
  { key: "passport", tag: null, tone: null },
  { key: "pitch", tag: "ideas", tone: "ideas" },
  { key: "packing", tag: "trip", tone: "trip" },
  { key: "dentist", tag: "health", tone: "health" },
  { key: "hotel", tag: null, tone: null },
  { key: "plants", tag: "ideas", tone: "ideas" },
  { key: "sintra", tag: "trip", tone: "trip" },
  { key: "groceries", tag: "home", tone: "home" },
  { key: "recipe", tag: null, tone: "accent" },
];

/** The notes the scene tags Trip, in the order it tags them. */
export const QUICK_NOTES_TAGGED_NOTES: readonly QuickNotesSceneNote[] = ["passport", "hotel"];

/** The notes the Trip filter keeps, in their order: those tagged Trip from the start and those the scene tags. */
export const QUICK_NOTES_FILTERED_NOTES: readonly QuickNotesSceneNote[] = QUICK_NOTES_CAST.filter(
  (card) => card.tag === "trip" || QUICK_NOTES_TAGGED_NOTES.includes(card.key),
).map((card) => card.key);

/** The palette slots the scene prefers for each color, in order, so one taken by the default color has a fallback. */
const TONE_PREFERENCES: Readonly<Record<QuickNotesSceneTone, readonly EventColor[]>> = {
  ideas: [8, 7, 9],
  trip: [18, 19, 16],
  home: [12, 11, 13],
  health: [4, 1, 5],
  accent: [23, 24, 22],
};

/**
 * The scene's colors for a default note color: each tone takes its first preferred palette slot that no other tone
 * and not the default color use, as a new tag avoids the colors already in use.
 */
export function quickNotesSceneColors(defaultColor: EventColor): Readonly<Record<QuickNotesSceneTone, EventColor>> {
  const taken = new Set<EventColor>([defaultColor]);
  const pick = (tone: QuickNotesSceneTone): EventColor => {
    const preferred = TONE_PREFERENCES[tone];
    const color = preferred.find((slot) => !taken.has(slot)) ?? preferred[0];
    taken.add(color);
    return color;
  };
  const trip = pick("trip");
  const ideas = pick("ideas");
  const home = pick("home");
  const health = pick("health");
  return { trip, ideas, home, health, accent: pick("accent") };
}

/** Moments of the desktop scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const QUICK_NOTES_BEATS = {
  /** The passport note's tag picker opens, and Trip is picked. */
  passportPickerTap: 1100,
  passportPick: 2000,
  /** The hotel note's tag picker opens, and Trip is picked. */
  hotelPickerTap: 3000,
  hotelPick: 3900,
  /** The Trip chip filters the notes down to the ones tagged Trip. */
  filterTap: 4900,
  end: 6800,
} as const;

const B = QUICK_NOTES_BEATS;

/** When a phone's Trip chip is tapped, and when its scene ends. */
export const QUICK_NOTES_MOBILE_FILTER_TAP = 1300;
export const QUICK_NOTES_MOBILE_END = 3600;

/** How long the last frame rests before the scene starts over. */
export const QUICK_NOTES_END_HOLD_MS = 900;

/** How long a tagged note takes to change to its tag's color and make room for its badge. */
export const QUICK_NOTES_RECOLOR_MS = 260;

/** How long the notes the filter hides take to fade out, and how long the rest then take to glide into place. */
export const QUICK_NOTES_FILTER_FADE_MS = 200;
export const QUICK_NOTES_GLIDE_MS = 420;

/** How long a run of the scene lasts on a phone or with a pointer. */
export function quickNotesSceneEnd(mobile: boolean): number {
  return mobile ? QUICK_NOTES_MOBILE_END : B.end;
}

/** The frame shown as a still with reduced motion: the notes filtered down to the ones tagged Trip. */
export function quickNotesSceneStill(mobile: boolean): number {
  return quickNotesSceneEnd(mobile);
}

/** The note whose tag picker is open, or null while none is, as on a phone, whose cards leave their actions to the editor. */
export function quickNotesPickerOpen(elapsed: number, mobile: boolean): QuickNotesSceneNote | null {
  if (mobile) return null;
  if (elapsed >= B.passportPickerTap && elapsed < B.passportPick) return "passport";
  if (elapsed >= B.hotelPickerTap && elapsed < B.hotelPick) return "hotel";
  return null;
}

/** When each note the scene tags gets the Trip tag with a pointer. */
const TRIP_TAGGED_AT: Readonly<Partial<Record<QuickNotesSceneNote, number>>> = { passport: B.passportPick, hotel: B.hotelPick };

/**
 * How far a note the scene tags has taken the Trip tag, from 0 to 1: it fades to the tag's color and makes room for its
 * badge. A phone's tagged notes have it from the start, and the other notes never take it.
 */
export function quickNotesTripProgress(key: QuickNotesSceneNote, elapsed: number, mobile: boolean): number {
  const taggedAt = TRIP_TAGGED_AT[key];
  if (taggedAt === undefined) return 0;
  return mobile ? 1 : easeInOutCubic(filmSpan(elapsed, taggedAt, taggedAt + QUICK_NOTES_RECOLOR_MS));
}

/** When the Trip chip's tap lands. */
export function quickNotesFilterTap(mobile: boolean): number {
  return mobile ? QUICK_NOTES_MOBILE_FILTER_TAP : B.filterTap;
}

/** Whether the Trip chip is selected rather than All. */
export function quickNotesFiltered(elapsed: number, mobile: boolean): boolean {
  return elapsed >= quickNotesFilterTap(mobile);
}

/** How far the filter has gone, each from 0 to 1: the hidden notes fade out, then the Trip notes glide into place. */
export interface QuickNotesSceneFilter {
  fade: number;
  glide: number;
}

/** The filter at `elapsed`. */
export function quickNotesFilter(elapsed: number, mobile: boolean): QuickNotesSceneFilter {
  const tap = quickNotesFilterTap(mobile);
  const glideStart = tap + QUICK_NOTES_FILTER_FADE_MS;
  return {
    fade: filmSpan(elapsed, tap, glideStart),
    glide: easeInOutCubic(filmSpan(elapsed, glideStart, glideStart + QUICK_NOTES_GLIDE_MS)),
  };
}

/** The scene's touches, each from 0 to 1 as it shows. */
export interface QuickNotesSceneTouches {
  filter: number;
}

/** A phone scene's touches at `elapsed`. */
export function quickNotesTouches(elapsed: number): QuickNotesSceneTouches {
  return { filter: filmTap(elapsed, QUICK_NOTES_MOBILE_FILTER_TAP) };
}

/**
 * The desktop pointer's path through the scene: it moves onto the passport note, whose actions show under it, opens
 * its tag picker and picks Trip, does the same for the hotel note, then clicks the Trip chip.
 */
export const QUICK_NOTES_CURSOR: WelcomeCursorScript = [
  cursorRest("quick-notes-panel", 0, { anchor: PROJECTS_CURSOR_REST_ANCHOR }),
  cursorClick("quick-notes-tag-passport", B.passportPickerTap),
  cursorClick("quick-notes-option-trip", B.passportPick, { after: B.passportPickerTap }),
  cursorClick("quick-notes-tag-hotel", B.hotelPickerTap, { after: B.passportPick }),
  cursorClick("quick-notes-option-trip", B.hotelPick, { after: B.hotelPickerTap }),
  cursorClick("quick-notes-chip-trip", B.filterTap, { after: B.hotelPick }),
];
