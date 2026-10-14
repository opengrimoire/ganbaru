import { notesPageTitle } from "$lib/notes/pages/title";
import {
  compareSidebarItems,
  compareSidebarNames,
  compareStarredFirst,
  customOrderRanks,
  DEFAULT_SIDEBAR_SORT_ORDER,
  type SidebarSortFields,
  type SidebarSortOrder,
} from "$lib/utils/sidebar-order";
import { NOTES_DATABASE_VIEW_KINDS, type NotesDatabaseView, type NotesDatabaseViewKind, type NotesFolder, type NotesNavigationDatabase, type NotesPage } from "$lib/notes/types";

export const NOTES_HIERARCHY_PANEL_ROW_HEIGHT = 32;
export const NOTES_HIERARCHY_PANEL_LIST_PADDING = 8;
export const NOTES_HIERARCHY_PANEL_CHROME_HEIGHT = 84;
export const NOTES_HIERARCHY_VIEW_PANEL_CHROME_HEIGHT = 40;

export type NotesHierarchyParent =
  | { kind: "root" }
  | { kind: "folder"; id: string }
  | { kind: "page"; id: string }
  | { kind: "database"; id: string };

export type NotesHierarchyNode =
  | { kind: "folder"; key: string; folder: NotesFolder; hasChildren: boolean }
  | { kind: "page"; key: string; page: NotesPage; hasChildren: boolean };

/** Sibling order shared with the sidebar: folders before notes, favorites first within each, then `sortOrder`. */
export interface NotesHierarchyOrder {
  sortOrder: SidebarSortOrder;
  favoriteFolderIds: readonly string[];
  favoritePageIds: readonly string[];
  /** Navigation keys in their custom order, which ranks siblings when `sortOrder` is `custom`. */
  customOrderKeys?: readonly string[];
}

const NAME_ORDER: NotesHierarchyOrder = { sortOrder: DEFAULT_SIDEBAR_SORT_ORDER, favoriteFolderIds: [], favoritePageIds: [] };

export type NotesHierarchyPickerNode = NotesHierarchyNode
  | { kind: "database"; key: string; database: NotesNavigationDatabase; hasChildren: true }
  | { kind: "view"; key: string; database: NotesNavigationDatabase; view: NotesDatabaseView; hasChildren: false };

/** Estimate menu chrome before layout, omitting search in cascading submenus. */
export function notesHierarchyPanelChromeHeight(parent: NotesHierarchyParent, showSearch = true): number {
  const chromeHeight = parent.kind === "database" ? NOTES_HIERARCHY_VIEW_PANEL_CHROME_HEIGHT : NOTES_HIERARCHY_PANEL_CHROME_HEIGHT;
  return chromeHeight - (showSearch ? 0 : NOTES_HIERARCHY_VIEW_PANEL_CHROME_HEIGHT);
}

/** Return the localized display title for any hierarchy picker row. */
export function notesHierarchyPickerTitle(node: NotesHierarchyPickerNode, untitledTitle: string): string {
  if (node.kind === "folder") return node.folder.name;
  if (node.kind === "page") return notesPageTitle(node.page, untitledTitle);
  return node.kind === "view" ? node.view.name : node.database.title.trim() || untitledTitle;
}

/**
 * List a level's children for the hierarchy picker: folders and notes in sidebar order, then the note's database
 * shells by name. A database level lists its saved views in their own order.
 */
export function notesHierarchyPickerChildren(
  pages: readonly NotesPage[],
  folders: readonly NotesFolder[],
  databases: readonly NotesNavigationDatabase[],
  parent: NotesHierarchyParent,
  untitledTitle: string,
  pageIdsWithChildren: readonly string[] = [],
  views: readonly NotesDatabaseView[] = [],
  order: NotesHierarchyOrder = NAME_ORDER,
): NotesHierarchyPickerNode[] {
  if (parent.kind === "database") {
    const database = databases.find((item) => item.id === parent.id);
    if (!database) return [];
    return views.filter((view) => view.parent.database_id === database.id
      && NOTES_DATABASE_VIEW_KINDS.includes(view.type as NotesDatabaseViewKind))
      .map((view) => ({ kind: "view", key: `view:${view.id}`, database, view, hasChildren: false }));
  }
  const items: NotesHierarchyPickerNode[] = notesHierarchyChildren(
    pages, folders, parent, untitledTitle,
    [...pageIdsWithChildren, ...databases.map((database) => database.page_id)],
    order,
  );
  if (parent.kind !== "page") return items;
  const databaseNodes = databases.filter((database) => database.page_id === parent.id)
    .map((database): NotesHierarchyPickerNode => ({ kind: "database", key: `database:${database.id}`, database, hasChildren: true }))
    .sort((left, right) => compareSidebarNames(
      notesHierarchyPickerTitle(left, untitledTitle),
      notesHierarchyPickerTitle(right, untitledTitle),
    ));
  return [...items, ...databaseNodes];
}

function hierarchyNodeSortFields(
  node: NotesHierarchyNode,
  untitledTitle: string,
  customRanks: ReadonlyMap<string, number>,
): SidebarSortFields {
  const rank = customRanks.get(node.key);
  return node.kind === "folder"
    ? { id: node.key, name: node.folder.name, createdAt: node.folder.created_time, modifiedAt: node.folder.last_edited_time, rank }
    : {
      id: node.key,
      name: notesPageTitle(node.page, untitledTitle),
      createdAt: node.page.created_time,
      modifiedAt: node.page.last_edited_time,
      rank,
    };
}

function pageBelongsToParent(page: NotesPage, parent: NotesHierarchyParent): boolean {
  if (parent.kind === "page") {
    return page.parent.type === "page_id" && page.parent.page_id === parent.id;
  }
  if (page.parent.type !== "workspace") return false;
  if (parent.kind === "folder") return page.folder_id === parent.id;
  return page.folder_id === null;
}

function folderBelongsToParent(folder: NotesFolder, parent: NotesHierarchyParent): boolean {
  if (parent.kind === "page") return false;
  if (parent.kind === "folder") return folder.parent_folder_id === parent.id;
  return folder.parent_folder_id === null;
}

function nodeHasChildren(
  node: { kind: "folder"; id: string } | { kind: "page"; id: string },
  pages: readonly NotesPage[],
  pageIdsWithChildren: ReadonlySet<string>,
): boolean {
  if (node.kind === "folder") return true;
  if (pageIdsWithChildren.has(node.id)) return true;
  const parent: NotesHierarchyParent = { kind: "page", id: node.id };
  return pages.some((page) => pageBelongsToParent(page, parent));
}

/** Return only the direct folder and note children for one hierarchy level, folders first, each kind in `order`. */
export function notesHierarchyChildren(
  pages: readonly NotesPage[],
  folders: readonly NotesFolder[],
  parent: NotesHierarchyParent,
  untitledTitle = "Untitled",
  pageIdsWithChildren: readonly string[] = [],
  order: NotesHierarchyOrder = NAME_ORDER,
): NotesHierarchyNode[] {
  if (parent.kind === "database") return [];
  const pageIdsWithChildrenSet = new Set(pageIdsWithChildren);
  const folderNodes: NotesHierarchyNode[] = folders
    .filter((folder) => folderBelongsToParent(folder, parent))
    .map((folder) => ({
      kind: "folder",
      key: `folder:${folder.id}`,
      folder,
      hasChildren: nodeHasChildren(
        { kind: "folder", id: folder.id },
        pages,
        pageIdsWithChildrenSet,
      ),
    }));
  const pageNodes: NotesHierarchyNode[] = pages
    .filter((page) => pageBelongsToParent(page, parent))
    .map((page) => ({
      kind: "page",
      key: `page:${page.id}`,
      page,
      hasChildren: nodeHasChildren(
        { kind: "page", id: page.id },
        pages,
        pageIdsWithChildrenSet,
      ),
    }));

  const favoriteFolderIds = new Set(order.favoriteFolderIds);
  const favoritePageIds = new Set(order.favoritePageIds);
  const customRanks = customOrderRanks(order.customOrderKeys ?? []);
  const isFavorite = (node: NotesHierarchyNode): boolean =>
    node.kind === "folder" ? favoriteFolderIds.has(node.folder.id) : favoritePageIds.has(node.page.id);
  const compareNodes = (left: NotesHierarchyNode, right: NotesHierarchyNode): number =>
    compareStarredFirst(isFavorite(left), isFavorite(right))
      || compareSidebarItems(
        hierarchyNodeSortFields(left, untitledTitle, customRanks),
        hierarchyNodeSortFields(right, untitledTitle, customRanks),
        order.sortOrder,
      );
  return [...folderNodes.sort(compareNodes), ...pageNodes.sort(compareNodes)];
}

/**
 * List the archived notes placed directly under a picker level, in the level's sort order without favorites, for
 * rows that follow the active children. Archived notes never open further levels, and a note that is active again
 * is left out.
 */
export function notesHierarchyArchivedChildren(
  archivedPages: readonly NotesPage[],
  activePageIds: ReadonlySet<string>,
  parent: NotesHierarchyParent,
  untitledTitle: string,
  order: NotesHierarchyOrder = NAME_ORDER,
): NotesHierarchyNode[] {
  if (parent.kind === "database") return [];
  const customRanks = customOrderRanks(order.customOrderKeys ?? []);
  return archivedPages
    .filter((page) => page.archived && !page.in_trash && !activePageIds.has(page.id) && pageBelongsToParent(page, parent))
    .map((page): NotesHierarchyNode => ({ kind: "page", key: `page:${page.id}`, page, hasChildren: false }))
    .sort((left, right) => compareSidebarItems(
      hierarchyNodeSortFields(left, untitledTitle, customRanks),
      hierarchyNodeSortFields(right, untitledTitle, customRanks),
      order.sortOrder,
    ));
}

/** Return the valid folder and note ancestry ending at a selected note. */
export function notesHierarchyPath(
  selectedPageId: string | null,
  pages: readonly NotesPage[],
  folders: readonly NotesFolder[],
  pageIdsWithChildren: readonly string[] = [],
): NotesHierarchyNode[] {
  if (!selectedPageId) return [];
  const pageById = new Map(pages.map((page) => [page.id, page]));
  const folderById = new Map(folders.map((folder) => [folder.id, folder]));
  const pagePath: NotesPage[] = [];
  const visitedPageIds = new Set<string>();
  let page = pageById.get(selectedPageId);

  while (page && !visitedPageIds.has(page.id)) {
    visitedPageIds.add(page.id);
    pagePath.unshift(page);
    page = page.parent.type === "page_id" ? pageById.get(page.parent.page_id) : undefined;
  }

  const folderPath: NotesFolder[] = [];
  const pageIdsWithChildrenSet = new Set(pageIdsWithChildren);
  const rootPage = pagePath[0];
  const visitedFolderIds = new Set<string>();
  let folder = rootPage?.parent.type === "workspace" && rootPage.folder_id
    ? folderById.get(rootPage.folder_id)
    : undefined;
  while (folder && !visitedFolderIds.has(folder.id)) {
    visitedFolderIds.add(folder.id);
    folderPath.unshift(folder);
    folder = folder.parent_folder_id ? folderById.get(folder.parent_folder_id) : undefined;
  }

  return [
    ...folderPath.map((item): NotesHierarchyNode => ({
      kind: "folder",
      key: `folder:${item.id}`,
      folder: item,
      hasChildren: nodeHasChildren(
        { kind: "folder", id: item.id },
        pages,
        pageIdsWithChildrenSet,
      ),
    })),
    ...pagePath.map((item): NotesHierarchyNode => ({
      kind: "page",
      key: `page:${item.id}`,
      page: item,
      hasChildren: nodeHasChildren(
        { kind: "page", id: item.id },
        pages,
        pageIdsWithChildrenSet,
      ),
    })),
  ];
}

/** Return the hierarchy level containing the provided folder or note. */
export function notesHierarchyNodeParent(node: NotesHierarchyNode): NotesHierarchyParent {
  if (node.kind === "folder") {
    return node.folder.parent_folder_id
      ? { kind: "folder", id: node.folder.parent_folder_id }
      : { kind: "root" };
  }
  if (node.page.parent.type === "page_id") {
    return { kind: "page", id: node.page.parent.page_id };
  }
  if (node.page.parent.type === "workspace" && node.page.folder_id) {
    return { kind: "folder", id: node.page.folder_id };
  }
  return { kind: "root" };
}

/** Return the folder containing a note or its root note ancestor. */
export function notesPageContainingFolderId(
  selectedPageId: string | null,
  pages: readonly NotesPage[],
): string | null {
  if (!selectedPageId) return null;
  const pageById = new Map(pages.map((page) => [page.id, page]));
  const visitedPageIds = new Set<string>();
  let page = pageById.get(selectedPageId);
  while (page && !visitedPageIds.has(page.id)) {
    visitedPageIds.add(page.id);
    if (page.parent.type !== "page_id") {
      return page.parent.type === "workspace" ? page.folder_id : null;
    }
    page = pageById.get(page.parent.page_id);
  }
  return null;
}
