/**
 * Timing for the welcome scene on Notes. On the Novel project's Characters page, a click on the empty line below its
 * first blocks puts the caret there, and the Markdown shortcut for a level 3 heading turns the line into one as its
 * space is typed. The heading is typed, Enter starts a paragraph, and Enter after it starts a new line, where a slash
 * and a short query narrow the slash menu to Callout, which Enter accepts, and the callout is filled in. The page then
 * scrolls to its Cast database, where a row is added to the table, named, and given a role from its cell, and the
 * database switches to its Board view, with a column for each role.
 */

import type { NotesBlockType, NotesColor } from "$lib/notes/types";
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

/** The Markdown shortcut typed into the empty line, and the heading it turns the line into once its space is typed. */
export const NOTES_HEADING_SHORTCUT = "###";
export const NOTES_HEADING_TYPE: NotesBlockType = "heading_3";

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const NOTES_BEATS = {
  /** A click on the empty line puts the caret there, and the heading shortcut is typed. */
  lineTap: 900,
  shortcutTypeStart: 1200,
  shortcutTypeEnd: 1500,
  /** The space after the shortcut turns the line into a heading, whose text is typed, and Enter starts a paragraph. */
  shortcutSpace: 1750,
  headingTypeStart: 1950,
  headingTypeEnd: 2600,
  headingSubmit: 2900,
  /** The paragraph is typed, and Enter starts a new line. */
  paragraphTypeStart: 3200,
  paragraphTypeEnd: 5400,
  paragraphSubmit: 5700,
  /** A slash opens the slash menu on the new line, and a query narrows it to Callout. */
  slashTyped: 6100,
  queryTypeStart: 6500,
  queryTypeEnd: 6800,
  /**
   * Callout is accepted, which turns the line into a callout: with Enter, as the menu completes its highlighted item from
   * the keyboard, or with a tap on a phone. The callout's text is then typed.
   */
  pick: 7300,
  calloutTypeStart: 7500,
  calloutTypeEnd: 8600,
  /** The page scrolls down to the database, keeping room below its table for the row the scene adds. */
  scrollStart: 9100,
  scrollEnd: 9800,
  /** A tap on the table's New page row adds an empty row with its title focused, a name is typed, and Enter keeps it. */
  newRowTap: 10300,
  nameTypeStart: 10500,
  nameTypeEnd: 11000,
  nameSubmit: 11300,
  /** A tap on the new row's Role cell opens its options, and Ally is picked. */
  roleTap: 12000,
  rolePick: 12900,
  /** A tap on the Board tab shows the database as a board with a column for each role. */
  boardTap: 13600,
  end: 14800,
} as const;

const B = NOTES_BEATS;

/** How long the last frame rests before the scene starts over. */
export const NOTES_END_HOLD_MS = 900;

/** How long the board takes to fade in over the table once its tab is tapped. */
export const NOTES_VIEW_FADE_MS = 220;

/** The frame shown as a still with reduced motion: the board with the new card in its role's column. */
export const NOTES_STILL = B.end;

/** Whether the clicked line holds the caret: from the click on it until Enter starts the paragraph below it. */
export function notesHeadingFocused(elapsed: number): boolean {
  return elapsed >= B.lineTap && elapsed < B.headingSubmit;
}

/** Whether the clicked line has turned into a heading: from the space typed after the shortcut. */
export function notesHeadingAdded(elapsed: number): boolean {
  return elapsed >= B.shortcutSpace;
}

/** The clicked line's text: the shortcut as it is typed, then, once the line is a heading, as much of `heading` as it holds. */
export function notesHeadingText(elapsed: number, heading: string): string {
  if (notesHeadingAdded(elapsed)) return filmTyped(elapsed, heading, B.headingTypeStart, B.headingTypeEnd);
  return filmTyped(elapsed, NOTES_HEADING_SHORTCUT, B.shortcutTypeStart, B.shortcutTypeEnd);
}

/** Whether the clicked line's caret shows: steady while something is typed into it, and blinking in between. */
export function notesHeadingCaretOn(elapsed: number): boolean {
  if (!notesHeadingFocused(elapsed)) return false;
  if (elapsed >= B.headingTypeStart) return filmCaretOn(elapsed, B.headingTypeEnd);
  return filmCaretOn(elapsed, elapsed >= B.shortcutTypeStart ? B.shortcutSpace : B.lineTap);
}

/** Whether the paragraph below the heading exists: from the Enter that starts it. */
export function notesParagraphShown(elapsed: number): boolean {
  return elapsed >= B.headingSubmit;
}

/** Whether the paragraph holds the caret: from the Enter that starts it until Enter moves the caret to a new line. */
export function notesParagraphFocused(elapsed: number): boolean {
  return elapsed >= B.headingSubmit && elapsed < B.paragraphSubmit;
}

/** As much of `paragraph` as the paragraph holds, typed a character at a time. */
export function notesParagraphText(elapsed: number, paragraph: string): string {
  return filmTyped(elapsed, paragraph, B.paragraphTypeStart, B.paragraphTypeEnd);
}

/** Whether the paragraph's caret shows: blinking once it starts, steady while typing, then blinking until Enter. */
export function notesParagraphCaretOn(elapsed: number): boolean {
  if (!notesParagraphFocused(elapsed)) return false;
  return filmCaretOn(elapsed, elapsed >= B.paragraphTypeStart ? B.paragraphTypeEnd : B.headingSubmit);
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
 * heading and the paragraph are typed, the slash menu narrows and Enter accepts Callout, the callout is typed, and the
 * page scrolls. It clicks New page once the table is in view, opens the new row's Role and picks Ally, then switches the
 * database to its board.
 */
export const NOTES_CURSOR: WelcomeCursorScript = [
  cursorRest("notes-page", 0, { anchor: PROJECTS_CURSOR_REST_ANCHOR }),
  cursorClick("notes-line", B.lineTap, { glyph: "text", after: 0 }),
  cursorRest("notes-page", B.shortcutTypeStart + WELCOME_CURSOR_TRAVEL_MS, {
    anchor: PROJECTS_CURSOR_REST_ANCHOR,
    busy: [{ start: B.scrollStart, end: B.scrollEnd }],
  }),
  cursorClick("notes-new-row", B.newRowTap, { after: B.scrollEnd - WELCOME_CURSOR_SCROLL_OVERLAP_MS }),
  cursorClick("notes-role", B.roleTap, { glyph: "pointer", after: B.nameSubmit }),
  cursorClick("notes-role-option", B.rolePick, { after: B.roleTap }),
  cursorClick("notes-board-tab", B.boardTap, { after: B.rolePick }),
];
