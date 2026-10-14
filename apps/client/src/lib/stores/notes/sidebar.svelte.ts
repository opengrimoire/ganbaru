import {
  recordRecentNotesPageId,
  setNotesPageFavoriteId,
} from "$lib/notes/pages/navigation";
import {
  initialNotesCustomOrders,
  initialNotesFavoriteFolderIds,
  initialNotesFavoritePageIds,
  initialNotesRecentPageIds,
  initialNotesSidebarCollapsedFolderIds,
  initialNotesSidebarOrderOverrides,
  saveNotesCustomOrders,
  saveNotesFavoriteFolderIds,
  saveNotesFavoritePageIds,
  saveNotesRecentPageIds,
  saveNotesSidebarCollapsedFolderIds,
  saveNotesSidebarOrderOverrides,
} from "./page-state";
import { withCustomSiblingOrder, withSidebarSortOrderOverride, type SidebarSortOrder } from "$lib/utils/sidebar-order";

interface NotesSidebarShellMetadata {
  pageIdsWithChildren: readonly string[];
  missingParentPageIds: readonly string[];
  trashedParentPageIds: readonly string[];
}

/** Own persisted Notes sidebar navigation state and shell metadata. */
export function createNotesSidebarController() {
  let favoritePageIds = $state<string[]>(initialNotesFavoritePageIds());
  let favoriteFolderIds = $state<string[]>(initialNotesFavoriteFolderIds());
  let recentPageIds = $state<string[]>(initialNotesRecentPageIds());
  let collapsedFolderIds = $state<string[]>(initialNotesSidebarCollapsedFolderIds());
  let sidebarOrderOverrides = $state<Record<string, SidebarSortOrder>>(initialNotesSidebarOrderOverrides());
  let customOrders = $state<Record<string, string[]>>(initialNotesCustomOrders());
  let pageIdsWithChildren = $state<string[]>([]);
  let missingParentPageIds = $state<string[]>([]);
  let trashedParentPageIds = $state<string[]>([]);

  function recordRecentPage(pageId: string): void {
    recentPageIds = recordRecentNotesPageId(recentPageIds, pageId);
    saveNotesRecentPageIds(recentPageIds);
  }

  function setPageFavorited(pageId: string, favorited: boolean): void {
    favoritePageIds = setNotesPageFavoriteId(favoritePageIds, pageId, favorited);
    saveNotesFavoritePageIds(favoritePageIds);
  }

  function setFolderFavorited(folderId: string, favorited: boolean): void {
    favoriteFolderIds = setNotesPageFavoriteId(favoriteFolderIds, folderId, favorited);
    saveNotesFavoriteFolderIds(favoriteFolderIds);
  }

  function setProjectSidebarOrder(projectId: string, order: SidebarSortOrder | null): void {
    sidebarOrderOverrides = withSidebarSortOrderOverride(sidebarOrderOverrides, projectId, order);
    saveNotesSidebarOrderOverrides(sidebarOrderOverrides);
  }

  /** Save `orderedSiblingKeys`, the navigation keys of one level's siblings, as their custom order in a project. */
  function setCustomSiblingOrder(projectId: string, orderedSiblingKeys: readonly string[]): void {
    customOrders = { ...customOrders, [projectId]: withCustomSiblingOrder(customOrders[projectId] ?? [], orderedSiblingKeys) };
    saveNotesCustomOrders(customOrders);
  }

  function setFolderCollapsed(folderId: string, collapsed: boolean): void {
    const id = folderId.trim();
    if (!id) return;
    collapsedFolderIds = collapsed
      ? [id, ...collapsedFolderIds.filter((candidate) => candidate !== id)]
      : collapsedFolderIds.filter((candidate) => candidate !== id);
    saveNotesSidebarCollapsedFolderIds(collapsedFolderIds);
  }

  function replaceMetadata(metadata: NotesSidebarShellMetadata): void {
    pageIdsWithChildren = [...metadata.pageIdsWithChildren];
    missingParentPageIds = [...metadata.missingParentPageIds];
    trashedParentPageIds = [...metadata.trashedParentPageIds];
  }

  function mergeMetadata(metadata: NotesSidebarShellMetadata): void {
    pageIdsWithChildren = [
      ...new Set([...pageIdsWithChildren, ...metadata.pageIdsWithChildren]),
    ];
    missingParentPageIds = [...metadata.missingParentPageIds];
    trashedParentPageIds = [...metadata.trashedParentPageIds];
  }

  function removePageIds(pageIds: ReadonlySet<string>): void {
    favoritePageIds = favoritePageIds.filter((id) => !pageIds.has(id));
    recentPageIds = recentPageIds.filter((id) => !pageIds.has(id));
    pageIdsWithChildren = pageIdsWithChildren.filter((id) => !pageIds.has(id));
    missingParentPageIds = missingParentPageIds.filter((id) => !pageIds.has(id));
    trashedParentPageIds = trashedParentPageIds.filter((id) => !pageIds.has(id));
    saveNotesFavoritePageIds(favoritePageIds);
    saveNotesRecentPageIds(recentPageIds);
  }

  return {
    get favoritePageIds(): readonly string[] { return favoritePageIds; },
    get favoriteFolderIds(): readonly string[] { return favoriteFolderIds; },
    get recentPageIds(): readonly string[] { return recentPageIds; },
    get collapsedFolderIds(): readonly string[] { return collapsedFolderIds; },
    sidebarOrderOverride(projectId: string | null): SidebarSortOrder | null {
      return projectId ? sidebarOrderOverrides[projectId] ?? null : null;
    },
    /** Navigation keys of a project's folders and notes in their custom order. */
    customOrder(projectId: string | null): readonly string[] {
      return projectId ? customOrders[projectId] ?? [] : [];
    },
    get pageIdsWithChildren(): readonly string[] { return pageIdsWithChildren; },
    get missingParentPageIds(): readonly string[] { return missingParentPageIds; },
    get trashedParentPageIds(): readonly string[] { return trashedParentPageIds; },
    seedPageIds(): string[] {
      return [...new Set([...favoritePageIds, ...recentPageIds])];
    },
    recordRecentPage,
    setPageFavorited,
    setFolderFavorited,
    setFolderCollapsed,
    setProjectSidebarOrder,
    setCustomSiblingOrder,
    replaceMetadata,
    mergeMetadata,
    removePageIds,
  };
}
