import { buildNotesPageTree } from "$lib/notes/pages/tree";
import type { NotesDestinationPickerTarget } from "$lib/notes/pages/destination-picker";
import type { NotesBlock, NotesPage } from "$lib/notes/types";

export interface NotesMoveToPageTarget extends NotesDestinationPickerTarget {
  id: string;
}

export interface NotesMoveToPageTargetOptions {
  recentPageIds?: readonly string[];
  excludedPageIds?: readonly string[];
}

/** Return pages that can receive this block through the handle move menu. */
export function notesMoveToPageTargets(
  pages: readonly NotesPage[],
  block: NotesBlock,
  currentPageId: string,
  untitledLabel: string,
  options: NotesMoveToPageTargetOptions = {},
): NotesMoveToPageTarget[] {
  const excludedPageIds = new Set(options.excludedPageIds ?? []);
  if (block.type === "child_page") excludedPageIds.add(block.id);
  const recentPageIds = new Set(options.recentPageIds ?? []);
  const allowedPages = pages
    .filter((page) => !page.in_trash)
    .filter((page) => page.id !== currentPageId)
    .filter((page) => !excludedPageIds.has(page.id));
  return buildNotesPageTree(allowedPages, {
    titleForPage: (page) => pageTitleForMoveTarget(page, untitledLabel),
  }).map((item) => ({
    key: item.page.id,
    id: item.page.id,
    title: pageTitleForMoveTarget(item.page, untitledLabel),
    path: pagePathTitles(pages, item.page, untitledLabel),
    depth: item.depth,
    recent: recentPageIds.has(item.page.id),
  }));
}

function pageTitleForMoveTarget(page: NotesPage, untitledLabel: string): string {
  const trimmed = pageTitlePlainText(page);
  return trimmed || untitledLabel;
}

function pageTitlePlainText(page: NotesPage): string {
  const title = page.properties.title;
  if (!isTitleProperty(title)) return "";
  return title.title
    .map((item) => item.plain_text)
    .join("")
    .trim();
}

function isTitleProperty(value: unknown): value is { title: { plain_text: string }[] } {
  if (!value || typeof value !== "object" || !("title" in value)) return false;
  const title = (value as { title: unknown }).title;
  return Array.isArray(title)
    && title.every(
      (item) =>
        !!item
        && typeof item === "object"
        && "plain_text" in item
        && typeof (item as { plain_text: unknown }).plain_text === "string",
    );
}

function pagePathTitles(
  pages: readonly NotesPage[],
  page: NotesPage,
  untitledLabel: string,
): string[] {
  const pageById = new Map(pages.map((candidate) => [candidate.id, candidate]));
  const path: string[] = [];
  const seen = new Set([page.id]);
  let cursor = page;
  while (cursor.parent.type === "page_id") {
    const parent = pageById.get(cursor.parent.page_id);
    if (!parent || seen.has(parent.id)) return path.reverse();
    path.push(pageTitleForMoveTarget(parent, untitledLabel));
    seen.add(parent.id);
    cursor = parent;
  }
  return path.reverse();
}
