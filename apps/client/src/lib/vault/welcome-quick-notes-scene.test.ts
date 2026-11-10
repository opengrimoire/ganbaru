import { describe, expect, it } from "vitest";
import { PALETTE_SIZE } from "$lib/calendar/types";
import { welcomeCursorHovered, welcomeCursorScriptOrdered } from "./welcome-cursor";
import { FILM_TAP_MS } from "./welcome-film";
import {
  QUICK_NOTES_BEATS,
  QUICK_NOTES_CAST,
  QUICK_NOTES_CURSOR,
  QUICK_NOTES_EDITOR_FADE_MS,
  QUICK_NOTES_FILTER_FADE_MS,
  QUICK_NOTES_FILTERED_NOTES,
  QUICK_NOTES_GLIDE_MS,
  QUICK_NOTES_INSERT_MS,
  QUICK_NOTES_MOBILE_BEATS,
  QUICK_NOTES_NEW_NOTE,
  QUICK_NOTES_RECOLOR_MS,
  QUICK_NOTES_TAG_ORDER,
  QUICK_NOTES_TAGGED_NOTES,
  quickNotesEditor,
  quickNotesFilter,
  quickNotesFiltered,
  quickNotesInserted,
  quickNotesPickerOpen,
  quickNotesSceneBeats,
  quickNotesSceneColors,
  quickNotesSceneEnd,
  quickNotesSceneStill,
  quickNotesTouches,
  quickNotesTripProgress,
} from "./welcome-quick-notes-scene";

const B = QUICK_NOTES_BEATS;
const M = QUICK_NOTES_MOBILE_BEATS;

/** Every step of a scene, for properties that must hold throughout. */
function moments(end: number, step = 10): number[] {
  return Array.from({ length: Math.floor(end / step) + 1 }, (_, index) => index * step);
}

describe("welcome quick notes scene", () => {
  it("keeps its beats in order", () => {
    for (const beats of [Object.values(B), Object.values(M)]) {
      expect(beats).toEqual([...beats].sort((a, b) => a - b));
    }
  });

  it("keeps every fraction between 0 and 1", () => {
    for (const mobile of [false, true]) {
      for (const elapsed of moments(quickNotesSceneEnd(mobile))) {
        const filter = quickNotesFilter(elapsed, mobile);
        const touches = quickNotesTouches(elapsed);
        const values = [filter.fade, filter.glide, touches.filter, touches.composer, touches.close, quickNotesInserted(elapsed, mobile)];
        const editor = quickNotesEditor(elapsed, mobile, "Title", "Body");
        if (editor) values.push(editor.opacity);
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
      const tap = mobile ? M.filterTap : B.filterTap;
      expect(quickNotesFiltered(tap - 1, mobile)).toBe(false);
      expect(quickNotesFiltered(tap, mobile)).toBe(true);
      expect(quickNotesFilter(tap, mobile)).toEqual({ fade: 0, glide: 0 });
      expect(quickNotesFilter(tap + QUICK_NOTES_FILTER_FADE_MS, mobile)).toEqual({ fade: 1, glide: 0 });
      const settled = tap + QUICK_NOTES_FILTER_FADE_MS + QUICK_NOTES_GLIDE_MS;
      expect(quickNotesFilter(settled, mobile)).toEqual({ fade: 1, glide: 1 });
      expect(settled).toBeLessThan(quickNotesSceneBeats(mobile).composerTap);
    }
  });

  it("writes a new note of its own that the Trip filter keeps", () => {
    expect(QUICK_NOTES_CAST.map((card) => card.key)).not.toContain(QUICK_NOTES_NEW_NOTE.key);
    expect(QUICK_NOTES_NEW_NOTE.tag).toBe("trip");
    expect(QUICK_NOTES_NEW_NOTE.tone).toBe("trip");
  });

  it("opens the editor on the composer's tap, types the title then the body, and closes it on Close's tap", () => {
    const title = "Museum tickets";
    const body = "Book the tile museum for Saturday.";
    for (const mobile of [false, true]) {
      const beats = quickNotesSceneBeats(mobile);
      expect(quickNotesEditor(beats.composerTap - 1, mobile, title, body)).toBeNull();
      expect(quickNotesEditor(beats.composerTap, mobile, title, body)).toMatchObject({ opacity: 0, title: "", body: "", field: "title", caretOn: true });
      expect(quickNotesEditor(beats.composerTap + QUICK_NOTES_EDITOR_FADE_MS, mobile, title, body)?.opacity).toBe(1);
      expect(beats.composerTap + QUICK_NOTES_EDITOR_FADE_MS).toBeLessThanOrEqual(beats.titleTypeStart);
      const titleTyping = quickNotesEditor((beats.titleTypeStart + beats.titleTypeEnd) / 2, mobile, title, body);
      expect(title.startsWith(titleTyping?.title ?? "")).toBe(true);
      expect(titleTyping?.title.length).toBeGreaterThan(0);
      expect(titleTyping?.title.length).toBeLessThan(title.length);
      expect(quickNotesEditor(beats.bodyTypeStart - 1, mobile, title, body)).toMatchObject({ title, body: "", field: "title" });
      expect(quickNotesEditor(beats.bodyTypeStart, mobile, title, body)).toMatchObject({ field: "body", caretOn: true });
      expect(quickNotesEditor(beats.bodyTypeEnd, mobile, title, body)).toMatchObject({ title, body, field: "body" });
      expect(quickNotesEditor(beats.closeTap, mobile, title, body)?.opacity).toBe(1);
      expect(quickNotesEditor(beats.closeTap + QUICK_NOTES_EDITOR_FADE_MS / 2, mobile, title, body)?.opacity).toBeCloseTo(0.5);
      expect(quickNotesEditor(beats.closeTap + QUICK_NOTES_EDITOR_FADE_MS, mobile, title, body)).toBeNull();
    }
  });

  it("blinks the caret once its field's typing ends", () => {
    const beats = quickNotesSceneBeats(false);
    const frames = moments(beats.closeTap - beats.bodyTypeEnd).map((offset) => quickNotesEditor(beats.bodyTypeEnd + offset, false, "Title", "Body"));
    expect(frames.some((frame) => frame?.caretOn === false)).toBe(true);
    expect(frames.some((frame) => frame?.caretOn === true)).toBe(true);
  });

  it("inserts the new note only once the editor has faded out, and settles before the scene ends", () => {
    for (const mobile of [false, true]) {
      const beats = quickNotesSceneBeats(mobile);
      const start = beats.closeTap + QUICK_NOTES_EDITOR_FADE_MS;
      expect(quickNotesInserted(start, mobile)).toBe(0);
      expect(quickNotesInserted(start + QUICK_NOTES_INSERT_MS, mobile)).toBe(1);
      expect(start + QUICK_NOTES_INSERT_MS).toBeLessThan(beats.end);
    }
  });

  it("starts a phone's scene with both notes tagged, and taps its chip, the composer, and Close", () => {
    for (const key of QUICK_NOTES_TAGGED_NOTES) expect(quickNotesTripProgress(key, 0, true)).toBe(1);
    expect(quickNotesTouches(M.filterTap - FILM_TAP_MS / 2).filter).toBeGreaterThan(0);
    expect(quickNotesTouches(M.composerTap - FILM_TAP_MS / 2).composer).toBeGreaterThan(0);
    expect(quickNotesTouches(M.closeTap - FILM_TAP_MS / 2).close).toBeGreaterThan(0);
    expect(quickNotesSceneEnd(true)).toBe(M.end);
  });

  it("shows the filtered notes with the new one, every picker and the editor closed and no touch, in its still frames", () => {
    for (const mobile of [false, true]) {
      const still = quickNotesSceneStill(mobile);
      expect(quickNotesFilter(still, mobile)).toEqual({ fade: 1, glide: 1 });
      expect(quickNotesPickerOpen(still, mobile)).toBeNull();
      for (const key of QUICK_NOTES_TAGGED_NOTES) expect(quickNotesTripProgress(key, still, mobile)).toBe(1);
      expect(quickNotesEditor(still, mobile, "Title", "Body")).toBeNull();
      expect(quickNotesInserted(still, mobile)).toBe(1);
      expect(quickNotesTouches(still)).toEqual({ filter: 0, composer: 0, close: 0 });
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
      ["quick-notes-composer", B.composerTap],
      ["quick-notes-editor-close", B.closeTap],
    ]);
    for (const stop of clicks) {
      const press = stop.press;
      if (!press) continue;
      expect(press.start).toBe(press.end - FILM_TAP_MS);
      for (let elapsed = press.start; elapsed <= press.end; elapsed += 10) expect(welcomeCursorHovered(QUICK_NOTES_CURSOR, elapsed)).toBe(stop.target);
    }
    expect(QUICK_NOTES_CURSOR.find((stop) => stop.target === "quick-notes-tag-hotel")?.depart).toBeGreaterThanOrEqual(B.passportPick);
    expect(QUICK_NOTES_CURSOR.find((stop) => stop.target === "quick-notes-editor-close")?.depart).toBeGreaterThanOrEqual(B.bodyTypeEnd);
  });
});
