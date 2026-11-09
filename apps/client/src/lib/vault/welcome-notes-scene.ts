/**
 * Timing for the welcome scene on Notes. On the Novel project's Characters page, a click on the empty line below its
 * first blocks puts the caret there, and the Markdown shortcut for a level 3 heading turns the line into one as its
 * space is typed. The heading is typed, Enter starts a paragraph, and Enter after it starts a new line, where a slash
 * and a short query narrow the slash menu to Callout, which Enter accepts, and the callout is filled in. The page then
 * scrolls to its Cast database, where a row is added to the table, named, and given a role from its cell, and the
 * database switches to its Board view, with a column for each role, which scrolls sideways to the new row's column.
 * The page then scrolls back up to the plain line at its top, whose text context menu opens, with a right click on
 * desktop or a long press that selects one of its words on a phone, and Quote under Paragraph turns it into a quote.
 */

import type { NotesBlockType, NotesColor } from "$lib/notes/types";
import { SUBMENU_OPEN_DELAY_MS } from "$lib/utils/menu-aim";
import {
  cursorClick,
  cursorRest,
  WELCOME_CURSOR_REST_MS,
  WELCOME_CURSOR_SCROLL_OVERLAP_MS,
  WELCOME_CURSOR_TRAVEL_MS,
  type WelcomeCursorScript,
} from "./welcome-cursor";
import { easeInOutCubic, filmCaretOn, filmPress, filmSpan, filmTap, filmTyped } from "./welcome-film";
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
  shortcutTypeEnd: 1800,
  /** The space after the shortcut turns the line into a heading, whose text is typed, and Enter starts a paragraph. */
  shortcutSpace: 2050,
  headingTypeStart: 2250,
  headingTypeEnd: 2900,
  headingSubmit: 3200,
  /** The paragraph is typed, and Enter starts a new line. */
  paragraphTypeStart: 3500,
  paragraphTypeEnd: 5700,
  paragraphSubmit: 6000,
  /** A slash opens the slash menu on the new line, and a query narrows it to Callout. */
  slashTyped: 6400,
  queryTypeStart: 6800,
  queryTypeEnd: 7400,
  /**
   * Callout is accepted, which turns the line into a callout: with Enter, as the menu completes its highlighted item from
   * the keyboard, or with a tap on a phone. The callout's text is then typed.
   */
  pick: 7900,
  calloutTypeStart: 8100,
  calloutTypeEnd: 9200,
  /** The page scrolls down to the database, keeping room below its table for the row the scene adds. */
  scrollStart: 9700,
  scrollEnd: 10400,
  /** A tap on the table's New page row adds an empty row with its title focused, a name is typed, and Enter keeps it. */
  newRowTap: 10900,
  nameTypeStart: 11100,
  nameTypeEnd: 11600,
  nameSubmit: 11900,
  /** A tap on the new row's Role cell opens its options, and Ally is picked. */
  roleTap: 12600,
  rolePick: 13500,
  /**
   * A tap on the Board tab shows the database as a board with a column for each role, which scrolls sideways to the
   * column of the new row's role, as the board's columns keep their width rather than shrinking to fit.
   */
  boardTap: 14200,
  boardScrollStart: 15200,
  boardScrollEnd: 15900,
  /** The page scrolls back up to the plain line at its top. */
  scrollBackStart: 16500,
  scrollBackEnd: 17200,
  /**
   * The line's text context menu opens: from a right click on desktop, or on a phone from a long press, held from
   * `quotePress` until it lifts at `quoteLift`, that selects the word under it.
   */
  quotePress: 17400,
  quoteMenuOpen: 17900,
  quoteLift: 18100,
  /**
   * Paragraph opens its submenu, once the pointer has rested on it on desktop or as a tap lands on it on a phone, and
   * Quote is picked from it, which turns the line into a quote and closes the menu.
   */
  submenuOpen: 18700,
  quotePick: 19700,
  end: 21000,
} as const;

const B = NOTES_BEATS;

/** How long the last frame rests before the scene starts over. */
export const NOTES_END_HOLD_MS = 900;

/** How long the board takes to fade in over the table once its tab is tapped. */
export const NOTES_VIEW_FADE_MS = 220;

/** The frame shown as a still with reduced motion: the top of the page, with its first line turned into a quote and every block written. */
export const NOTES_STILL = B.end;

/** The plain line at the page's top, split around the word a long press selects on a phone. */
export interface NotesQuoteParts {
  before: string;
  word: string;
  after: string;
}

/** `line` split around the first `word` in it, or the whole line as the word when it does not hold `word`. */
export function notesQuoteParts(line: string, word: string): NotesQuoteParts {
  const index = word ? line.indexOf(word) : -1;
  if (index < 0) return { before: "", word: line, after: "" };
  return { before: line.slice(0, index), word, after: line.slice(index + word.length) };
}

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

/** How far the page has scrolled down to the database, from 0 to 1, until it scrolls back up to its top. */
export function notesPageScroll(elapsed: number): number {
  if (elapsed >= B.scrollBackStart) return 1 - easeInOutCubic(filmSpan(elapsed, B.scrollBackStart, B.scrollBackEnd));
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

/** How far the board has scrolled sideways to the new row's column, from 0 to 1. */
export function notesBoardScroll(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, B.boardScrollStart, B.boardScrollEnd));
}

/** Whether the text context menu is open on the line at the page's top: from the click or press that opens it until Quote is picked. */
export function notesContextMenuOpen(elapsed: number): boolean {
  return elapsed >= B.quoteMenuOpen && elapsed < B.quotePick;
}

/** Whether the context menu's Paragraph submenu is open: from the moment it opens until Quote is picked. */
export function notesContextSubmenuOpen(elapsed: number): boolean {
  return elapsed >= B.submenuOpen && elapsed < B.quotePick;
}

/** When the desktop pointer reaches Paragraph, so the submenu opens after it has rested there as the menu waits to. */
export const NOTES_PARAGRAPH_HOVER = B.submenuOpen - SUBMENU_OPEN_DELAY_MS;

/** Whether the word the long press selects shows as selected on a phone: while the menu it opens is open. */
export function notesQuoteWordSelected(elapsed: number): boolean {
  return notesContextMenuOpen(elapsed);
}

/** Whether the menu's Paragraph row shows as highlighted on a phone: just before its tap lands and while its submenu is open. */
export function notesContextParagraphHighlighted(elapsed: number): boolean {
  return elapsed >= B.submenuOpen - PROJECTS_HOVER_LEAD_MS && elapsed < B.quotePick;
}

/** Whether the submenu's Quote row shows as highlighted on a phone, just before its tap lands. */
export function notesContextQuoteHighlighted(elapsed: number): boolean {
  return elapsed >= B.quotePick - PROJECTS_HOVER_LEAD_MS && elapsed < B.quotePick;
}

/** Whether the line at the page's top has turned into a quote: from the moment Quote is picked. */
export function notesQuoteAdded(elapsed: number): boolean {
  return elapsed >= B.quotePick;
}

/** The type of the line at the page's top: a paragraph until Quote turns it into a quote. */
export function notesQuoteLineType(elapsed: number): NotesBlockType {
  return notesQuoteAdded(elapsed) ? "quote" : "paragraph";
}

/** The scene's touches, each from 0 to 1 as it shows. */
export interface NotesSceneTouches {
  line: number;
  pick: number;
  newRow: number;
  role: number;
  roleOption: number;
  board: number;
  quoteWord: number;
  paragraph: number;
  quote: number;
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
    quoteWord: filmPress(elapsed, B.quotePress, B.quoteLift),
    paragraph: filmTap(elapsed, B.submenuOpen),
    quote: filmTap(elapsed, B.quotePick),
  };
}

/** Where on the board the pointer rests while the board scrolls sideways under it: across its middle, among the cards. */
export const NOTES_BOARD_CURSOR_ANCHOR = [0.5, 0.4] as const;

/**
 * The desktop pointer's path through the scene: it clicks the empty line, then moves back over the page while the
 * heading and the paragraph are typed, the slash menu narrows and Enter accepts Callout, the callout is typed, and the
 * page scrolls. It clicks New page once the table is in view, opens the new row's Role and picks Ally, switches the
 * database to its board, then moves onto the board, which scrolls sideways under it as a mouse wheel scrolls it. Once
 * the page scrolls back up under it, it right clicks the line at the top, rests on Paragraph until its submenu opens,
 * and clicks Quote.
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
  cursorRest("notes-board", B.boardScrollStart - WELCOME_CURSOR_REST_MS, {
    anchor: NOTES_BOARD_CURSOR_ANCHOR,
    busy: [
      { start: B.boardScrollStart, end: B.boardScrollEnd },
      { start: B.scrollBackStart, end: B.scrollBackEnd },
    ],
  }),
  cursorClick("notes-quote-word", B.quoteMenuOpen, { glyph: "text", after: B.scrollBackEnd - WELCOME_CURSOR_SCROLL_OVERLAP_MS }),
  cursorRest("notes-context-paragraph", NOTES_PARAGRAPH_HOVER),
  cursorClick("notes-context-quote", B.quotePick, { after: B.submenuOpen }),
];
