import { describe, expect, it } from "vitest";
import { welcomeCursorFrame, welcomeCursorHovered, welcomeCursorScriptOrdered } from "./welcome-cursor";
import { FILM_TAP_MS } from "./welcome-film";
import {
  NOTES_BEATS,
  NOTES_CAST,
  NOTES_CURSOR,
  NOTES_DROP_ROLE,
  NOTES_MOBILE_END,
  NOTES_NEW_CHARACTER,
  NOTES_NEW_ROLE,
  NOTES_ROLE_COLORS,
  NOTES_ROLE_ORDER,
  NOTES_VIEW_FADE_MS,
  notesBoardActive,
  notesBoardShown,
  notesDrag,
  notesNameCaretOn,
  notesNameDraft,
  notesNameFocused,
  notesNewRole,
  notesOptionHovered,
  notesPageScroll,
  notesRoleHighlighted,
  notesRoleMenuOpen,
  notesRowAdded,
  notesSceneEnd,
  notesSceneStill,
  notesTouches,
} from "./welcome-notes-scene";

const B = NOTES_BEATS;
const NAME = "Mateo";

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(B.end / step) + 1 }, (_, index) => index * step);
}

describe("welcome notes scene", () => {
  it("keeps its beats in order", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
  });

  it("keeps every fraction between 0 and 1", () => {
    for (const elapsed of moments()) {
      const drag = notesDrag(elapsed);
      const values = [notesPageScroll(elapsed), notesBoardShown(elapsed), ...Object.values(notesTouches(elapsed))];
      if (drag) values.push(drag.lift, drag.progress);
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("gives every role a color, starts with a row in other roles than the new one, and drops it on a role a row already has", () => {
    for (const role of NOTES_ROLE_ORDER) expect(NOTES_ROLE_COLORS[role]).toBeTruthy();
    expect(new Set(NOTES_ROLE_ORDER.map((role) => NOTES_ROLE_COLORS[role])).size).toBe(NOTES_ROLE_ORDER.length);
    expect(NOTES_CAST.map((row) => row.role)).not.toContain(NOTES_NEW_ROLE);
    expect(NOTES_CAST.map((row) => row.role)).toContain(NOTES_DROP_ROLE);
    expect(NOTES_DROP_ROLE).not.toBe(NOTES_NEW_ROLE);
    expect(NOTES_CAST.map((row) => row.key)).not.toContain(NOTES_NEW_CHARACTER);
  });

  it("scrolls the page to the database before the new row is added", () => {
    expect(notesPageScroll(B.scrollStart)).toBe(0);
    expect(notesPageScroll(B.scrollEnd)).toBe(1);
    expect(notesPageScroll(B.newRowTap)).toBe(1);
    expect(notesPageScroll(B.end)).toBe(1);
  });

  it("adds an empty row with its title focused, types its name, and keeps it with Enter", () => {
    expect(notesRowAdded(B.newRowTap - 1)).toBe(false);
    expect(notesRowAdded(B.newRowTap)).toBe(true);
    expect(notesNameFocused(B.newRowTap - 1)).toBe(false);
    expect(notesNameFocused(B.newRowTap)).toBe(true);
    expect(notesNameDraft(B.nameTypeStart, NAME)).toBe("");
    expect(notesNameDraft((B.nameTypeStart + B.nameTypeEnd) / 2, NAME).length).toBeGreaterThan(0);
    expect(notesNameDraft(B.nameTypeEnd, NAME)).toBe(NAME);
    expect(notesNameCaretOn((B.nameTypeStart + B.nameTypeEnd) / 2)).toBe(true);
    expect(notesNameFocused(B.nameSubmit - 1)).toBe(true);
    expect(notesNameFocused(B.nameSubmit)).toBe(false);
    expect(notesNameCaretOn(B.nameSubmit)).toBe(false);
    expect(notesNameDraft(B.end, NAME)).toBe(NAME);
  });

  it("opens the role menu with its tap, highlights the cell and the option before their taps, and picks the role", () => {
    expect(notesRoleMenuOpen(B.roleTap - 1)).toBe(false);
    expect(notesRoleHighlighted(B.roleTap - 1)).toBe(true);
    expect(notesRoleMenuOpen(B.roleTap)).toBe(true);
    expect(notesOptionHovered(B.rolePick - 1)).toBe(true);
    expect(notesOptionHovered(B.roleTap)).toBe(false);
    expect(notesNewRole(B.rolePick - 1)).toBeNull();
    expect(notesRoleMenuOpen(B.rolePick)).toBe(false);
    expect(notesRoleHighlighted(B.rolePick)).toBe(false);
    expect(notesOptionHovered(B.rolePick)).toBe(false);
    expect(notesNewRole(B.rolePick)).toBe(NOTES_NEW_ROLE);
  });

  it("keeps the name field and the role menu from being open at once", () => {
    for (const elapsed of moments()) expect(notesNameFocused(elapsed) && notesRoleMenuOpen(elapsed)).toBe(false);
  });

  it("shows the board from its tab's tap, fading it in over the table", () => {
    expect(notesBoardActive(B.boardTap - 1)).toBe(false);
    expect(notesBoardShown(B.boardTap - 1)).toBe(0);
    expect(notesBoardActive(B.boardTap)).toBe(true);
    expect(notesBoardShown(B.boardTap + NOTES_VIEW_FADE_MS / 2)).toBeGreaterThan(0);
    expect(notesBoardShown(B.boardTap + NOTES_VIEW_FADE_MS)).toBe(1);
    expect(B.boardTap).toBeGreaterThan(B.rolePick);
  });

  it("holds the card from its press, lifts it before it moves, shows where it lands past halfway, and drops it in the other role", () => {
    expect(notesDrag(B.dragPress - 1)).toBeNull();
    expect(notesDrag(B.dragPress)).toEqual({ lift: 0, progress: 0, over: false });
    expect(notesDrag(B.dragMoveStart)?.lift).toBe(1);
    expect(notesDrag(B.dragMoveStart)?.progress).toBe(0);
    expect(notesDrag(B.dragMoveEnd)).toEqual({ lift: 1, progress: 1, over: true });
    expect(notesDrag(B.drop - 1)?.over).toBe(true);
    expect(notesDrag(B.drop)).toBeNull();
    expect(notesNewRole(B.drop - 1)).toBe(NOTES_NEW_ROLE);
    expect(notesNewRole(B.drop)).toBe(NOTES_DROP_ROLE);
    let lastProgress = 0;
    for (const elapsed of moments()) {
      const progress = notesDrag(elapsed)?.progress;
      if (progress === undefined) continue;
      expect(progress).toBeGreaterThanOrEqual(lastProgress);
      lastProgress = progress;
    }
  });

  it("ends a phone's scene on the board before the drag, which a phone does not show", () => {
    expect(notesSceneEnd(true)).toBe(NOTES_MOBILE_END);
    expect(notesSceneEnd(false)).toBe(B.end);
    expect(NOTES_MOBILE_END).toBeGreaterThan(B.boardTap + NOTES_VIEW_FADE_MS);
    expect(NOTES_MOBILE_END).toBeLessThanOrEqual(B.dragPress);
  });

  it("shows the board with the new row's card in its final role, every menu closed, in its still frames", () => {
    for (const [mobile, role] of [
      [true, NOTES_NEW_ROLE],
      [false, NOTES_DROP_ROLE],
    ] as const) {
      const still = notesSceneStill(mobile);
      expect(notesBoardShown(still)).toBe(1);
      expect(notesNewRole(still)).toBe(role);
      expect(notesRoleMenuOpen(still)).toBe(false);
      expect(notesNameFocused(still)).toBe(false);
      expect(notesDrag(still)).toBeNull();
      expect(Object.values(notesTouches(still)).every((touch) => touch === 0)).toBe(true);
    }
  });

  it("clicks with the desktop pointer on the beats the touches land on, resting on each control through its press", () => {
    expect(welcomeCursorScriptOrdered(NOTES_CURSOR)).toBe(true);
    const clicks = NOTES_CURSOR.filter((stop) => stop.press && !stop.follow);
    expect(clicks.map((stop) => [stop.target, stop.press?.end])).toEqual([
      ["notes-new-row", B.newRowTap],
      ["notes-role", B.roleTap],
      ["notes-role-option", B.rolePick],
      ["notes-board-tab", B.boardTap],
    ]);
    for (const stop of clicks) {
      const press = stop.press;
      if (!press) continue;
      expect(press.start).toBe(press.end - FILM_TAP_MS);
      for (let elapsed = press.start; elapsed <= press.end; elapsed += 10) expect(welcomeCursorHovered(NOTES_CURSOR, elapsed)).toBe(stop.target);
    }
  });

  it("reaches the new row once the page has nearly scrolled, and drags the card from its handle while holding the button", () => {
    expect(NOTES_CURSOR.find((stop) => stop.target === "notes-new-row")?.depart).toBeGreaterThan(B.scrollStart);
    expect(NOTES_CURSOR.find((stop) => stop.target === "notes-role")?.depart).toBeGreaterThanOrEqual(B.nameSubmit);
    const drag = NOTES_CURSOR.find((stop) => stop.follow);
    expect(drag?.target).toBe("notes-drag-grip");
    expect(drag?.press).toEqual({ start: B.dragPress, end: B.drop });
    expect(drag?.depart).toBe(B.dragMoveStart);
    expect(drag?.arrive).toBe(B.dragMoveEnd);
    expect(welcomeCursorHovered(NOTES_CURSOR, B.dragPress - 1)).toBe("notes-card-grip");
    expect(welcomeCursorFrame(NOTES_CURSOR, B.dragPress + FILM_TAP_MS).pressed).toBe(1);
    expect(welcomeCursorFrame(NOTES_CURSOR, B.dragMoveEnd).away).toBe(false);
  });
});
