import { describe, expect, it } from "vitest";
import { notesSlashMenuCatalog, notesSlashMenuGroups } from "$lib/components/notes/blocks/slash-menu-presentation";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { blockTypeForTextShortcut } from "$lib/notes/blocks/shortcuts";
import { sectionNotesSlashCommandItems } from "$lib/notes/editor/slash-commands";
import { SUBMENU_OPEN_DELAY_MS } from "$lib/utils/menu-aim";
import { welcomeCursorFrame, welcomeCursorHovered, welcomeCursorScriptOrdered } from "./welcome-cursor";
import { FILM_TAP_MS } from "./welcome-film";
import { PROJECTS_HOVER_LEAD_MS } from "./welcome-projects-scene";
import {
  NOTES_BEATS,
  NOTES_CAST,
  NOTES_CURSOR,
  NOTES_HEADING_SHORTCUT,
  NOTES_HEADING_TYPE,
  NOTES_NEW_CHARACTER,
  NOTES_NEW_ROLE,
  NOTES_NAVIGATOR_FOLDER,
  NOTES_NAVIGATOR_FOLDER_ITEMS,
  NOTES_NAVIGATOR_ROOT,
  NOTES_PARAGRAPH_HOVER,
  NOTES_ROLE_COLORS,
  NOTES_ROLE_ORDER,
  NOTES_STILL,
  NOTES_VIEW_FADE_MS,
  notesBoardActive,
  notesBoardScroll,
  notesBoardShown,
  notesHeadingAdded,
  notesHeadingCaretOn,
  notesHeadingFocused,
  notesHeadingText,
  notesLineCaretOn,
  notesLineFocused,
  notesLineText,
  notesNameCaretOn,
  notesNameDraft,
  notesNameFocused,
  notesNavigatorFolderHighlighted,
  notesNavigatorFolderOpen,
  notesNavigatorOpen,
  notesNewRole,
  notesOptionHovered,
  notesParagraphCaretOn,
  notesParagraphFocused,
  notesParagraphShown,
  notesParagraphText,
  notesPageScroll,
  notesRoleHighlighted,
  notesRoleMenuOpen,
  notesRowAdded,
  notesSlashMenuOpen,
  notesSlashLineShown,
  notesSlashQuery,
  notesCalloutAdded,
  notesContextMenuOpen,
  notesContextParagraphHighlighted,
  notesContextQuoteHighlighted,
  notesContextSubmenuOpen,
  notesQuoteAdded,
  notesQuoteLineType,
  notesQuoteParts,
  notesQuoteWordSelected,
  notesSubmenuScroll,
  notesTouches,
} from "./welcome-notes-scene";

const B = NOTES_BEATS;
const NAME = "Mateo";
const QUERY = "call";
const CALLOUT = "Decide what Mateo saw at the station.";
const HEADING = "Mateo's secret";
const PARAGRAPH = "Clara and Tomás grew up on the same street, and only Mateo knows why they stopped talking.";

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
      const values = [notesPageScroll(elapsed), notesBoardShown(elapsed), notesBoardScroll(elapsed), ...Object.values(notesTouches(elapsed))];
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

  it("types the editor's own Markdown shortcut for the heading it turns the line into", () => {
    expect(blockTypeForTextShortcut(NOTES_HEADING_SHORTCUT)).toBe(NOTES_HEADING_TYPE);
  });

  it("types the shortcut into the clicked line, turns it into a heading with the space, types the heading, and starts the paragraph with Enter", () => {
    expect(notesHeadingFocused(B.lineTap - 1)).toBe(false);
    expect(notesHeadingFocused(B.lineTap)).toBe(true);
    expect(notesHeadingCaretOn(B.lineTap)).toBe(true);
    expect(notesHeadingText(B.shortcutTypeStart, HEADING)).toBe("");
    expect(notesHeadingText(B.shortcutTypeEnd, HEADING)).toBe(NOTES_HEADING_SHORTCUT);
    expect(notesHeadingAdded(B.shortcutSpace - 1)).toBe(false);
    expect(notesHeadingText(B.shortcutSpace - 1, HEADING)).toBe(NOTES_HEADING_SHORTCUT);
    expect(notesHeadingAdded(B.shortcutSpace)).toBe(true);
    expect(notesHeadingText(B.shortcutSpace, HEADING)).toBe("");
    expect(notesHeadingText((B.headingTypeStart + B.headingTypeEnd) / 2, HEADING).length).toBeGreaterThan(0);
    expect(notesHeadingCaretOn((B.headingTypeStart + B.headingTypeEnd) / 2)).toBe(true);
    expect(notesHeadingText(B.headingTypeEnd, HEADING)).toBe(HEADING);
    expect(notesParagraphShown(B.headingSubmit - 1)).toBe(false);
    expect(notesParagraphFocused(B.headingSubmit - 1)).toBe(false);
    expect(notesHeadingFocused(B.headingSubmit)).toBe(false);
    expect(notesHeadingCaretOn(B.headingSubmit)).toBe(false);
    expect(notesParagraphShown(B.headingSubmit)).toBe(true);
    expect(notesParagraphFocused(B.headingSubmit)).toBe(true);
    expect(notesHeadingText(B.end, HEADING)).toBe(HEADING);
  });

  it("types the paragraph below the heading, then starts the slash line with Enter", () => {
    expect(notesParagraphText(B.paragraphTypeStart, PARAGRAPH)).toBe("");
    expect(notesParagraphText((B.paragraphTypeStart + B.paragraphTypeEnd) / 2, PARAGRAPH).length).toBeGreaterThan(0);
    expect(notesParagraphCaretOn((B.paragraphTypeStart + B.paragraphTypeEnd) / 2)).toBe(true);
    expect(notesParagraphText(B.paragraphTypeEnd, PARAGRAPH)).toBe(PARAGRAPH);
    expect(notesSlashLineShown(B.paragraphSubmit - 1)).toBe(false);
    expect(notesLineFocused(B.paragraphSubmit - 1)).toBe(false);
    expect(notesSlashLineShown(B.paragraphSubmit)).toBe(true);
    expect(notesParagraphFocused(B.paragraphSubmit)).toBe(false);
    expect(notesParagraphCaretOn(B.paragraphSubmit)).toBe(false);
    expect(notesLineFocused(B.paragraphSubmit)).toBe(true);
    expect(notesParagraphText(B.end, PARAGRAPH)).toBe(PARAGRAPH);
  });

  it("opens the slash menu with the slash, narrows it with the query typed straight after, and closes it as Enter accepts", () => {
    expect(notesLineText(B.slashTyped - 1, QUERY, CALLOUT)).toBe("");
    expect(notesSlashMenuOpen(B.slashTyped - 1)).toBe(false);
    expect(notesLineText(B.slashTyped, QUERY, CALLOUT)).toBe("/");
    expect(notesSlashMenuOpen(B.slashTyped)).toBe(true);
    expect(notesSlashQuery(B.queryTypeStart, QUERY)).toBe("");
    expect(notesLineText(B.queryTypeEnd, QUERY, CALLOUT)).toBe(`/${QUERY}`);
    expect(notesSlashMenuOpen(B.pick - 1)).toBe(true);
    expect(notesSlashMenuOpen(B.pick)).toBe(false);
  });

  it("turns the line into a callout as Enter accepts it, clears the slash, and types the callout", () => {
    expect(notesCalloutAdded(B.pick - 1)).toBe(false);
    expect(notesCalloutAdded(B.pick)).toBe(true);
    expect(notesLineText(B.pick, QUERY, CALLOUT)).toBe("");
    expect(notesLineText((B.calloutTypeStart + B.calloutTypeEnd) / 2, QUERY, CALLOUT).length).toBeGreaterThan(0);
    expect(notesLineText(B.calloutTypeEnd, QUERY, CALLOUT)).toBe(CALLOUT);
    expect(notesLineText(B.end, QUERY, CALLOUT)).toBe(CALLOUT);
    expect(notesLineCaretOn((B.calloutTypeStart + B.calloutTypeEnd) / 2)).toBe(true);
    expect(notesLineCaretOn(B.scrollStart)).toBe(false);
  });

  it("narrows the slash menu to Callout first with each language's query", () => {
    for (const catalog of [en, es]) {
      const t = translatorFor(catalog);
      const query = t("vaultSetup.notes.slashQuery");
      const sections = sectionNotesSlashCommandItems(notesSlashMenuCatalog(true, t), query, []);
      const first = notesSlashMenuGroups(sections, t).flatMap((group) => group.items)[0];
      expect(first?.command).toEqual({ kind: "block", blockType: "callout" });
    }
  });

  it("scrolls the page to the database before the new row is added, and back to its top after the board scrolls", () => {
    expect(notesPageScroll(B.scrollStart)).toBe(0);
    expect(notesPageScroll(B.scrollEnd)).toBe(1);
    expect(notesPageScroll(B.newRowTap)).toBe(1);
    expect(notesPageScroll(B.boardScrollEnd)).toBe(1);
    expect(B.scrollBackStart).toBeGreaterThanOrEqual(B.boardScrollEnd);
    expect(notesPageScroll(B.scrollBackStart)).toBe(1);
    expect(notesPageScroll((B.scrollBackStart + B.scrollBackEnd) / 2)).toBeGreaterThan(0);
    expect(notesPageScroll(B.scrollBackEnd)).toBe(0);
    expect(notesPageScroll(B.end)).toBe(0);
  });

  it("selects the pressed word before the first line's context menu opens once the page is back at its top, opens Paragraph, and turns the line into a quote with Quote", () => {
    expect(B.quotePress).toBeGreaterThanOrEqual(B.scrollBackEnd);
    expect(notesQuoteLineType(B.quotePick - 1)).toBe("paragraph");
    expect(notesContextMenuOpen(B.quoteMenuOpen - 1)).toBe(false);
    expect(notesContextMenuOpen(B.quoteMenuOpen)).toBe(true);
    expect(notesQuoteWordSelected(B.quoteMenuOpen)).toBe(true);
    expect(notesQuoteWordSelected(B.quoteSelect - 1)).toBe(false);
    expect(notesQuoteWordSelected(B.quoteSelect)).toBe(true);
    expect(B.quoteSelect).toBeGreaterThan(B.quotePress);
    expect(B.quoteMenuOpen).toBeGreaterThan(B.quoteLift);
    expect(notesContextMenuOpen(B.quoteLift)).toBe(false);
    expect(notesContextSubmenuOpen(B.submenuOpen - 1)).toBe(false);
    expect(notesContextParagraphHighlighted(B.submenuOpen - 1)).toBe(true);
    expect(notesContextSubmenuOpen(B.submenuOpen)).toBe(true);
    expect(notesContextQuoteHighlighted(B.submenuOpen)).toBe(false);
    expect(notesContextQuoteHighlighted(B.quotePick - 1)).toBe(true);
    expect(notesSubmenuScroll(B.submenuOpen)).toBe(0);
    expect(notesSubmenuScroll(B.quoteScrollEnd)).toBe(1);
    expect(B.quoteScrollEnd).toBeLessThanOrEqual(B.quotePick - PROJECTS_HOVER_LEAD_MS);
    expect(notesContextMenuOpen(B.quotePick)).toBe(false);
    expect(notesContextSubmenuOpen(B.quotePick)).toBe(false);
    expect(notesQuoteWordSelected(B.quotePick)).toBe(false);
    expect(notesQuoteAdded(B.quotePick)).toBe(true);
    expect(notesQuoteLineType(B.quotePick)).toBe("quote");
  });

  it("opens the submenu only inside the context menu, and never opens two menus or the navigator with a menu", () => {
    for (const elapsed of moments()) {
      if (notesContextSubmenuOpen(elapsed)) expect(notesContextMenuOpen(elapsed)).toBe(true);
      const open = [notesContextMenuOpen(elapsed), notesSlashMenuOpen(elapsed), notesRoleMenuOpen(elapsed), notesNavigatorOpen(elapsed)].filter(Boolean);
      expect(open.length).toBeLessThanOrEqual(1);
    }
  });

  it("finds each language's selected word in its first line", () => {
    for (const catalog of [en, es]) {
      const t = translatorFor(catalog);
      const line = t("vaultSetup.notes.quote");
      const word = t("vaultSetup.notes.quoteWord");
      const parts = notesQuoteParts(line, word);
      expect(parts.word).toBe(word);
      expect(parts.before + parts.word + parts.after).toBe(line);
    }
  });

  it("splits a line without the word into the whole line as its word", () => {
    expect(notesQuoteParts("A quiet line", "missing")).toEqual({ before: "", word: "A quiet line", after: "" });
    expect(notesQuoteParts("one two one", "one")).toEqual({ before: "", word: "one", after: " two one" });
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
      expect(notesParagraphFocused(elapsed) && (notesLineFocused(elapsed) || notesNameFocused(elapsed))).toBe(false);
      expect(notesHeadingFocused(elapsed) && (notesParagraphFocused(elapsed) || notesLineFocused(elapsed) || notesNameFocused(elapsed))).toBe(false);
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

  it("scrolls the board sideways only after it has fully faded in, and rests on the new row's column", () => {
    expect(B.boardScrollStart).toBeGreaterThanOrEqual(B.boardTap + NOTES_VIEW_FADE_MS);
    expect(notesBoardScroll(B.boardScrollStart)).toBe(0);
    expect(notesBoardScroll((B.boardScrollStart + B.boardScrollEnd) / 2)).toBeGreaterThan(0);
    expect(notesBoardScroll(B.boardScrollEnd)).toBe(1);
    expect(notesBoardScroll(NOTES_STILL)).toBe(1);
  });

  it("shows the page's top with its first line as a quote, the board with the new row's card in its role, every block written, and every menu closed in its still frame", () => {
    expect(notesPageScroll(NOTES_STILL)).toBe(0);
    expect(notesQuoteLineType(NOTES_STILL)).toBe("quote");
    expect(notesContextMenuOpen(NOTES_STILL)).toBe(false);
    expect(notesContextSubmenuOpen(NOTES_STILL)).toBe(false);
    expect(notesNavigatorOpen(NOTES_STILL)).toBe(false);
    expect(notesBoardShown(NOTES_STILL)).toBe(1);
    expect(notesHeadingAdded(NOTES_STILL)).toBe(true);
    expect(notesHeadingText(NOTES_STILL, HEADING)).toBe(HEADING);
    expect(notesNewRole(NOTES_STILL)).toBe(NOTES_NEW_ROLE);
    expect(notesLineText(NOTES_STILL, QUERY, CALLOUT)).toBe(CALLOUT);
    expect(notesParagraphText(NOTES_STILL, PARAGRAPH)).toBe(PARAGRAPH);
    expect(notesSlashMenuOpen(NOTES_STILL)).toBe(false);
    expect(notesRoleMenuOpen(NOTES_STILL)).toBe(false);
    expect(notesNameFocused(NOTES_STILL)).toBe(false);
    expect(Object.values(notesTouches(NOTES_STILL)).every((touch) => touch === 0)).toBe(true);
  });

  it("clicks with the desktop pointer on the beats the touches land on, resting on each control through its press", () => {
    expect(welcomeCursorScriptOrdered(NOTES_CURSOR)).toBe(true);
    const clicks = NOTES_CURSOR.filter((stop) => stop.press);
    expect(clicks.map((stop) => [stop.target, stop.press?.end])).toEqual([
      ["notes-line", B.lineTap],
      ["notes-new-row", B.newRowTap],
      ["notes-role", B.roleTap],
      ["notes-role-option", B.rolePick],
      ["notes-board-tab", B.boardTap],
      ["notes-quote-word", B.quoteMenuOpen],
      ["notes-context-quote", B.quotePick],
    ]);
    for (const stop of clicks) {
      const press = stop.press;
      if (!press) continue;
      expect(press.start).toBe(press.end - FILM_TAP_MS);
      for (let elapsed = press.start; elapsed <= press.end; elapsed += 10) expect(welcomeCursorHovered(NOTES_CURSOR, elapsed)).toBe(stop.target);
    }
  });

  it("leaves Callout to the keyboard, hides on the line while the page is written, shows in place for the scroll, and waits for the scroll before the new row", () => {
    expect(NOTES_CURSOR.some((stop) => stop.target === "notes-slash-pick")).toBe(false);
    expect(NOTES_CURSOR[2]?.target).toBe("notes-new-row");
    expect(welcomeCursorFrame(NOTES_CURSOR, B.headingTypeStart).away).toBe(true);
    expect(welcomeCursorFrame(NOTES_CURSOR, B.calloutTypeEnd).away).toBe(true);
    for (let elapsed = B.lineTap; elapsed <= B.calloutTypeEnd; elapsed += 10) expect(welcomeCursorFrame(NOTES_CURSOR, elapsed).to.target).toBe("notes-line");
    for (let elapsed = B.scrollStart; elapsed <= B.scrollEnd; elapsed += 10) expect(welcomeCursorFrame(NOTES_CURSOR, elapsed).shown).toBe(1);
    expect(NOTES_CURSOR.find((stop) => stop.target === "notes-new-row")?.depart).toBeGreaterThan(B.scrollStart);
    expect(NOTES_CURSOR.find((stop) => stop.target === "notes-role")?.depart).toBeGreaterThanOrEqual(B.nameSubmit);
  });

  it("moves the pointer from the Board tab onto the board before the board scrolls sideways under it", () => {
    const board = NOTES_CURSOR.find((stop) => stop.target === "notes-board");
    expect(board?.depart).toBeGreaterThanOrEqual(B.boardTap);
    expect(board?.arrive).toBeLessThanOrEqual(B.boardScrollStart);
    expect(board?.busy).toEqual([
      { start: B.boardScrollStart, end: B.boardScrollEnd },
      { start: B.scrollBackStart, end: B.scrollBackEnd },
    ]);
    for (let elapsed = B.boardScrollStart; elapsed <= B.boardScrollEnd; elapsed += 10) {
      expect(welcomeCursorHovered(NOTES_CURSOR, elapsed)).toBe("notes-board");
      expect(welcomeCursorFrame(NOTES_CURSOR, elapsed).away).toBe(false);
    }
  });

  it("rests the pointer on Paragraph until its submenu opens, then moves onto Quote", () => {
    for (let elapsed = NOTES_PARAGRAPH_HOVER; elapsed < B.submenuOpen; elapsed += 10) {
      expect(welcomeCursorHovered(NOTES_CURSOR, elapsed)).toBe("notes-context-paragraph");
    }
    expect(NOTES_CURSOR.find((stop) => stop.target === "notes-context-quote")?.depart).toBeGreaterThanOrEqual(B.submenuOpen);
  });

  it("opens the navigator once the quote is picked, and the folder's level after it", () => {
    expect(B.navStart).toBeGreaterThanOrEqual(B.quotePick);
    expect(notesNavigatorOpen(B.navOpen - 1)).toBe(false);
    expect(notesNavigatorOpen(B.navOpen)).toBe(true);
    expect(notesNavigatorFolderOpen(B.folderOpen - 1)).toBe(false);
    expect(notesNavigatorFolderOpen(B.folderOpen)).toBe(true);
    expect(notesNavigatorOpen(B.end)).toBe(true);
    expect(notesNavigatorFolderOpen(B.end)).toBe(true);
    for (const elapsed of moments()) {
      if (notesNavigatorFolderOpen(elapsed)) expect(notesNavigatorOpen(elapsed)).toBe(true);
    }
  });

  it("highlights the folder on a phone just before its tap, and taps the page's place and the folder on their beats", () => {
    expect(notesNavigatorFolderHighlighted(B.folderOpen - PROJECTS_HOVER_LEAD_MS - 1)).toBe(false);
    expect(notesNavigatorFolderHighlighted(B.folderOpen - PROJECTS_HOVER_LEAD_MS)).toBe(true);
    expect(notesNavigatorFolderHighlighted(B.folderOpen)).toBe(false);
    expect(notesTouches(B.navOpen - FILM_TAP_MS / 2).navigator).toBeGreaterThan(0);
    expect(notesTouches(B.folderOpen - FILM_TAP_MS / 2).folder).toBeGreaterThan(0);
  });

  it("lists folders that hold something at the project's root, and the opened folder with both folders and notes inside it", () => {
    const folder = NOTES_NAVIGATOR_ROOT.find((item) => item.key === NOTES_NAVIGATOR_FOLDER);
    expect(folder).toEqual({ key: NOTES_NAVIGATOR_FOLDER, kind: "folder", hasChildren: true });
    expect(NOTES_NAVIGATOR_ROOT.some((item) => item.key === "page")).toBe(true);
    expect(new Set(NOTES_NAVIGATOR_FOLDER_ITEMS.map((item) => item.kind))).toEqual(new Set(["folder", "page"]));
    const keys = [...NOTES_NAVIGATOR_ROOT, ...NOTES_NAVIGATOR_FOLDER_ITEMS].map((item) => item.key);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("rests the pointer on the page's place in the header until the navigator opens, then moves onto the folder", () => {
    const breadcrumb = NOTES_CURSOR.find((stop) => stop.target === "notes-breadcrumb");
    const folder = NOTES_CURSOR.find((stop) => stop.target === "notes-nav-folder");
    expect(breadcrumb?.arrive).toBe(B.navOpen - SUBMENU_OPEN_DELAY_MS);
    expect(breadcrumb?.depart).toBeGreaterThanOrEqual(B.navStart);
    for (let elapsed = B.navOpen - SUBMENU_OPEN_DELAY_MS; elapsed <= B.navOpen; elapsed += 10) {
      expect(welcomeCursorHovered(NOTES_CURSOR, elapsed)).toBe("notes-breadcrumb");
    }
    expect(folder?.depart).toBeGreaterThanOrEqual(B.navOpen);
    expect(folder?.arrive).toBeLessThanOrEqual(B.folderOpen);
    expect(welcomeCursorHovered(NOTES_CURSOR, B.end)).toBe("notes-nav-folder");
  });
});
