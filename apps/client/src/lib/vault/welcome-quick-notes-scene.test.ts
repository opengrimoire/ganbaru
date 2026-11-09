import { describe, expect, it } from "vitest";
import { PALETTE_SIZE } from "$lib/calendar/types";
import { welcomeCursorHovered, welcomeCursorScriptOrdered } from "./welcome-cursor";
import { FILM_TAP_MS } from "./welcome-film";
import {
  QUICK_NOTES_BEATS,
  QUICK_NOTES_CAST,
  QUICK_NOTES_CURSOR,
  QUICK_NOTES_FILTER_FADE_MS,
  QUICK_NOTES_FILTERED_NOTES,
  QUICK_NOTES_GLIDE_MS,
  QUICK_NOTES_MOBILE_END,
  QUICK_NOTES_MOBILE_FILTER_TAP,
  QUICK_NOTES_RECOLOR_MS,
  QUICK_NOTES_TAG_ORDER,
  QUICK_NOTES_TAGGED_NOTES,
  quickNotesFilter,
  quickNotesFiltered,
  quickNotesPickerOpen,
  quickNotesSceneColors,
  quickNotesSceneEnd,
  quickNotesSceneStill,
  quickNotesTouches,
  quickNotesTripProgress,
} from "./welcome-quick-notes-scene";

const B = QUICK_NOTES_BEATS;

/** Every step of a scene, for properties that must hold throughout. */
function moments(end: number, step = 10): number[] {
  return Array.from({ length: Math.floor(end / step) + 1 }, (_, index) => index * step);
}

describe("welcome quick notes scene", () => {
  it("keeps its beats in order", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
  });

  it("keeps every fraction between 0 and 1", () => {
    for (const mobile of [false, true]) {
      for (const elapsed of moments(quickNotesSceneEnd(mobile))) {
        const filter = quickNotesFilter(elapsed, mobile);
        const values = [filter.fade, filter.glide, quickNotesTouches(elapsed).filter];
        for (const key of QUICK_NOTES_TAGGED_NOTES) values.push(quickNotesTripProgress(key, elapsed, mobile));
        for (const value of values) {
          expect(value).toBeGreaterThanOrEqual(0);
          expect(value).toBeLessThanOrEqual(1);
        }
      }
    }
  });

  it("starts with notes already tagged Trip, and the notes it tags untagged and apart", () => {
    const keys = QUICK_NOTES_CAST.map((card) => card.key);
    expect(QUICK_NOTES_CAST.some((card) => card.tag === "trip")).toBe(true);
    for (const key of QUICK_NOTES_TAGGED_NOTES) {
      expect(QUICK_NOTES_CAST.find((card) => card.key === key)?.tag).toBeNull();
    }
    const [first, second] = QUICK_NOTES_TAGGED_NOTES.map((key) => keys.indexOf(key));
    expect(keys.indexOf("gift")).toBeLessThan(first);
    expect(second - first).toBeGreaterThan(1);
  });

  it("leads its tags with Trip and shows every tag on some note from the start", () => {
    expect(QUICK_NOTES_TAG_ORDER[0]).toBe("trip");
    expect(new Set(QUICK_NOTES_TAG_ORDER).size).toBe(QUICK_NOTES_TAG_ORDER.length);
    expect(QUICK_NOTES_TAG_ORDER.length).toBeGreaterThan(2);
    for (const tag of QUICK_NOTES_TAG_ORDER) {
      expect(QUICK_NOTES_CAST.some((card) => card.tag === tag)).toBe(true);
    }
    for (const card of QUICK_NOTES_CAST) {
      if (card.tag !== null) expect(card.tone).toBe(card.tag);
    }
  });

  it("filters down to the notes tagged Trip from the start and the ones it tags, in their order", () => {
    const expected = QUICK_NOTES_CAST.filter((card) => card.tag === "trip" || QUICK_NOTES_TAGGED_NOTES.includes(card.key)).map((card) => card.key);
    expect(QUICK_NOTES_FILTERED_NOTES).toEqual(expected);
    for (const key of QUICK_NOTES_TAGGED_NOTES) expect(QUICK_NOTES_FILTERED_NOTES).toContain(key);
    expect(QUICK_NOTES_FILTERED_NOTES.length).toBeLessThan(QUICK_NOTES_CAST.length);
  });

  it("gives each tone its own palette slot, never the default note color", () => {
    for (let defaultColor = 0; defaultColor < PALETTE_SIZE; defaultColor += 1) {
      const colors = Object.values(quickNotesSceneColors(defaultColor));
      expect(colors).not.toContain(defaultColor);
      expect(new Set(colors).size).toBe(colors.length);
      for (const color of colors) {
        expect(color).toBeGreaterThanOrEqual(0);
        expect(color).toBeLessThan(PALETTE_SIZE);
      }
    }
  });

  it("tags each note as Trip is picked from its picker, which then closes", () => {
    const picks = [
      { key: "passport", open: B.passportPickerTap, pick: B.passportPick },
      { key: "hotel", open: B.hotelPickerTap, pick: B.hotelPick },
    ] as const;
    for (const { key, open, pick } of picks) {
      expect(quickNotesPickerOpen(open - 1, false)).toBeNull();
      expect(quickNotesPickerOpen(open, false)).toBe(key);
      expect(quickNotesPickerOpen(pick - 1, false)).toBe(key);
      expect(quickNotesPickerOpen(pick, false)).toBeNull();
      expect(quickNotesTripProgress(key, pick, false)).toBe(0);
      expect(quickNotesTripProgress(key, pick + QUICK_NOTES_RECOLOR_MS, false)).toBe(1);
    }
    expect(quickNotesTripProgress("gift", B.end, false)).toBe(0);
    expect(quickNotesTripProgress("packing", B.end, false)).toBe(0);
    expect(B.passportPick + QUICK_NOTES_RECOLOR_MS).toBeLessThanOrEqual(B.hotelPickerTap);
    expect(B.hotelPick + QUICK_NOTES_RECOLOR_MS).toBeLessThanOrEqual(B.filterTap);
  });

  it("fades the other notes out before the tagged ones glide together, and settles before the scene ends", () => {
    for (const mobile of [false, true]) {
      const tap = mobile ? QUICK_NOTES_MOBILE_FILTER_TAP : B.filterTap;
      expect(quickNotesFiltered(tap - 1, mobile)).toBe(false);
      expect(quickNotesFiltered(tap, mobile)).toBe(true);
      expect(quickNotesFilter(tap, mobile)).toEqual({ fade: 0, glide: 0 });
      expect(quickNotesFilter(tap + QUICK_NOTES_FILTER_FADE_MS, mobile)).toEqual({ fade: 1, glide: 0 });
      const settled = tap + QUICK_NOTES_FILTER_FADE_MS + QUICK_NOTES_GLIDE_MS;
      expect(quickNotesFilter(settled, mobile)).toEqual({ fade: 1, glide: 1 });
      expect(settled).toBeLessThan(quickNotesSceneEnd(mobile));
    }
  });

  it("starts a phone's scene with both notes tagged, and taps only its chip", () => {
    for (const key of QUICK_NOTES_TAGGED_NOTES) expect(quickNotesTripProgress(key, 0, true)).toBe(1);
    expect(quickNotesTouches(QUICK_NOTES_MOBILE_FILTER_TAP - FILM_TAP_MS / 2).filter).toBeGreaterThan(0);
    expect(quickNotesSceneEnd(true)).toBe(QUICK_NOTES_MOBILE_END);
  });

  it("shows the filtered notes, every picker closed and no touch, in its still frames", () => {
    for (const mobile of [false, true]) {
      const still = quickNotesSceneStill(mobile);
      expect(quickNotesFilter(still, mobile)).toEqual({ fade: 1, glide: 1 });
      expect(quickNotesPickerOpen(still, mobile)).toBeNull();
      for (const key of QUICK_NOTES_TAGGED_NOTES) expect(quickNotesTripProgress(key, still, mobile)).toBe(1);
      expect(quickNotesTouches(still).filter).toBe(0);
    }
  });

  it("clicks with the desktop pointer on each beat, resting on each control through its press", () => {
    expect(welcomeCursorScriptOrdered(QUICK_NOTES_CURSOR)).toBe(true);
    const clicks = QUICK_NOTES_CURSOR.filter((stop) => stop.press);
    expect(clicks.map((stop) => [stop.target, stop.press?.end])).toEqual([
      ["quick-notes-tag-passport", B.passportPickerTap],
      ["quick-notes-option-trip", B.passportPick],
      ["quick-notes-tag-hotel", B.hotelPickerTap],
      ["quick-notes-option-trip", B.hotelPick],
      ["quick-notes-chip-trip", B.filterTap],
    ]);
    for (const stop of clicks) {
      const press = stop.press;
      if (!press) continue;
      expect(press.start).toBe(press.end - FILM_TAP_MS);
      for (let elapsed = press.start; elapsed <= press.end; elapsed += 10) expect(welcomeCursorHovered(QUICK_NOTES_CURSOR, elapsed)).toBe(stop.target);
    }
    expect(QUICK_NOTES_CURSOR.find((stop) => stop.target === "quick-notes-tag-hotel")?.depart).toBeGreaterThanOrEqual(B.passportPick);
  });
});
