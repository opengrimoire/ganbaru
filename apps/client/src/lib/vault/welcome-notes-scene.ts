/**
 * Timing for the welcome scene on Notes. On the Novel project's Characters page, a click on the empty line below the
 * title puts the caret there and a paragraph is typed, and Enter starts a new line. A slash and a short query narrow the
 * slash menu to Callout, which Enter accepts, and the callout is filled in. The page then scrolls to its Cast database,
 * where a row is added to the table, named, and given a role from its cell, and the database switches to its Board view,
 * with a column for each role.
 */

import type { NotesColor } from "$lib/notes/types";
import {
  cursorClick,
  cursorRest,
  WELCOME_CURSOR_SCROLL_OVERLAP_MS,
  WELCOME_CURSOR_TRAVEL_MS,
  type WelcomeCursorScript,
} from "./welcome-cursor";
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
  /** A click on the empty line below the title puts the caret there, a paragraph is typed, and Enter starts a new line. */
  lineTap: 900,
  paragraphTypeStart: 1200,
  paragraphTypeEnd: 3400,
  paragraphSubmit: 3700,
  /** A slash opens the slash menu on the new line, and a query narrows it to Callout. */
  slashTyped: 4100,
  queryTypeStart: 4500,
  queryTypeEnd: 4800,
  /**
   * Callout is accepted, which turns the line into a callout: with Enter, as the menu completes its highlighted item from
   * the keyboard, or with a tap on a phone. The callout's text is then typed.
   */
  pick: 5300,
  calloutTypeStart: 5500,
  calloutTypeEnd: 6600,
  /** The page scrolls down to the database, keeping room below its table for the row the scene adds. */
  scrollStart: 7100,
  scrollEnd: 7800,
  /** A tap on the table's New page row adds an empty row with its title focused, a name is typed, and Enter keeps it. */
  newRowTap: 8300,
  nameTypeStart: 8500,
  nameTypeEnd: 9000,
  nameSubmit: 9300,
  /** A tap on the new row's Role cell opens its options, and Ally is picked. */
  roleTap: 10000,
  rolePick: 10900,
  /** A tap on the Board tab shows the database as a board with a column for each role. */
  boardTap: 11600,
  end: 12800,
} as const;

const B = NOTES_BEATS;

/** How long the last frame rests before the scene starts over. */
export const NOTES_END_HOLD_MS = 900;

/** How long the board takes to fade in over the table once its tab is tapped. */
export const NOTES_VIEW_FADE_MS = 220;

/** The frame shown as a still with reduced motion: the board with the new card in its role's column. */
export const NOTES_STILL = B.end;

/** Whether the paragraph line holds the caret: from the click on it until Enter moves the caret to a new line. */
export function notesParagraphFocused(elapsed: number): boolean {
  return elapsed >= B.lineTap && elapsed < B.paragraphSubmit;
}

/** As much of `paragraph` as the clicked line holds, typed a character at a time. */
export function notesParagraphText(elapsed: number, paragraph: string): string {
  return filmTyped(elapsed, paragraph, B.paragraphTypeStart, B.paragraphTypeEnd);
}

/** Whether the paragraph line's caret shows: blinking once clicked, steady while typing, then blinking until Enter. */
export function notesParagraphCaretOn(elapsed: number): boolean {
  if (!notesParagraphFocused(elapsed)) return false;
  return filmCaretOn(elapsed, elapsed >= B.paragraphTypeStart ? B.paragraphTypeEnd : B.lineTap);
}

/** Whether the new line below the paragraph exists: from the Enter that starts it. */
export function notesSlashLineShown(elapsed: number): boolean {
  return elapsed >= B.paragraphSubmit;
}

/** Whether the slash line holds the caret: from the Enter that starts it until the page scrolls away to the database. */
export function notesLineFocused(elapsed: number): boolean {
  return elapsed >= B.paragraphSubmit && elapsed < B.scrollStart;
}

/** Whether the slash menu is open: from the typed slash until a block is accepted. */
export function notesSlashMenuOpen(elapsed: number): boolean {
  return elapsed >= B.slashTyped && elapsed < B.pick;
}

/** As much of `query` as follows the slash, typed a character at a time. */
export function notesSlashQuery(elapsed: number, query: string): string {
  return filmTyped(elapsed, query, B.queryTypeStart, B.queryTypeEnd);
}

/** Whether the slash line has turned into a callout. */
export function notesCalloutAdded(elapsed: number): boolean {
  return elapsed >= B.pick;
}

/**
 * The slash line's text: empty until the slash, then the slash and its query until a block is accepted, then as much
 * of `callout` as the new callout holds.
 */
export function notesLineText(elapsed: number, query: string, callout: string): string {
  if (notesCalloutAdded(elapsed)) return filmTyped(elapsed, callout, B.calloutTypeStart, B.calloutTypeEnd);
  return elapsed >= B.slashTyped ? `/${notesSlashQuery(elapsed, query)}` : "";
}

/** Whether the slash line's caret shows: steady while something is typed into it, and blinking in between. */
export function notesLineCaretOn(elapsed: number): boolean {
  if (!notesLineFocused(elapsed)) return false;
  if (elapsed >= B.pick) return filmCaretOn(elapsed, B.calloutTypeEnd);
  if (elapsed >= B.queryTypeStart) return filmCaretOn(elapsed, B.queryTypeEnd);
  return filmCaretOn(elapsed, elapsed >= B.slashTyped ? B.slashTyped : B.paragraphSubmit);
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
    pick: filmTap(elapsed, B.pick),
    newRow: filmTap(elapsed, B.newRowTap),
    role: filmTap(elapsed, B.roleTap),
    roleOption: filmTap(elapsed, B.rolePick),
    board: filmTap(elapsed, B.boardTap),
  };
}

/**
 * The desktop pointer's path through the scene: it clicks the empty line, then moves back over the page while the
 * paragraph is typed, the slash menu narrows and Enter accepts Callout, the callout is typed, and the page scrolls. It
 * clicks New page once the table is in view, opens the new row's Role and picks Ally, then switches the database to its
 * board.
 */
export const NOTES_CURSOR: WelcomeCursorScript = [
  cursorRest("notes-page", 0, { anchor: PROJECTS_CURSOR_REST_ANCHOR }),
  cursorClick("notes-line", B.lineTap, { glyph: "text", after: 0 }),
  cursorRest("notes-page", B.paragraphTypeStart + WELCOME_CURSOR_TRAVEL_MS, {
    anchor: PROJECTS_CURSOR_REST_ANCHOR,
    busy: [{ start: B.scrollStart, end: B.scrollEnd }],
  }),
  cursorClick("notes-new-row", B.newRowTap, { after: B.scrollEnd - WELCOME_CURSOR_SCROLL_OVERLAP_MS }),
  cursorClick("notes-role", B.roleTap, { glyph: "pointer", after: B.nameSubmit }),
  cursorClick("notes-role-option", B.rolePick, { after: B.roleTap }),
  cursorClick("notes-board-tab", B.boardTap, { after: B.rolePick }),
];
