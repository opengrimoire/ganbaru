import { buildNotesPageTree } from "./tree";
import { notesPageTitle } from "./title";
import type { NotesPage, NotesParent } from "$lib/notes/types";
import type { NotesDestinationPickerTarget } from "./destination-picker";

export interface NotesPageMoveTarget extends NotesDestinationPickerTarget {
  parent: NotesParent;
  pageId: string | null;
}

/** Return valid page move destinations for the source page. */
export function notesPageMoveTargets(
  pages: readonly NotesPage[],
  sourcePageId: string,
  workspaceTitle: string,
  titleForPage: (page: NotesPage) => string = notesPageTitle,
  recentPageIds: readonly string[] = [],
): NotesPageMoveTarget[] {
  const sourcePage = pages.find((page) => page.id === sourcePageId);
  if (!sourcePage) return [];
  const descendantIds = descendantPageIds(pages, sourcePageId);
  const recentPageIdSet = new Set(recentPageIds);
  const targets: NotesPageMoveTarget[] = [];
  if (sourcePage.parent.type !== "workspace") {
    targets.push({
      key: "workspace",
      parent: { type: "workspace", workspace: true },
      pageId: null,
      title: workspaceTitle,
      path: [],
      depth: 0,
      recent: false,
    });
  }
  const tree = buildNotesPageTree(
    pages.filter((page) => page.id !== sourcePageId && !descendantIds.has(page.id)),
    { titleForPage },
  );
  for (const item of tree) {
    if (sourcePage.parent.type === "page_id" && sourcePage.parent.page_id === item.page.id) {
      continue;
    }
    targets.push({
      key: item.page.id,
      parent: { type: "page_id", page_id: item.page.id },
      pageId: item.page.id,
      title: titleForPage(item.page),
      path: pagePathTitles(pages, item.page, titleForPage),
      depth: item.depth,
      recent: recentPageIdSet.has(item.page.id),
    });
  }
  return targets;
}

function descendantPageIds(pages: readonly NotesPage[], sourcePageId: string): Set<string> {
  const childrenByParentId = new Map<string, NotesPage[]>();
  for (const page of pages) {
    if (page.parent.type !== "page_id") continue;
    const children = childrenByParentId.get(page.parent.page_id) ?? [];
    children.push(page);
    childrenByParentId.set(page.parent.page_id, children);
  }
  const descendants = new Set<string>();
  const queue = [...(childrenByParentId.get(sourcePageId) ?? [])];
  while (queue.length > 0) {
    const page = queue.shift();
    if (!page || descendants.has(page.id)) continue;
    descendants.add(page.id);
    queue.push(...(childrenByParentId.get(page.id) ?? []));
  }
  return descendants;
}

function pagePathTitles(
  pages: readonly NotesPage[],
  page: NotesPage,
  titleForPage: (page: NotesPage) => string,
): string[] {
  const pageById = new Map(pages.map((candidate) => [candidate.id, candidate]));
  const path: string[] = [];
  const seen = new Set([page.id]);
  let cursor = page;
  while (cursor.parent.type === "page_id") {
    const parent = pageById.get(cursor.parent.page_id);
    if (!parent || seen.has(parent.id)) return path.reverse();
    path.push(titleForPage(parent));
    seen.add(parent.id);
    cursor = parent;
  }
  return path.reverse();
}
