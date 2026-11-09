/**
 * Timing for the welcome scene on Notes. On the Novel project's Characters page, the page scrolls to the Cast
 * database, a row is added to its table and named, its role is picked from its cell, and the database switches to its
 * Board view, where the new card is dragged from one role's column to another's. A phone has no card drag, so its scene
 * ends once the board shows.
 */

import type { NotesColor } from "$lib/notes/types";
import { cursorClick, cursorRest, WELCOME_CURSOR_REST_MS, WELCOME_CURSOR_SCROLL_OVERLAP_MS, type WelcomeCursorScript } from "./welcome-cursor";
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

/** The character the scene adds, the role its cell is given, and the role whose column its card is dragged to. */
export const NOTES_NEW_CHARACTER: NotesSceneCharacter = "mateo";
export const NOTES_NEW_ROLE: NotesSceneRole = "ally";
export const NOTES_DROP_ROLE: NotesSceneRole = "rival";

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const NOTES_BEATS = {
  /** The page scrolls down to the database, keeping room below its table for the row the scene adds. */
  scrollStart: 400,
  scrollEnd: 1000,
  /** A tap on the table's New page row adds an empty row with its title focused, a name is typed, and Enter keeps it. */
  newRowTap: 1500,
  nameTypeStart: 1700,
  nameTypeEnd: 2300,
  nameSubmit: 2700,
  /** A tap on the new row's Role cell opens its options, and Ally is picked. */
  roleTap: 3500,
  rolePick: 4700,
  /** A tap on the Board tab shows the database as a board with a column for each role. */
  boardTap: 5600,
  /** The new card is held by its handle, dragged to the Rival column, and dropped below the card there. */
  dragPress: 6700,
  dragMoveStart: 6900,
  dragMoveEnd: 7900,
  drop: 8100,
  end: 9300,
} as const;

const B = NOTES_BEATS;

/** How long the last frame rests before the scene starts over. */
export const NOTES_END_HOLD_MS = 900;

/** How long the board takes to fade in over the table once its tab is tapped. */
export const NOTES_VIEW_FADE_MS = 220;

/** How long a held card takes to lift off the board before it moves. */
export const NOTES_DRAG_LIFT_MS = 160;

/** Where a phone's scene ends: once the board has shown for a while, before the card drag a phone does not show. */
export const NOTES_MOBILE_END = B.boardTap + 1000;

/** How long a run of the scene lasts on a phone or with a pointer. */
export function notesSceneEnd(mobile: boolean): number {
  return mobile ? NOTES_MOBILE_END : B.end;
}

/**
 * The frame shown as a still with reduced motion: the board with the new card in its role's column, after the drag on
 * desktop and as the role cell left it on a phone.
 */
export function notesSceneStill(mobile: boolean): number {
  return notesSceneEnd(mobile);
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

/** The new row's role: empty as the row is added, then the picked role, then the column its card is dropped in. */
export function notesNewRole(elapsed: number): NotesSceneRole | null {
  if (elapsed >= B.drop) return NOTES_DROP_ROLE;
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

/** The new card's drag at one moment. */
export interface NotesSceneDrag {
  /** How far the card has lifted off the board, from 0 to 1. */
  lift: number;
  /** How far the card has travelled from its column to its drop place, eased, from 0 to 1. */
  progress: number;
  /** Whether the card is over the column it is dropped in, which shows where it lands. */
  over: boolean;
}

/** The new card's drag at `elapsed`, or null while it is not held. */
export function notesDrag(elapsed: number): NotesSceneDrag | null {
  if (elapsed < B.dragPress || elapsed >= B.drop) return null;
  const progress = easeInOutCubic(filmSpan(elapsed, B.dragMoveStart, B.dragMoveEnd));
  return { lift: filmSpan(elapsed, B.dragPress, B.dragPress + NOTES_DRAG_LIFT_MS), progress, over: progress >= 0.5 };
}

/** The scene's touches, each from 0 to 1 as it shows. */
export interface NotesSceneTouches {
  newRow: number;
  role: number;
  roleOption: number;
  board: number;
}

/** The scene's touches at `elapsed`. */
export function notesTouches(elapsed: number): NotesSceneTouches {
  return {
    newRow: filmTap(elapsed, B.newRowTap),
    role: filmTap(elapsed, B.roleTap),
    roleOption: filmTap(elapsed, B.rolePick),
    board: filmTap(elapsed, B.boardTap),
  };
}

/**
 * The desktop pointer's path through the scene: it rests on the page while it scrolls, clicks New page once the table
 * is in view and stays while the name is typed, opens the new row's Role and picks Ally, switches to the board, then
 * takes the new card by its handle and drags it to the Rival column, following the card it carries.
 */
export const NOTES_CURSOR: WelcomeCursorScript = [
  cursorRest("notes-page", 0, { anchor: PROJECTS_CURSOR_REST_ANCHOR, busy: [{ start: B.scrollStart, end: B.scrollEnd }] }),
  cursorClick("notes-new-row", B.newRowTap, { after: B.scrollEnd - WELCOME_CURSOR_SCROLL_OVERLAP_MS }),
  cursorClick("notes-role", B.roleTap, { glyph: "pointer", after: B.nameSubmit }),
  cursorClick("notes-role-option", B.rolePick),
  cursorClick("notes-board-tab", B.boardTap, { after: B.rolePick }),
  cursorRest("notes-card-grip", B.dragPress - WELCOME_CURSOR_REST_MS),
  {
    target: "notes-drag-grip",
    depart: B.dragMoveStart,
    arrive: B.dragMoveEnd,
    press: { start: B.dragPress, end: B.drop },
    follow: true,
  },
];
