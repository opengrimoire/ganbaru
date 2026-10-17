import { notesPageTitle } from "./title";
import type { NotesPage } from "$lib/notes/types";

export type NotesPageParentStatus = "missing" | "trashed";

export interface NotesPageTreeItem {
  page: NotesPage;
  depth: number;
  hasChildren: boolean;
  collapsed: boolean;
  matchesQuery: boolean;
  descendantMatchesQuery: boolean;
  parentStatus: NotesPageParentStatus | null;
}

export interface NotesPageTreeOptions {
  collapsedPageIds?: readonly string[];
  expandedPageIds?: readonly string[];
  pageIdsWithChildren?: readonly string[];
  missingParentPageIds?: readonly string[];
  trashedParentPageIds?: readonly string[];
  activePageId?: string | null;
  query?: string;
  untitledTitle?: string;
  titleForPage?: (page: NotesPage) => string;
}

/** Build a visible page hierarchy for navigation and destination pickers. */
export function buildNotesPageTree(
  pages: readonly NotesPage[],
  options: NotesPageTreeOptions = {},
): NotesPageTreeItem[] {
  const titleForPage = options.titleForPage
    ?? ((page: NotesPage) => notesPageTitle(page, options.untitledTitle ?? "Untitled"));
  const normalizedQuery = normalizeSearchText(options.query ?? "");
  const pageById = new Map(pages.map((page) => [page.id, page]));
  const childrenByParentId = new Map<string | null, NotesPage[]>();
  const missingParentPageIds = new Set(options.missingParentPageIds ?? []);
  const trashedParentPageIds = new Set(options.trashedParentPageIds ?? []);
  for (const page of pages) {
    const parentId = sidebarParentPageId(
      page,
      pageById,
      missingParentPageIds,
      trashedParentPageIds,
    );
    const siblings = childrenByParentId.get(parentId) ?? [];
    siblings.push(page);
    childrenByParentId.set(parentId, siblings);
  }

  const collapsedPageIds = new Set(options.collapsedPageIds ?? []);
  const expandedPageIds = options.expandedPageIds
    ? new Set(options.expandedPageIds)
    : null;
  const pageIdsWithChildren = new Set(options.pageIdsWithChildren ?? []);
  const activeAncestorIds = activePageAncestorIds(pageById, options.activePageId ?? null);
  const result: NotesPageTreeItem[] = [];
  appendTreeItems({
    parentId: null,
    depth: 0,
    childrenByParentId,
    collapsedPageIds,
    expandedPageIds,
    pageIdsWithChildren,
    activeAncestorIds,
    missingParentPageIds,
    trashedParentPageIds,
    normalizedQuery,
    titleForPage,
    result,
  });
  return result;
}

interface AppendTreeItemsOptions {
  parentId: string | null;
  depth: number;
  childrenByParentId: ReadonlyMap<string | null, readonly NotesPage[]>;
  collapsedPageIds: ReadonlySet<string>;
  expandedPageIds: ReadonlySet<string> | null;
  pageIdsWithChildren: ReadonlySet<string>;
  activeAncestorIds: ReadonlySet<string>;
  missingParentPageIds: ReadonlySet<string>;
  trashedParentPageIds: ReadonlySet<string>;
  normalizedQuery: string;
  titleForPage: (page: NotesPage) => string;
  result: NotesPageTreeItem[];
}

function appendTreeItems(options: AppendTreeItemsOptions): void {
  const children = options.childrenByParentId.get(options.parentId) ?? [];
  for (const page of children) {
    const childPages = options.childrenByParentId.get(page.id) ?? [];
    const hasChildren = childPages.length > 0 || options.pageIdsWithChildren.has(page.id);
    const matchesQuery = pageMatchesQuery(page, options.normalizedQuery, options.titleForPage);
    const descendantMatchesQuery = hasChildren
      && childPages.some((child) =>
        pageOrDescendantMatchesQuery(
          child,
          options.childrenByParentId,
          options.normalizedQuery,
          options.titleForPage,
        )
      );
    if (options.normalizedQuery && !matchesQuery && !descendantMatchesQuery) continue;
    const parentStatus = pageParentStatus(
      page,
      options.childrenByParentId,
      options.missingParentPageIds,
      options.trashedParentPageIds,
    );
    const collapsed = pageIsCollapsed({
      pageId: page.id,
      hasChildren,
      normalizedQuery: options.normalizedQuery,
      collapsedPageIds: options.collapsedPageIds,
      expandedPageIds: options.expandedPageIds,
      activeAncestorIds: options.activeAncestorIds,
    });
    options.result.push({
      page,
      depth: options.depth,
      hasChildren,
      collapsed,
      matchesQuery,
      descendantMatchesQuery,
      parentStatus,
    });
    if (hasChildren && !collapsed) {
      appendTreeItems({
        ...options,
        parentId: page.id,
        depth: options.depth + 1,
      });
    }
  }
}

interface PageIsCollapsedOptions {
  pageId: string;
  hasChildren: boolean;
  normalizedQuery: string;
  collapsedPageIds: ReadonlySet<string>;
  expandedPageIds: ReadonlySet<string> | null;
  activeAncestorIds: ReadonlySet<string>;
}

function pageIsCollapsed(options: PageIsCollapsedOptions): boolean {
  if (!options.hasChildren || options.normalizedQuery || options.activeAncestorIds.has(options.pageId)) {
    return false;
  }
  if (options.expandedPageIds) {
    return !options.expandedPageIds.has(options.pageId);
  }
  return options.collapsedPageIds.has(options.pageId);
}

function pageOrDescendantMatchesQuery(
  page: NotesPage,
  childrenByParentId: ReadonlyMap<string | null, readonly NotesPage[]>,
  normalizedQuery: string,
  titleForPage: (page: NotesPage) => string,
): boolean {
  if (!normalizedQuery) return true;
  if (pageMatchesQuery(page, normalizedQuery, titleForPage)) return true;
  return (childrenByParentId.get(page.id) ?? []).some((child) =>
    pageOrDescendantMatchesQuery(child, childrenByParentId, normalizedQuery, titleForPage)
  );
}

function pageMatchesQuery(
  page: NotesPage,
  normalizedQuery: string,
  titleForPage: (page: NotesPage) => string,
): boolean {
  if (!normalizedQuery) return true;
  return normalizeSearchText(titleForPage(page)).includes(normalizedQuery);
}

function sidebarParentPageId(
  page: NotesPage,
  pageById: ReadonlyMap<string, NotesPage>,
  missingParentPageIds: ReadonlySet<string>,
  trashedParentPageIds: ReadonlySet<string>,
): string | null {
  if (page.parent.type !== "page_id") return null;
  const parentId = page.parent.page_id;
  if (missingParentPageIds.has(parentId) || trashedParentPageIds.has(parentId)) return null;
  if (!pageById.has(parentId) || parentId === page.id) return null;
  if (pageHasParentCycle(page, pageById)) return null;
  return parentId;
}

function pageParentStatus(
  page: NotesPage,
  childrenByParentId: ReadonlyMap<string | null, readonly NotesPage[]>,
  missingParentPageIds: ReadonlySet<string>,
  trashedParentPageIds: ReadonlySet<string>,
): NotesPageParentStatus | null {
  if (page.parent.type !== "page_id") return null;
  const parentId = page.parent.page_id;
  if (trashedParentPageIds.has(parentId)) return "trashed";
  if (missingParentPageIds.has(parentId)) return "missing";
  const parentChildren = childrenByParentId.get(parentId) ?? [];
  if (!parentChildren.includes(page) && !childrenByParentId.has(parentId)) return "missing";
  return null;
}

function pageHasParentCycle(
  page: NotesPage,
  pageById: ReadonlyMap<string, NotesPage>,
): boolean {
  const seen = new Set([page.id]);
  let cursor: NotesPage | undefined = page;
  while (cursor?.parent.type === "page_id") {
    const parentId = cursor.parent.page_id;
    if (seen.has(parentId)) return true;
    seen.add(parentId);
    cursor = pageById.get(parentId);
  }
  return false;
}

function activePageAncestorIds(
  pageById: ReadonlyMap<string, NotesPage>,
  activePageId: string | null,
): Set<string> {
  const result = new Set<string>();
  if (!activePageId) return result;
  const activePage = pageById.get(activePageId);
  if (!activePage) return result;
  const seen = new Set([activePage.id]);
  let cursor: NotesPage | undefined = activePage;
  while (cursor?.parent.type === "page_id") {
    const parentId = cursor.parent.page_id;
    if (seen.has(parentId)) return result;
    const parent = pageById.get(parentId);
    if (!parent) return result;
    result.add(parent.id);
    seen.add(parent.id);
    cursor = parent;
  }
  return result;
}

function normalizeSearchText(value: string): string {
  return value.trim().toLowerCase();
}
