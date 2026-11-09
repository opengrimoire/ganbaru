/**
 * Timing for the welcome scene on Notes. On the Novel project's Characters page, a click on an empty line and a typed
 * slash open the slash menu, which scrolls through the blocks it offers; a query narrows it to To-do list, which is
 * picked and filled in. The page then scrolls to its Cast database, where a row is added to the table, named, and given
 * a role from its cell, and the database switches to its Board view, with a column for each role.
 */

import type { NotesColor } from "$lib/notes/types";
import { cursorClick, cursorRest, WELCOME_CURSOR_SCROLL_OVERLAP_MS, type WelcomeCursorScript } from "./welcome-cursor";
import { easeInOutCubic, filmCaretOn, filmSpan, filmTap, filmTyped } from "./welcome-film";
import { PROJECTS_CURSOR_REST_ANCHOR, PROJECTS_HOVER_LEAD_MS } from "./welcome-projects-scene";

/** The roles of the Cast database's Role property, in the order its options list them. */
export type NotesSceneRole = "lead" | "rival" | "ally";
export const NOTES_ROLE_ORDER: readonly NotesSceneRole[] = ["lead", "rival", "ally"];

/** Each role option's color, as a select option keeps one. */
export const NOTES_ROLE_COLORS: Readonly<Record<NotesSceneRole, NotesColor>> = { lead: "blue", rival: "red", ally: "green" };

/** The characters of the Cast database, by the keys of their names. */
export type NotesSceneCharacter = "clara" | "tomas" | "mateo";

/** A row of the Cast database: a character and its role, or null while its Role is empty. */
export interface NotesSceneRow {
  key: NotesSceneCharacter;
  role: NotesSceneRole | null;
}

/** The rows the database starts with. */
export const NOTES_CAST: readonly NotesSceneRow[] = [
  { key: "clara", role: "lead" },
  { key: "tomas", role: "rival" },
];

/** The character the scene adds and the role its cell is given. */
export const NOTES_NEW_CHARACTER: NotesSceneCharacter = "mateo";
export const NOTES_NEW_ROLE: NotesSceneRole = "ally";

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const NOTES_BEATS = {
  /** A click on the empty line below Open threads puts the caret there, and a slash opens the slash menu. */
  lineTap: 900,
  slashTyped: 1400,
  /** The menu scrolls down through its groups of blocks, then back to the top as a query narrows it. */
  menuScrollStart: 1900,
  menuScrollEnd: 3100,
  queryTypeStart: 3500,
  queryTypeEnd: 3900,
  /** To-do list is picked, which turns the line into a to-do, and the to-do is typed. */
  pickTap: 4600,
  todoTypeStart: 4900,
  todoTypeEnd: 6000,
  /** The page scrolls down to the database, keeping room below its table for the row the scene adds. */
  scrollStart: 6500,
  scrollEnd: 7200,
  /** A tap on the table's New page row adds an empty row with its title focused, a name is typed, and Enter keeps it. */
  newRowTap: 7700,
  nameTypeStart: 7900,
  nameTypeEnd: 8400,
  nameSubmit: 8700,
  /** A tap on the new row's Role cell opens its options, and Ally is picked. */
  roleTap: 9400,
  rolePick: 10300,
  /** A tap on the Board tab shows the database as a board with a column for each role. */
  boardTap: 11000,
  end: 12200,
} as const;

const B = NOTES_BEATS;

/** How long the last frame rests before the scene starts over. */
export const NOTES_END_HOLD_MS = 900;

/** How long the board takes to fade in over the table once its tab is tapped. */
export const NOTES_VIEW_FADE_MS = 220;

/** The frame shown as a still with reduced motion: the board with the new card in its role's column. */
export const NOTES_STILL = B.end;

/** Whether the slash line holds the caret: from the click on it until the page scrolls away to the database. */
export function notesLineFocused(elapsed: number): boolean {
  return elapsed >= B.lineTap && elapsed < B.scrollStart;
}

/** Whether the slash menu is open: from the typed slash until a block is picked. */
export function notesSlashMenuOpen(elapsed: number): boolean {
  return elapsed >= B.slashTyped && elapsed < B.pickTap;
}

/** As much of `query` as follows the slash, typed a character at a time. */
export function notesSlashQuery(elapsed: number, query: string): string {
  return filmTyped(elapsed, query, B.queryTypeStart, B.queryTypeEnd);
}

/**
 * How far the slash menu has scrolled toward the groups below its basic blocks, from 0 to 1. A query narrows the list
 * and brings its first match to the top, so the menu is back at the top once one is typed.
 */
export function notesSlashMenuScroll(elapsed: number): number {
  if (elapsed >= B.queryTypeStart) return 0;
  return easeInOutCubic(filmSpan(elapsed, B.menuScrollStart, B.menuScrollEnd));
}

/** Whether the slash line has turned into a to-do. */
export function notesTodoAdded(elapsed: number): boolean {
  return elapsed >= B.pickTap;
}

/**
 * The slash line's text: empty until the slash, then the slash and its query until a block is picked, then as much
 * of `todo` as the new to-do holds.
 */
export function notesLineText(elapsed: number, query: string, todo: string): string {
  if (notesTodoAdded(elapsed)) return filmTyped(elapsed, todo, B.todoTypeStart, B.todoTypeEnd);
  return elapsed >= B.slashTyped ? `/${notesSlashQuery(elapsed, query)}` : "";
}

/** Whether the slash line's caret shows: steady while something is typed into it, and blinking in between. */
export function notesLineCaretOn(elapsed: number): boolean {
  if (!notesLineFocused(elapsed)) return false;
  if (elapsed >= B.pickTap) return filmCaretOn(elapsed, B.todoTypeEnd);
  return filmCaretOn(elapsed, elapsed >= B.queryTypeStart ? B.queryTypeEnd : B.slashTyped);
}

/** How far the page has scrolled down to the database, from 0 to 1. */
export function notesPageScroll(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, B.scrollStart, B.scrollEnd));
}

/** Whether the new row has been added to the table. */
export function notesRowAdded(elapsed: number): boolean {
  return elapsed >= B.newRowTap;
}

/** Whether the new row's title is focused: from the tap that adds the row until Enter keeps its name. */
export function notesNameFocused(elapsed: number): boolean {
  return elapsed >= B.newRowTap && elapsed < B.nameSubmit;
}

/** As much of `name` as the new row's title holds, typed a character at a time. */
export function notesNameDraft(elapsed: number, name: string): string {
  return filmTyped(elapsed, name, B.nameTypeStart, B.nameTypeEnd);
}

/** Whether the new row's caret shows: steady while typing, then blinking until Enter. */
export function notesNameCaretOn(elapsed: number): boolean {
  return notesNameFocused(elapsed) && filmCaretOn(elapsed, B.nameTypeEnd);
}

/** Whether the new row's Role menu is open. */
export function notesRoleMenuOpen(elapsed: number): boolean {
  return elapsed >= B.roleTap && elapsed < B.rolePick;
}

/** Whether the new row's Role cell is highlighted on a phone: from just before its tap and while its menu is open. */
export function notesRoleHighlighted(elapsed: number): boolean {
  return elapsed >= B.roleTap - PROJECTS_HOVER_LEAD_MS && elapsed < B.rolePick;
}

/** Whether the option about to be picked shows as hovered on a phone, just before its tap lands. */
export function notesOptionHovered(elapsed: number): boolean {
  return elapsed >= B.rolePick - PROJECTS_HOVER_LEAD_MS && elapsed < B.rolePick;
}

/** The new row's role: empty as the row is added, then the picked role. */
export function notesNewRole(elapsed: number): NotesSceneRole | null {
  return elapsed >= B.rolePick ? NOTES_NEW_ROLE : null;
}

/** Whether the database shows its Board view rather than its Table view. */
export function notesBoardActive(elapsed: number): boolean {
  return elapsed >= B.boardTap;
}

/** How far the board has faded in over the table, from 0 to 1. */
export function notesBoardShown(elapsed: number): number {
  return filmSpan(elapsed, B.boardTap, B.boardTap + NOTES_VIEW_FADE_MS);
}

/** The scene's touches, each from 0 to 1 as it shows. */
export interface NotesSceneTouches {
  line: number;
  pick: number;
  newRow: number;
  role: number;
  roleOption: number;
  board: number;
}

/** The scene's touches at `elapsed`. */
export function notesTouches(elapsed: number): NotesSceneTouches {
  return {
    line: filmTap(elapsed, B.lineTap),
    pick: filmTap(elapsed, B.pickTap),
    newRow: filmTap(elapsed, B.newRowTap),
    role: filmTap(elapsed, B.roleTap),
    roleOption: filmTap(elapsed, B.rolePick),
    board: filmTap(elapsed, B.boardTap),
  };
}

/**
 * The desktop pointer's path through the scene: it clicks the empty line and waits while the slash menu scrolls and
 * narrows, clicks To-do list once the query leaves it on top, then moves back over the page while the to-do is typed
 * and the page scrolls. It clicks New page once the table is in view, opens the new row's Role and picks Ally, then
 * switches the database to its board.
 */
export const NOTES_CURSOR: WelcomeCursorScript = [
  cursorRest("notes-page", 0, { anchor: PROJECTS_CURSOR_REST_ANCHOR }),
  cursorClick("notes-slash-line", B.lineTap, { glyph: "text", after: 0 }),
  cursorClick("notes-slash-pick", B.pickTap, { after: B.queryTypeEnd }),
  cursorRest("notes-page", B.todoTypeEnd, { anchor: PROJECTS_CURSOR_REST_ANCHOR, busy: [{ start: B.scrollStart, end: B.scrollEnd }] }),
  cursorClick("notes-new-row", B.newRowTap, { after: B.scrollEnd - WELCOME_CURSOR_SCROLL_OVERLAP_MS }),
  cursorClick("notes-role", B.roleTap, { glyph: "pointer", after: B.nameSubmit }),
  cursorClick("notes-role-option", B.rolePick, { after: B.roleTap }),
  cursorClick("notes-board-tab", B.boardTap, { after: B.rolePick }),
];
