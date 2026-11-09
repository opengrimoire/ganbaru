import { describe, expect, it } from "vitest";
import { notesSlashMenuCatalog, notesSlashMenuGroups } from "$lib/components/notes/blocks/slash-menu-presentation";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { sectionNotesSlashCommandItems } from "$lib/notes/editor/slash-commands";
import { welcomeCursorHovered, welcomeCursorScriptOrdered } from "./welcome-cursor";
import { FILM_TAP_MS } from "./welcome-film";
import {
  NOTES_BEATS,
  NOTES_CAST,
  NOTES_CURSOR,
  NOTES_NEW_CHARACTER,
  NOTES_NEW_ROLE,
  NOTES_ROLE_COLORS,
  NOTES_ROLE_ORDER,
  NOTES_STILL,
  NOTES_VIEW_FADE_MS,
  notesBoardActive,
  notesBoardShown,
  notesLineCaretOn,
  notesLineFocused,
  notesLineText,
  notesNameCaretOn,
  notesNameDraft,
  notesNameFocused,
  notesNewRole,
  notesOptionHovered,
  notesPageScroll,
  notesRoleHighlighted,
  notesRoleMenuOpen,
  notesRowAdded,
  notesSlashMenuOpen,
  notesSlashMenuScroll,
  notesSlashQuery,
  notesTodoAdded,
  notesTouches,
} from "./welcome-notes-scene";

const B = NOTES_BEATS;
const NAME = "Mateo";
const QUERY = "todo";
const TODO = "Decide what Mateo saw";

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(B.end / step) + 1 }, (_, index) => index * step);
}

/** A translator over one language's catalog. */
function translatorFor(catalog: unknown): Translate {
  return ((key, ...args) => translateFromPartialCatalog(catalog, key, ...args)) as Translate;
}

describe("welcome notes scene", () => {
  it("keeps its beats in order", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
  });

  it("keeps every fraction between 0 and 1", () => {
    for (const elapsed of moments()) {
      const values = [notesSlashMenuScroll(elapsed), notesPageScroll(elapsed), notesBoardShown(elapsed), ...Object.values(notesTouches(elapsed))];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("gives every role its own color and starts with rows in other roles than the new one", () => {
    for (const role of NOTES_ROLE_ORDER) expect(NOTES_ROLE_COLORS[role]).toBeTruthy();
    expect(new Set(NOTES_ROLE_ORDER.map((role) => NOTES_ROLE_COLORS[role])).size).toBe(NOTES_ROLE_ORDER.length);
    expect(NOTES_CAST.map((row) => row.role)).not.toContain(NOTES_NEW_ROLE);
    expect(NOTES_CAST.map((row) => row.key)).not.toContain(NOTES_NEW_CHARACTER);
  });

  it("opens the slash menu with the slash typed into the clicked line, and closes it as a block is picked", () => {
    expect(notesLineFocused(B.lineTap - 1)).toBe(false);
    expect(notesLineFocused(B.lineTap)).toBe(true);
    expect(notesLineText(B.slashTyped - 1, QUERY, TODO)).toBe("");
    expect(notesSlashMenuOpen(B.slashTyped - 1)).toBe(false);
    expect(notesLineText(B.slashTyped, QUERY, TODO)).toBe("/");
    expect(notesSlashMenuOpen(B.slashTyped)).toBe(true);
    expect(notesLineText(B.queryTypeEnd, QUERY, TODO)).toBe(`/${QUERY}`);
    expect(notesSlashMenuOpen(B.pickTap - 1)).toBe(true);
    expect(notesSlashMenuOpen(B.pickTap)).toBe(false);
  });

  it("scrolls the menu down through its groups, then back to the top once the query narrows it", () => {
    expect(notesSlashMenuScroll(B.menuScrollStart)).toBe(0);
    expect(notesSlashMenuScroll(B.menuScrollEnd)).toBe(1);
    expect(notesSlashMenuScroll(B.queryTypeStart - 1)).toBe(1);
    expect(notesSlashQuery(B.queryTypeStart - 1, QUERY)).toBe("");
    expect(notesSlashMenuScroll(B.queryTypeStart)).toBe(0);
  });

  it("turns the line into a to-do as the pick lands, clears the slash, and types the to-do", () => {
    expect(notesTodoAdded(B.pickTap - 1)).toBe(false);
    expect(notesTodoAdded(B.pickTap)).toBe(true);
    expect(notesLineText(B.pickTap, QUERY, TODO)).toBe("");
    expect(notesLineText((B.todoTypeStart + B.todoTypeEnd) / 2, QUERY, TODO).length).toBeGreaterThan(0);
    expect(notesLineText(B.todoTypeEnd, QUERY, TODO)).toBe(TODO);
    expect(notesLineText(B.end, QUERY, TODO)).toBe(TODO);
    expect(notesLineCaretOn((B.todoTypeStart + B.todoTypeEnd) / 2)).toBe(true);
    expect(notesLineCaretOn(B.scrollStart)).toBe(false);
  });

  it("narrows the slash menu to To-do list first with each language's query", () => {
    for (const catalog of [en, es]) {
      const t = translatorFor(catalog);
      const query = t("vaultSetup.notes.slashQuery");
      const sections = sectionNotesSlashCommandItems(notesSlashMenuCatalog(true, t), query, []);
      const first = notesSlashMenuGroups(sections, t).flatMap((group) => group.items)[0];
      expect(first?.command).toEqual({ kind: "block", blockType: "to_do" });
    }
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
  });

  it("opens the role menu with its tap, highlights the cell and the option before their taps, and picks the role", () => {
    expect(notesRoleMenuOpen(B.roleTap - 1)).toBe(false);
    expect(notesRoleHighlighted(B.roleTap - 1)).toBe(true);
    expect(notesRoleMenuOpen(B.roleTap)).toBe(true);
    expect(notesOptionHovered(B.rolePick - 1)).toBe(true);
    expect(notesOptionHovered(B.roleTap)).toBe(false);
    expect(notesNewRole(B.rolePick - 1)).toBeNull();
    expect(notesRoleMenuOpen(B.rolePick)).toBe(false);
    expect(notesNewRole(B.rolePick)).toBe(NOTES_NEW_ROLE);
  });

  it("never has two carets, or a caret and an open menu that does not belong to it, at once", () => {
    for (const elapsed of moments()) {
      expect(notesLineFocused(elapsed) && notesNameFocused(elapsed)).toBe(false);
      expect(notesNameFocused(elapsed) && notesRoleMenuOpen(elapsed)).toBe(false);
      expect(notesSlashMenuOpen(elapsed) && notesRoleMenuOpen(elapsed)).toBe(false);
    }
  });

  it("shows the board from its tab's tap, fading it in over the table", () => {
    expect(notesBoardActive(B.boardTap - 1)).toBe(false);
    expect(notesBoardShown(B.boardTap - 1)).toBe(0);
    expect(notesBoardActive(B.boardTap)).toBe(true);
    expect(notesBoardShown(B.boardTap + NOTES_VIEW_FADE_MS / 2)).toBeGreaterThan(0);
    expect(notesBoardShown(B.boardTap + NOTES_VIEW_FADE_MS)).toBe(1);
  });

  it("shows the board with the new row's card in its role, the to-do written, and every menu closed in its still frame", () => {
    expect(notesBoardShown(NOTES_STILL)).toBe(1);
    expect(notesNewRole(NOTES_STILL)).toBe(NOTES_NEW_ROLE);
    expect(notesLineText(NOTES_STILL, QUERY, TODO)).toBe(TODO);
    expect(notesSlashMenuOpen(NOTES_STILL)).toBe(false);
    expect(notesRoleMenuOpen(NOTES_STILL)).toBe(false);
    expect(notesNameFocused(NOTES_STILL)).toBe(false);
    expect(Object.values(notesTouches(NOTES_STILL)).every((touch) => touch === 0)).toBe(true);
  });

  it("clicks with the desktop pointer on the beats the touches land on, resting on each control through its press", () => {
    expect(welcomeCursorScriptOrdered(NOTES_CURSOR)).toBe(true);
    const clicks = NOTES_CURSOR.filter((stop) => stop.press);
    expect(clicks.map((stop) => [stop.target, stop.press?.end])).toEqual([
      ["notes-slash-line", B.lineTap],
      ["notes-slash-pick", B.pickTap],
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

  it("waits for the query before reaching for To-do list, and for the scroll before reaching for the new row", () => {
    expect(NOTES_CURSOR.find((stop) => stop.target === "notes-slash-pick")?.depart).toBeGreaterThanOrEqual(B.queryTypeEnd);
    expect(NOTES_CURSOR.find((stop) => stop.target === "notes-new-row")?.depart).toBeGreaterThan(B.scrollStart);
    expect(NOTES_CURSOR.find((stop) => stop.target === "notes-role")?.depart).toBeGreaterThanOrEqual(B.nameSubmit);
  });
});
