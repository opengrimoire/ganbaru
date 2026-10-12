import {
  recordRecentNotesPageId,
  setNotesPageFavoriteId,
} from "$lib/notes/pages/navigation";
import {
  initialNotesFavoritePageIds,
  initialNotesRecentPageIds,
  initialNotesSidebarCollapsedFolderIds,
  saveNotesFavoritePageIds,
  saveNotesRecentPageIds,
  saveNotesSidebarCollapsedFolderIds,
} from "./page-state";

interface NotesSidebarShellMetadata {
  pageIdsWithChildren: readonly string[];
  missingParentPageIds: readonly string[];
  trashedParentPageIds: readonly string[];
}

/** Own persisted Notes sidebar navigation state and shell metadata. */
export function createNotesSidebarController() {
  let favoritePageIds = $state<string[]>(initialNotesFavoritePageIds());
  let recentPageIds = $state<string[]>(initialNotesRecentPageIds());
  let collapsedFolderIds = $state<string[]>(initialNotesSidebarCollapsedFolderIds());
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
    get recentPageIds(): readonly string[] { return recentPageIds; },
    get collapsedFolderIds(): readonly string[] { return collapsedFolderIds; },
    get pageIdsWithChildren(): readonly string[] { return pageIdsWithChildren; },
    get missingParentPageIds(): readonly string[] { return missingParentPageIds; },
    get trashedParentPageIds(): readonly string[] { return trashedParentPageIds; },
    seedPageIds(): string[] {
      return [...new Set([...favoritePageIds, ...recentPageIds])];
    },
    recordRecentPage,
    setPageFavorited,
    setFolderCollapsed,
    replaceMetadata,
    mergeMetadata,
    removePageIds,
  };
}
