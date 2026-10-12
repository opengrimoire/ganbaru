import type { NotesDestinationPickerTarget } from "$lib/notes/pages/destination-picker";
import { notesPageTitle } from "$lib/notes/pages/title";
import type { NotesPageParentStatus } from "$lib/notes/pages/tree";
import {
  notesPageProjectId,
  notesPagesForProject,
  normalizeNotesProjectId,
} from "$lib/notes/project-membership";
import type { NotesFolder, NotesPage, NotesParent } from "$lib/notes/types";

const SIDEBAR_COLLAPSED_FOLDER_IDS_CONFIG_KEY = "notes.sidebarCollapsedFolderIds";
const ROOT_KEY = "root";

export interface NotesNavigationTreeItemBase {
  key: string;
  depth: number;
  hasChildren: boolean;
  collapsed: boolean;
  matchesQuery: boolean;
  descendantMatchesQuery: boolean;
}

export interface NotesNavigationFolderTreeItem extends NotesNavigationTreeItemBase {
  kind: "folder";
  folder: NotesFolder;
}

export interface NotesNavigationPageTreeItem extends NotesNavigationTreeItemBase {
  kind: "page";
  page: NotesPage;
  parentStatus: NotesPageParentStatus | null;
}

export type NotesNavigationTreeItem =
  | NotesNavigationFolderTreeItem
  | NotesNavigationPageTreeItem;

export type NotesNavigationSortOrder =
  | "name-asc"
  | "name-desc"
  | "modified-desc"
  | "modified-asc"
  | "created-desc"
  | "created-asc";

export interface NotesNavigationTreeOptions {
  /** Restrict the sidebar to folders and their workspace-parented notes. */
  rootPagesOnly?: boolean;
  collapsedPageIds?: readonly string[];
  expandedPageIds?: readonly string[];
  collapsedFolderIds?: readonly string[];
  pageIdsWithChildren?: readonly string[];
  missingParentPageIds?: readonly string[];
  trashedParentPageIds?: readonly string[];
  activePageId?: string | null;
  query?: string;
  untitledTitle?: string;
  titleForPage?: (page: NotesPage) => string;
  sortOrder?: NotesNavigationSortOrder;
}

export interface NotesPageFolderMoveTarget extends NotesDestinationPickerTarget {
  kind: "workspace" | "folder" | "page";
  parent: NotesParent;
  folderId: string | null;
  pageId: string | null;
}

export interface NotesFolderMoveTarget extends NotesDestinationPickerTarget {
  kind: "workspace" | "folder";
  folderId: string | null;
}

interface NavigationFolderNode {
  kind: "folder";
  key: string;
  folder: NotesFolder;
}

interface NavigationPageNode {
  kind: "page";
  key: string;
  page: NotesPage;
}

type NavigationNode = NavigationFolderNode | NavigationPageNode;

interface AppendNavigationItemsOptions {
  parentKey: string;
  depth: number;
  childrenByParentKey: ReadonlyMap<string, readonly NavigationNode[]>;
  parentKeyByNodeKey: ReadonlyMap<string, string>;
  collapsedPageIds: ReadonlySet<string>;
  expandedPageIds: ReadonlySet<string> | null;
  collapsedFolderIds: ReadonlySet<string>;
  pageIdsWithChildren: ReadonlySet<string>;
  missingParentPageIds: ReadonlySet<string>;
  trashedParentPageIds: ReadonlySet<string>;
  activeAncestorKeys: ReadonlySet<string>;
  normalizedQuery: string;
  titleForPage: (page: NotesPage) => string;
  sortOrder: NotesNavigationSortOrder;
  matchByNodeKey: Map<string, boolean>;
  result: NotesNavigationTreeItem[];
}

function folderKey(folderId: string): string {
  return `folder:${folderId}`;
}

function pageKey(pageId: string): string {
  return `page:${pageId}`;
}

/** Return the config key that stores collapsed Notes folder ids. */
export function notesSidebarCollapsedFolderIdsConfigKey(): string {
  return SIDEBAR_COLLAPSED_FOLDER_IDS_CONFIG_KEY;
}

/** Parse persisted collapsed Notes folder ids defensively. */
export function parseStoredNotesSidebarCollapsedFolderIds(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return [...new Set(value.filter((item): item is string => typeof item === "string"))];
}

/** Return Notes folders owned by the selected project. */
export function notesFoldersForProject(
  folders: readonly NotesFolder[],
  projectId: string | null | undefined,
): NotesFolder[] {
  const normalizedProjectId = normalizeNotesProjectId(projectId);
  if (!normalizedProjectId) return [...folders];
  return folders.filter(
    (folder) => normalizeNotesProjectId(folder.project_id) === normalizedProjectId,
  );
}

/** Build a flat visible navigation tree containing folders and pages. */
export function buildNotesNavigationTree(
  pages: readonly NotesPage[],
  folders: readonly NotesFolder[],
  options: NotesNavigationTreeOptions = {},
): NotesNavigationTreeItem[] {
  const titleForPage = options.titleForPage
    ?? ((page: NotesPage) => notesPageTitle(page, options.untitledTitle ?? "Untitled"));
  const folderById = new Map(folders.map((folder) => [folder.id, folder]));
  const pageById = new Map(pages.map((page) => [page.id, page]));
  const missingParentPageIds = new Set(options.missingParentPageIds ?? []);
  const trashedParentPageIds = new Set(options.trashedParentPageIds ?? []);
  const parentKeyByNodeKey = new Map<string, string>();
  const childrenByParentKey = new Map<string, NavigationNode[]>();

  for (const folder of folders) {
    const key = folderKey(folder.id);
    const parentKey = navigationParentKeyForFolder(folder, folderById);
    parentKeyByNodeKey.set(key, parentKey);
    appendNavigationChild(childrenByParentKey, parentKey, { kind: "folder", key, folder });
  }
  for (const page of pages) {
    if (options.rootPagesOnly && page.parent.type !== "workspace") continue;
    const key = pageKey(page.id);
    const parentKey = navigationParentKeyForPage(
      page,
      pageById,
      folderById,
      missingParentPageIds,
      trashedParentPageIds,
    );
    parentKeyByNodeKey.set(key, parentKey);
    appendNavigationChild(childrenByParentKey, parentKey, { kind: "page", key, page });
  }

  const result: NotesNavigationTreeItem[] = [];
  appendNavigationItems({
    parentKey: ROOT_KEY,
    depth: 0,
    childrenByParentKey,
    parentKeyByNodeKey,
    collapsedPageIds: new Set(options.collapsedPageIds ?? []),
    expandedPageIds: options.expandedPageIds ? new Set(options.expandedPageIds) : null,
    collapsedFolderIds: new Set(options.collapsedFolderIds ?? []),
    pageIdsWithChildren: new Set(options.rootPagesOnly ? [] : options.pageIdsWithChildren ?? []),
    missingParentPageIds,
    trashedParentPageIds,
    activeAncestorKeys: navigationActiveAncestorKeys(
      options.activePageId ?? null,
      parentKeyByNodeKey,
    ),
    normalizedQuery: normalizeSearchText(options.query ?? ""),
    titleForPage,
    sortOrder: options.sortOrder ?? "name-asc",
    matchByNodeKey: new Map(),
    result,
  });
  return result;
}

/** Return valid folder and page destinations for moving a page. */
export function notesPageFolderMoveTargets(
  pages: readonly NotesPage[],
  folders: readonly NotesFolder[],
  sourcePageId: string,
  workspaceTitle: string,
  titleForPage: (page: NotesPage) => string = notesPageTitle,
  recentPageIds: readonly string[] = [],
): NotesPageFolderMoveTarget[] {
  const sourcePage = pages.find((page) => page.id === sourcePageId);
  if (!sourcePage) return [];
  const sourceProjectId = notesPageProjectId(sourcePage);
  const scopedPages = sourceProjectId
    ? notesPagesForProject(pages, sourceProjectId)
    : [...pages];
  const scopedFolders = sourceProjectId
    ? notesFoldersForProject(folders, sourceProjectId)
    : [];
  const descendantIds = descendantPageIds(scopedPages, sourcePageId);
  const recentPageIdSet = new Set(recentPageIds);
  const targets: NotesPageFolderMoveTarget[] = [];
  if (sourcePage.parent.type !== "workspace" || sourcePage.folder_id !== null) {
    targets.push({
      kind: "workspace",
      key: "workspace",
      parent: { type: "workspace", workspace: true },
      folderId: null,
      pageId: null,
      title: workspaceTitle,
      path: [],
      depth: 0,
      recent: false,
    });
  }

  const allowedPages = scopedPages.filter(
    (page) => page.id !== sourcePageId && !descendantIds.has(page.id),
  );
  const tree = buildNotesNavigationTree(allowedPages, scopedFolders, { titleForPage });
  const pathByDepth: string[] = [];
  for (const item of tree) {
    const title = navigationItemTitle(item, titleForPage);
    const path = pathByDepth.slice(0, item.depth);
    pathByDepth[item.depth] = title;
    pathByDepth.length = item.depth + 1;
    if (item.kind === "folder") {
      if (sourcePage.folder_id === item.folder.id) continue;
      targets.push({
        kind: "folder",
        key: item.key,
        parent: { type: "workspace", workspace: true },
        folderId: item.folder.id,
        pageId: null,
        title,
        path,
        depth: item.depth,
        recent: false,
      });
      continue;
    }
    if (sourcePage.parent.type === "page_id" && sourcePage.parent.page_id === item.page.id) {
      continue;
    }
    targets.push({
      kind: "page",
      key: item.key,
      parent: { type: "page_id", page_id: item.page.id },
      folderId: null,
      pageId: item.page.id,
      title,
      path,
      depth: item.depth,
      recent: recentPageIdSet.has(item.page.id),
    });
  }
  return targets;
}

/** Return valid folder destinations for moving a folder. */
export function notesFolderMoveTargets(
  folders: readonly NotesFolder[],
  sourceFolderId: string,
  workspaceTitle: string,
): NotesFolderMoveTarget[] {
  const sourceFolder = folders.find((folder) => folder.id === sourceFolderId);
  if (!sourceFolder) return [];
  const projectFolders = notesFoldersForProject(folders, sourceFolder.project_id);
  const descendantIds = descendantFolderIds(projectFolders, sourceFolderId);
  const targets: NotesFolderMoveTarget[] = [];
  if (sourceFolder.parent_folder_id !== null) {
    targets.push({
      kind: "workspace",
      key: "workspace",
      folderId: null,
      title: workspaceTitle,
      path: [],
      depth: 0,
      recent: false,
    });
  }
  const allowedFolders = projectFolders.filter(
    (folder) => folder.id !== sourceFolderId && !descendantIds.has(folder.id),
  );
  const tree = buildNotesNavigationTree([], allowedFolders);
  const pathByDepth: string[] = [];
  for (const item of tree) {
    if (item.kind !== "folder") continue;
    const path = pathByDepth.slice(0, item.depth);
    pathByDepth[item.depth] = item.folder.name;
    pathByDepth.length = item.depth + 1;
    if (sourceFolder.parent_folder_id === item.folder.id) continue;
    targets.push({
      kind: "folder",
      key: item.key,
      folderId: item.folder.id,
      title: item.folder.name,
      path,
      depth: item.depth,
      recent: false,
    });
  }
  return targets;
}

function appendNavigationChild(
  childrenByParentKey: Map<string, NavigationNode[]>,
  parentKey: string,
  node: NavigationNode,
): void {
  const siblings = childrenByParentKey.get(parentKey) ?? [];
  siblings.push(node);
  childrenByParentKey.set(parentKey, siblings);
}

function navigationParentKeyForFolder(
  folder: NotesFolder,
  folderById: ReadonlyMap<string, NotesFolder>,
): string {
  const parentId = folder.parent_folder_id;
  if (!parentId || parentId === folder.id || !folderById.has(parentId)) return ROOT_KEY;
  if (folderHasParentCycle(folder, folderById)) return ROOT_KEY;
  return folderKey(parentId);
}

function navigationParentKeyForPage(
  page: NotesPage,
  pageById: ReadonlyMap<string, NotesPage>,
  folderById: ReadonlyMap<string, NotesFolder>,
  missingParentPageIds: ReadonlySet<string>,
  trashedParentPageIds: ReadonlySet<string>,
): string {
  if (page.parent.type === "page_id") {
    const parentId = page.parent.page_id;
    if (
      parentId === page.id
      || missingParentPageIds.has(parentId)
      || trashedParentPageIds.has(parentId)
      || !pageById.has(parentId)
      || pageHasParentCycle(page, pageById)
    ) {
      return ROOT_KEY;
    }
    return pageKey(parentId);
  }
  if (
    page.parent.type === "workspace"
    && page.folder_id !== null
    && folderById.has(page.folder_id)
  ) {
    return folderKey(page.folder_id);
  }
  return ROOT_KEY;
}

function folderHasParentCycle(
  folder: NotesFolder,
  folderById: ReadonlyMap<string, NotesFolder>,
): boolean {
  const seen = new Set([folder.id]);
  let cursor: NotesFolder | undefined = folder;
  while (cursor?.parent_folder_id) {
    const parentId = cursor.parent_folder_id;
    if (seen.has(parentId)) return true;
    seen.add(parentId);
    cursor = folderById.get(parentId);
  }
  return false;
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

function appendNavigationItems(options: AppendNavigationItemsOptions): void {
  const children = [...(options.childrenByParentKey.get(options.parentKey) ?? [])].sort(
    (left, right) => compareNavigationNodes(left, right, options),
  );
  for (const node of children) {
    const childNodes = options.childrenByParentKey.get(node.key) ?? [];
    const hasChildren = childNodes.length > 0
      || (node.kind === "page" && options.pageIdsWithChildren.has(node.page.id));
    const matchesQuery = navigationNodeMatchesQuery(
      node,
      options.normalizedQuery,
      options.titleForPage,
    );
    const descendantMatchesQuery = childNodes.some((child) =>
      navigationNodeOrDescendantMatchesQuery(child, options)
    );
    if (options.normalizedQuery && !matchesQuery && !descendantMatchesQuery) continue;
    const collapsed = navigationNodeIsCollapsed(node, hasChildren, options);
    if (node.kind === "folder") {
      options.result.push({
        kind: "folder",
        key: node.key,
        folder: node.folder,
        depth: options.depth,
        hasChildren,
        collapsed,
        matchesQuery,
        descendantMatchesQuery,
      });
    } else {
      options.result.push({
        kind: "page",
        key: node.key,
        page: node.page,
        depth: options.depth,
        hasChildren,
        collapsed,
        matchesQuery,
        descendantMatchesQuery,
        parentStatus: navigationPageParentStatus(
          node.page,
          options.parentKeyByNodeKey,
          options.missingParentPageIds,
          options.trashedParentPageIds,
        ),
      });
    }
    if (hasChildren && !collapsed) {
      appendNavigationItems({
        ...options,
        parentKey: node.key,
        depth: options.depth + 1,
      });
    }
  }
}

function compareNavigationNodes(
  left: NavigationNode,
  right: NavigationNode,
  options: Pick<AppendNavigationItemsOptions, "sortOrder" | "titleForPage">,
): number {
  if (left.kind !== right.kind) return left.kind === "folder" ? -1 : 1;
  const leftTitle = left.kind === "folder" ? left.folder.name : options.titleForPage(left.page);
  const rightTitle = right.kind === "folder" ? right.folder.name : options.titleForPage(right.page);
  const nameComparison = leftTitle.localeCompare(rightTitle, undefined, {
    numeric: true,
    sensitivity: "base",
  });
  if (options.sortOrder === "name-asc") return nameComparison || left.key.localeCompare(right.key);
  if (options.sortOrder === "name-desc") return -nameComparison || left.key.localeCompare(right.key);

  const timestampField = options.sortOrder.startsWith("modified")
    ? "last_edited_time"
    : "created_time";
  const leftTimestamp = left.kind === "folder"
    ? left.folder[timestampField]
    : left.page[timestampField];
  const rightTimestamp = right.kind === "folder"
    ? right.folder[timestampField]
    : right.page[timestampField];
  const timestampComparison = leftTimestamp.localeCompare(rightTimestamp);
  const chronologicalComparison = options.sortOrder.endsWith("asc")
    ? timestampComparison
    : -timestampComparison;
  return chronologicalComparison || nameComparison || left.key.localeCompare(right.key);
}

function navigationNodeMatchesQuery(
  node: NavigationNode,
  normalizedQuery: string,
  titleForPage: (page: NotesPage) => string,
): boolean {
  if (!normalizedQuery) return true;
  const title = node.kind === "folder" ? node.folder.name : titleForPage(node.page);
  return normalizeSearchText(title).includes(normalizedQuery);
}

function navigationNodeOrDescendantMatchesQuery(
  node: NavigationNode,
  options: AppendNavigationItemsOptions,
): boolean {
  const cached = options.matchByNodeKey.get(node.key);
  if (cached !== undefined) return cached;
  const matches = navigationNodeMatchesQuery(
    node,
    options.normalizedQuery,
    options.titleForPage,
  ) || (options.childrenByParentKey.get(node.key) ?? []).some((child) =>
    navigationNodeOrDescendantMatchesQuery(child, options)
  );
  options.matchByNodeKey.set(node.key, matches);
  return matches;
}

function navigationNodeIsCollapsed(
  node: NavigationNode,
  hasChildren: boolean,
  options: AppendNavigationItemsOptions,
): boolean {
  if (options.normalizedQuery) return false;
  if (node.kind === "folder") {
    return options.collapsedFolderIds.has(node.folder.id);
  }
  if (options.activeAncestorKeys.has(node.key)) return false;
  if (!hasChildren) return false;
  if (options.expandedPageIds) {
    return !options.expandedPageIds.has(node.page.id);
  }
  return options.collapsedPageIds.has(node.page.id);
}

function navigationActiveAncestorKeys(
  activePageId: string | null,
  parentKeyByNodeKey: ReadonlyMap<string, string>,
): Set<string> {
  const result = new Set<string>();
  if (!activePageId) return result;
  const seen = new Set([pageKey(activePageId)]);
  let parentKey = parentKeyByNodeKey.get(pageKey(activePageId));
  while (parentKey && parentKey !== ROOT_KEY && !seen.has(parentKey)) {
    result.add(parentKey);
    seen.add(parentKey);
    parentKey = parentKeyByNodeKey.get(parentKey);
  }
  return result;
}

function navigationPageParentStatus(
  page: NotesPage,
  parentKeyByNodeKey: ReadonlyMap<string, string>,
  missingParentPageIds: ReadonlySet<string>,
  trashedParentPageIds: ReadonlySet<string>,
): NotesPageParentStatus | null {
  if (page.parent.type !== "page_id") return null;
  const parentId = page.parent.page_id;
  if (trashedParentPageIds.has(parentId)) return "trashed";
  if (missingParentPageIds.has(parentId)) return "missing";
  if (!parentKeyByNodeKey.has(pageKey(parentId))) return "missing";
  return null;
}

function navigationItemTitle(
  item: NotesNavigationTreeItem,
  titleForPage: (page: NotesPage) => string,
): string {
  return item.kind === "folder" ? item.folder.name : titleForPage(item.page);
}

function descendantPageIds(
  pages: readonly NotesPage[],
  sourcePageId: string,
): Set<string> {
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

function descendantFolderIds(
  folders: readonly NotesFolder[],
  sourceFolderId: string,
): Set<string> {
  const childrenByParentId = new Map<string, NotesFolder[]>();
  for (const folder of folders) {
    if (!folder.parent_folder_id) continue;
    const children = childrenByParentId.get(folder.parent_folder_id) ?? [];
    children.push(folder);
    childrenByParentId.set(folder.parent_folder_id, children);
  }
  const descendants = new Set<string>();
  const queue = [...(childrenByParentId.get(sourceFolderId) ?? [])];
  while (queue.length > 0) {
    const folder = queue.shift();
    if (!folder || descendants.has(folder.id)) continue;
    descendants.add(folder.id);
    queue.push(...(childrenByParentId.get(folder.id) ?? []));
  }
  return descendants;
}

function normalizeSearchText(value: string): string {
  return value.trim().toLocaleLowerCase();
}
