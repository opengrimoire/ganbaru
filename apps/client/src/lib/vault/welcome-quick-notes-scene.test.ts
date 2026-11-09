import { describe, expect, it } from "vitest";
import { PALETTE_SIZE } from "$lib/calendar/types";
import { welcomeCursorHovered, welcomeCursorScriptOrdered } from "./welcome-cursor";
import { FILM_TAP_MS } from "./welcome-film";
import {
  QUICK_NOTES_BEATS,
  QUICK_NOTES_CAST,
  QUICK_NOTES_CURSOR,
  QUICK_NOTES_FILTER_FADE_MS,
  QUICK_NOTES_GLIDE_MS,
  QUICK_NOTES_MOBILE_END,
  QUICK_NOTES_MOBILE_FILTER_TAP,
  QUICK_NOTES_RECOLOR_MS,
  QUICK_NOTES_TRIP_NOTES,
  quickNotesCreatingTag,
  quickNotesFilter,
  quickNotesFiltered,
  quickNotesPickerOpen,
  quickNotesSceneColors,
  quickNotesSceneEnd,
  quickNotesSceneStill,
  quickNotesTagCaretOn,
  quickNotesTagDraft,
  quickNotesTouches,
  quickNotesTripChipShown,
  quickNotesTripCreated,
  quickNotesTripProgress,
} from "./welcome-quick-notes-scene";

const B = QUICK_NOTES_BEATS;
const NAME = "Trip";

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
        const values = [filter.fade, filter.glide, quickNotesTripChipShown(elapsed, mobile), quickNotesTouches(elapsed).filter];
        for (const key of QUICK_NOTES_TRIP_NOTES) values.push(quickNotesTripProgress(key, elapsed, mobile));
        for (const value of values) {
          expect(value).toBeGreaterThanOrEqual(0);
          expect(value).toBeLessThanOrEqual(1);
        }
      }
    }
  });

  it("starts the notes it tags untagged and apart, with other notes between them", () => {
    const keys = QUICK_NOTES_CAST.map((card) => card.key);
    for (const key of QUICK_NOTES_TRIP_NOTES) {
      expect(QUICK_NOTES_CAST.find((card) => card.key === key)?.tag).toBeNull();
    }
    const [first, second] = QUICK_NOTES_TRIP_NOTES.map((key) => keys.indexOf(key));
    expect(keys.indexOf("gift")).toBeLessThan(first);
    expect(second - first).toBeGreaterThan(1);
    expect(QUICK_NOTES_CAST.every((card) => card.tag !== "trip")).toBe(true);
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

  it("creates the tag from the passport note's picker, typing its name and keeping it with Enter", () => {
    expect(quickNotesPickerOpen(B.passportPickerTap - 1)).toBeNull();
    expect(quickNotesPickerOpen(B.passportPickerTap)).toBe("passport");
    expect(quickNotesCreatingTag(B.newTagTap - 1)).toBe(false);
    expect(quickNotesCreatingTag(B.newTagTap)).toBe(true);
    expect(quickNotesTagDraft(B.tagTypeStart, NAME)).toBe("");
    expect(quickNotesTagDraft(B.tagTypeEnd, NAME)).toBe(NAME);
    expect(quickNotesTagCaretOn((B.tagTypeStart + B.tagTypeEnd) / 2)).toBe(true);
    expect(quickNotesTripCreated(B.tagSubmit - 1, false)).toBe(false);
    expect(quickNotesTripCreated(B.tagSubmit, false)).toBe(true);
    expect(quickNotesPickerOpen(B.tagSubmit)).toBeNull();
    expect(quickNotesCreatingTag(B.tagSubmit)).toBe(false);
    expect(quickNotesTagCaretOn(B.tagSubmit)).toBe(false);
    expect(quickNotesTripChipShown(B.tagSubmit, false)).toBe(0);
    expect(quickNotesTripChipShown(B.end, false)).toBe(1);
  });

  it("tags each note as its picker closes, the passport on creating the tag and the hotel on picking it", () => {
    expect(quickNotesTripProgress("passport", B.tagSubmit, false)).toBe(0);
    expect(quickNotesTripProgress("passport", B.tagSubmit + QUICK_NOTES_RECOLOR_MS, false)).toBe(1);
    expect(quickNotesTripProgress("hotel", B.hotelPick - 1, false)).toBe(0);
    expect(quickNotesPickerOpen(B.hotelPick - 1)).toBe("hotel");
    expect(quickNotesPickerOpen(B.hotelPick)).toBeNull();
    expect(quickNotesTripProgress("hotel", B.hotelPick + QUICK_NOTES_RECOLOR_MS, false)).toBe(1);
    expect(quickNotesTripProgress("gift", B.end, false)).toBe(0);
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

  it("starts a phone's scene with the tag and both notes tagged, and taps only its chip", () => {
    expect(quickNotesTripCreated(0, true)).toBe(true);
    for (const key of QUICK_NOTES_TRIP_NOTES) expect(quickNotesTripProgress(key, 0, true)).toBe(1);
    expect(quickNotesTouches(QUICK_NOTES_MOBILE_FILTER_TAP - FILM_TAP_MS / 2).filter).toBeGreaterThan(0);
    expect(quickNotesSceneEnd(true)).toBe(QUICK_NOTES_MOBILE_END);
  });

  it("shows the filtered notes, every picker closed and no touch, in its still frames", () => {
    for (const mobile of [false, true]) {
      const still = quickNotesSceneStill(mobile);
      expect(quickNotesFilter(still, mobile)).toEqual({ fade: 1, glide: 1 });
      expect(quickNotesPickerOpen(still)).toBeNull();
      for (const key of QUICK_NOTES_TRIP_NOTES) expect(quickNotesTripProgress(key, still, mobile)).toBe(1);
      expect(quickNotesTouches(still).filter).toBe(0);
    }
  });

  it("clicks with the desktop pointer on each beat, resting on each control through its press", () => {
    expect(welcomeCursorScriptOrdered(QUICK_NOTES_CURSOR)).toBe(true);
    const clicks = QUICK_NOTES_CURSOR.filter((stop) => stop.press);
    expect(clicks.map((stop) => [stop.target, stop.press?.end])).toEqual([
      ["quick-notes-tag-passport", B.passportPickerTap],
      ["quick-notes-new-tag", B.newTagTap],
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
    expect(QUICK_NOTES_CURSOR.find((stop) => stop.target === "quick-notes-tag-hotel")?.depart).toBeGreaterThanOrEqual(B.tagSubmit);
  });
});
