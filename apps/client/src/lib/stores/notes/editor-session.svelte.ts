import {
  getNotesDatabaseReference,
  getNotesPageBreadcrumb,
  hydrateNotesBlocks,
  openNotesPage,
} from "$lib/api/notes";
import { blockPlainText, isTextEditableBlock } from "$lib/notes/blocks/factory";
import { getLocalization } from "$lib/i18n/translator.svelte";
import type { NotesBlockLinkTarget, NotesPageLinkTarget } from "$lib/notes/links/block-link";
import { parseNotesLinkHash } from "$lib/notes/links/block-link";
import { notesDatabaseSession } from "$lib/notes/database/session.svelte";
import {
  notesIndentationContextIds,
  blockChildrenAreVisible,
  parentIdForBlock,
  type NotesTreeState,
} from "$lib/notes/blocks/tree";
import {
  notesOutlineSubtreeIds,
  type NotesBlockOutlineItem,
} from "$lib/notes/blocks/outline";
import {
  notesContextualPageOpenMode,
  notesPageOpenModeForSelection,
  notesDefaultOpenModeForProject,
  type NotesPageOpenMode,
} from "$lib/notes/pages/open-mode";
import {
  mergeNotesNavigationPages,
  notesPageProjectId,
} from "$lib/notes/project-membership";
import {
  nextNotesFocusRequest,
  planNotesPageLoadFocus,
  type NotesFocusRequest,
} from "$lib/notes/editor/focus";
import type { NotesDocumentSelection, NotesTextSelection } from "$lib/notes/editor/selection";
import type { NotesUndoSnapshot } from "$lib/notes/history/undo-history";
import {
  type NotesPostMutationResult,
  type NotesSidebarMetadataImpact,
} from "$lib/notes/post-mutation";
import { createNotesSidebarRefreshCoordinator } from "$lib/notes/navigation/sidebar-refresh-coordinator";
import { createNotesBlockActions } from "./block-actions";
import { createNotesDatabaseDeletionController } from "./database-deletion.svelte";
import { createNotesArchiveController } from "./archive.svelte";
import { createNotesSearchController } from "./search.svelte";
import { createNotesLinksController } from "./links.svelte";
import { createNotesCollaborationController } from "./collaboration.svelte";
import { createNotesTransferActions } from "./transfer-actions";
import { createNotesSidebarController } from "./sidebar.svelte";
import { createNotesPageTemplatesController } from "./page-templates.svelte";
import { createNotesFoldersController } from "./folders.svelte";
import { createNotesPageActions } from "./page-actions";
import { BLOCK_HYDRATION_LIMIT, createNotesHydrationController } from "./hydration";
import { createNotesWorkspaceController } from "./workspace.svelte";
import {
  createNotesOptionalSubsystemController,
  type NotesOptionalSubsystem,
  type NotesPagePanelSubsystem,
} from "./optional-subsystems";
import { createNotesPageHistoryController } from "./page-history.svelte";
import { createNotesUndoController } from "./undo";
import { getPreferences } from "$lib/stores/preferences.svelte";
import { resolveSidebarSortOrder, type SidebarSortOrder } from "$lib/utils/sidebar-order";
import { getProjects } from "$lib/stores/projects.svelte";
import {
  flatNotesBlockItems,
  flatNotesBlockItemsForContext,
  isOnlyNotesBlockInContext,
  notesColumnItemsForBlock,
  notesTabItemsForBlock,
  notesTableRowsForBlock,
  notesTreeState,
  previousNotesBlockType,
  type NotesBlockTreeSnapshot,
} from "./block-tree";
import {
  initialNotesSelectedPageId,
  saveNotesSelectedPageId,
} from "./page-state";
import { notesPageSubtreeIds } from "$lib/notes/pages/selection";
import { createNotesBlockPersistence } from "./persistence";
import { NotesPageSessionController } from "./page-session.svelte";
import { NotesTreeProjectionController } from "./tree-projection.svelte";
import { createNotesPageCreationController } from "./page-creation.svelte";
import { invalidateNotesNotificationSchedule } from "$lib/notes/collaboration/notification-schedule.svelte";
import type {
  NotesBlock,
  NotesBlockOutline,
  NotesColumnBlockItems,
  NotesBlockTreeItem,
  NotesBlockType,
  NotesChildDatabaseBlock,
  NotesLoadedPage,
  NotesPage,
  NotesPageBreadcrumbItem,
  NotesPageHistorySettings,
  NotesPageHistorySnapshot,
  NotesTabBlockItems,
  NotesTableRowBlock,
  NotesWorkspaceShell,
  NotesNavigationDatabase,
} from "$lib/notes/types";

type NotesViewMode = "pages" | "archive" | "trash";
export interface NotesSelectPageOptions {
  openMode?: NotesPageOpenMode;
  focusBlockId?: string | null;
  /** Open only this database while retaining its owner's editor session. */
  databaseBlockId?: string;
}

export interface NotesOpenDatabaseOptions {
  /** Hierarchy navigation opens this shell; links normally follow its canonical source. */
  followSource?: boolean;
  viewId?: string;
}

/** A discussion the editor should open its discussions panel on, with a counter that changes per request. */
export interface NotesDiscussionRequest {
  /** Page selected when the request was made, so a request never carries over to the next page. */
  pageId: string | null;
  /** Null once an editor has handled the request. */
  discussionId: string | null;
  requestId: number;
}

interface NotesLoadPageTreeOptions {
  focusOnLoad?: boolean;
  focusBlockId?: string | null;
}

const BLOCK_SAVE_DEBOUNCE_MS = 350;

let pages = $state<NotesPage[]>([]);
let allPages = $state<NotesPage[]>([]);
let navigationDatabases = $state<NotesNavigationDatabase[]>([]);
let pendingPageRemovals = $state<Record<string, string[]>>({});
const pendingPageRemovalIds = $derived(new Set(Object.values(pendingPageRemovals).flat()));
let trashActionError = $state<string | null>(null);
let navigationMutationRevision = 0;
let viewMode = $state<NotesViewMode>("pages");
// Session UI state outlives NotesView when the active application tab changes.
let explorerCollapsed = $state(false);
let workspaceLoaded = $state(false);
const preferences = getPreferences();
const projects = getProjects();
const sidebarController = createNotesSidebarController();
const archiveController = createNotesArchiveController();
const searchController = createNotesSearchController();
const sessions = new Set<NotesEditorStore>();
const foldersController = createNotesFoldersController({
  setFolderCollapsed: (id, collapsed) => sidebarController.setFolderCollapsed(id, collapsed),
  scheduleHierarchyRefresh: () => { sessions.values().next().value?.scheduleHierarchyRefresh(); },
  markNavigationMutation: () => { navigationMutationRevision += 1; },
});
const pageCreationController = createNotesPageCreationController({
  reconcile: async (loaded) => {
    for (const session of sessions) await session.reconcileCreatedPage(loaded);
  },
  afterPersisted: async (pageId) => {
    for (const session of sessions) {
      if (session.selectedPageId === pageId) await session.flushPendingWrites();
    }
  },
});

export interface NotesEditorNavigation {
  select: (pageId: string | null, options: NotesSelectPageOptions) => Promise<void>;
  openLoaded: (loaded: NotesLoadedPage, impact: Exclude<NotesSidebarMetadataImpact, "none">, mode: NotesPageOpenMode) => Promise<void>;
  openProvisional: (loaded: NotesLoadedPage, mode: NotesPageOpenMode) => Promise<void>;
  close: () => Promise<void>;
  showAs: (mode: NotesPageOpenMode) => void;
  focusBlock: (id: string, selection: NotesTextSelection | null, preventScroll: boolean) => boolean;
  focusTitle: (pageId: string) => boolean;
  removePages: (pageIds: readonly string[]) => void;
  afterRemoval: (replacementId: string | null) => Promise<void>;
}

/** Own one live editor's document, history, hydration, drafts, and ordered save queue. */
export function createNotesEditorStore(navigation: NotesEditorNavigation, restoreSelection = false) {
  let editorScrollTop = 0;
  let selectedDatabaseBlockId = $state<string | null>(null);
  let databaseNavigationRequest = 0;
  let pageSelectionRequest = 0;
  let documentSelectionRestore = $state<{ pageId: string; selection: NotesDocumentSelection | null } | null>(null);
  let focusRequest = $state<NotesFocusRequest>({
    blockId: null,
    requestId: 0,
    selection: null,
  });
  let discussionRequest = $state<NotesDiscussionRequest>({
    pageId: null,
    discussionId: null,
    requestId: 0,
  });
  const pageSession = new NotesPageSessionController({
    initialSelectedPageId: restoreSelection ? initialNotesSelectedPageId() : null,
    persistSelectedPageId: saveNotesSelectedPageId,
    recordRecentPage: (pageId) => sidebarController.recordRecentPage(pageId),
  });
  const treeProjection = new NotesTreeProjectionController({
    readSelectedPageId: () => pageSession.selectedPageId,
  });
  const linksController = createNotesLinksController({
    readSelectedPageId: () => pageSession.selectedPageId,
    readSelectedProjectId: () => projects.selectedProjectId,
    readAllPages: () => allPages,
    reloadSelectedPage: async (pageId) => {
      await loadPageTree(pageId);
    },
    scheduleVisibleMetadataRefresh: () => sidebarRefreshCoordinator.schedule("visible-metadata"),
  });
  const navigationPages = $derived(
    mergeNotesNavigationPages(allPages, linksController.destinations)
      .filter((page) => !pendingPageRemovalIds.has(page.id)),
  );
  const {
    reloadArchivedPages,
    loadMoreArchivedPages,
    reloadTrashedPages,
    loadMoreTrashedPages,
  } = archiveController;

  function blockTreeSnapshot(): NotesBlockTreeSnapshot {
    return {
      selectedPageId: pageSession.selectedPageId,
      blocksById: treeProjection.blocksById,
      childIdsByParentId: treeProjection.childIdsByParentId,
    };
  }

  function defaultNotesPageOpenMode(projectId: string | null = projects.selectedProjectId): NotesPageOpenMode {
    return notesDefaultOpenModeForProject(
      preferences.notesDefaultOpenMode,
      projects.projectById(projectId)?.notesDefaultOpenMode,
    );
  }

  function projectIdForPage(pageId: string): string | null {
    const page = allPages.find((candidate) => candidate.id === pageId)
      ?? (treeProjection.loadedPage?.id === pageId ? treeProjection.loadedPage : null);
    return page ? notesPageProjectId(page) : projects.selectedProjectId;
  }

  function treeState(): NotesTreeState {
    return notesTreeState(blockTreeSnapshot());
  }

  function saveSelectedPageId(pageId: string | null): void {
    if (pageId !== pageSession.selectedPageId) {
      blockActions.databasePaste.dismiss();
      databaseDeletion.cancel();
    }
    resetDatabaseSelection();
    pageSession.select(pageId);
  }

  function showSelectedPageAs(openMode: NotesPageOpenMode): void {
    navigation.showAs(openMode);
  }

  function openSelectedPage(pageId: string, openMode: NotesPageOpenMode): void {
    if (pageId !== pageSession.selectedPageId) {
      blockActions.databasePaste.dismiss();
      databaseDeletion.cancel();
    }
    resetDatabaseSelection();
    if (pageId !== pageSession.selectedPageId) editorScrollTop = 0;
    viewMode = "pages";
    pageSession.open(pageId, openMode);
  }

  function recordRecentPage(pageId: string): void {
    pageSession.recordRecentIfCurrent(pageSession.generation, pageId);
  }

  function requestBlockFocus(
    blockId: string | null,
    selection: NotesTextSelection | null = null,
    preventScroll = false,
  ): void {
    if (blockId && !treeProjection.blocksById[blockId] && navigation.focusBlock(blockId, selection, preventScroll)) return;
    focusRequest = nextNotesFocusRequest(focusRequest, blockId, selection, preventScroll);
  }

  function requestTitleFocus(pageId: string): void {
    if (pageId !== pageSession.selectedPageId && navigation.focusTitle(pageId)) return;
    pageSession.requestTitleFocus(pageId);
  }

  function setPageTitleDraft(pageId: string, title: string): void {
    pageCreationController.markChanged(pageId);
    pageSession.setTitleDraft(pageId, title);
  }

  function clearPageTitleDraft(pageId: string): void {
    pageSession.clearTitleDraft(pageId);
  }

  function pageTitleDraftForPage(pageId: string): string | null {
    return pageSession.titleDraftForPage(pageId);
  }

  function replacePages(nextPages: NotesPage[]): void {
    pages = [...nextPages];
  }

  function replaceAllPages(nextPages: NotesPage[]): void {
    allPages = [...nextPages];
  }

  function upsertPageInActiveCollections(page: NotesPage): void {
    pages = pages.some((item) => item.id === page.id)
      ? pages.map((item) => (item.id === page.id ? page : item))
      : [page, ...pages];
    allPages = allPages.some((item) => item.id === page.id)
      ? allPages.map((item) => (item.id === page.id ? page : item))
      : [page, ...allPages];
    navigationMutationRevision += 1;
    invalidateNotesNotificationSchedule();
  }

  function removePagesFromActiveCollections(pageIds: ReadonlySet<string>): void {
    pages = pages.filter((page) => !pageIds.has(page.id));
    allPages = allPages.filter((page) => !pageIds.has(page.id));
    navigationDatabases = navigationDatabases.filter((database) => !pageIds.has(database.page_id));
    navigationMutationRevision += 1;
    invalidateNotesNotificationSchedule();
  }

  function beginPageRemoval(pageId: string): ReadonlySet<string> | null {
    if (pendingPageRemovalIds.has(pageId)) return null;
    const loadedPage = treeProjection.loadedPage;
    const ids = notesPageSubtreeIds(
      loadedPage ? [...navigationPages, loadedPage] : navigationPages,
      pageId,
    );
    if (loadedPage?.id === pageSession.selectedPageId
      && pageSession.breadcrumbs.some((item) => item.id === pageId)) {
      for (const id of notesPageSubtreeIds(navigationPages, loadedPage.id)) ids.add(id);
    }
    pendingPageRemovals = { ...pendingPageRemovals, [pageId]: [...ids] };
    return ids;
  }

  function endPageRemoval(pageId: string): void {
    pendingPageRemovals = Object.fromEntries(
      Object.entries(pendingPageRemovals).filter(([rootId]) => rootId !== pageId),
    );
  }

  function sidebarSeedPageIds(): string[] {
    return sidebarController.seedPageIds();
  }

  function replaceBlock(block: NotesBlock): void {
    treeProjection.replaceBlock(block);
  }

  function applyLocalUndoSnapshot(
    target: NotesUndoSnapshot,
    source: NotesUndoSnapshot,
  ): void {
    treeProjection.applyLocalUndoSnapshot(target, source);
  }

  function insertBlockAfter(block: NotesBlock, afterBlockId: string | null): void {
    treeProjection.insertBlockAfter(block, afterBlockId);
  }

  function insertBlockBefore(block: NotesBlock, beforeBlockId: string): void {
    treeProjection.insertBlockBefore(block, beforeBlockId);
  }

  function removeLeafBlockLocally(blockId: string): boolean {
    return treeProjection.removeLeafBlock(blockId);
  }

  function setLoadedPage(loaded: NotesLoadedPage): void {
    treeProjection.setLoadedPage(loaded);
  }

  function replaceBlockOutlines(outlines: readonly NotesBlockOutline[], pageId: string): void {
    treeProjection.replaceOutlines(outlines, pageId);
  }

  function mergeBlockOutlines(outlines: readonly NotesBlockOutline[], pageId: string): void {
    treeProjection.mergeOutlines(outlines, pageId);
  }

  function requestLoadedPageFocus(
    loaded: NotesLoadedPage,
    focusBlockId: string | null = null,
  ): void {
    requestBlockFocus(
      planNotesPageLoadFocus(
        loaded.blocks.results.map((block) => block.id),
        focusBlockId,
      ),
    );
  }

  async function hydrateBlockRange(
    blockIds: readonly string[],
    generation = pageSession.generation,
  ): Promise<void> {
    await hydrationController.hydrateBlockRange(blockIds, generation);
  }

  function queueDescendantHydration(
    pageId: string | null = pageSession.selectedPageId,
    generation: number = pageSession.generation,
  ): void {
    hydrationController.queueDescendantHydration(pageId, generation);
  }

  async function reloadPages(selectedPageIdOverride: string | null = pageSession.selectedPageId): Promise<void> {
    await workspaceController.reloadPages(selectedPageIdOverride);
  }

  async function loadNavigationChildren(pageId: string): Promise<void> {
    const expandedPageIds = [pageId];
    await workspaceController.reloadPages(pageId, expandedPageIds);
  }

  /** Replace metadata for refreshed notes while retaining the rest of the navigation index. */
  function mergeNavigationDatabases(shell: NotesWorkspaceShell): void {
    const refreshed = new Set(shell.pages.map((page) => page.id));
    navigationDatabases = [
      ...navigationDatabases.filter((database) => !refreshed.has(database.page_id)),
      ...shell.navigation_databases,
    ];
  }

  function mergeReloadedWorkspaceShell(shell: NotesWorkspaceShell): void {
    mergeNavigationDatabases(shell);
    const mergedPages = [...new Map([...allPages, ...shell.pages].map((page) => [page.id, page])).values()];
    replacePages(mergedPages);
    replaceAllPages(mergedPages);
    foldersController.merge(shell.folders);
    sidebarController.mergeMetadata({
      pageIdsWithChildren: shell.page_ids_with_children,
      missingParentPageIds: shell.missing_parent_page_ids,
      trashedParentPageIds: shell.trashed_parent_page_ids,
    });
  }

  function prepareWorkspaceLoad(): void {
    trashActionError = null;
    pageSession.invalidate();
    hydrationController.invalidate();
    treeProjection.resetOutlines();
    optionalSubsystemController.resetAll();
    linksController.resetAll();
    collaborationController.resetPageState();
    pageHistoryController.resetPageState();
    sidebarRefreshCoordinator.cancel();
  }

  function applyInitialWorkspaceShell(
    shell: NotesWorkspaceShell,
    requestedSelection: string | null,
  ): string | null {
    workspaceLoaded = true;
    navigationDatabases = shell.navigation_databases;
    replacePages(shell.pages);
    replaceAllPages([...new Map(
      [...shell.navigation_pages, ...shell.pages].map((page) => [page.id, page]),
    ).values()]);
    foldersController.replace([...new Map(
      [...shell.navigation_folders, ...shell.folders].map((folder) => [folder.id, folder]),
    ).values()]);
    sidebarController.replaceMetadata({
      pageIdsWithChildren: [...new Set([
        ...shell.navigation_page_ids_with_children,
        ...shell.page_ids_with_children,
      ])],
      missingParentPageIds: shell.missing_parent_page_ids,
      trashedParentPageIds: shell.trashed_parent_page_ids,
    });
    const selectionUnchanged = pageSession.selectedPageId === requestedSelection;
    const nextSelected = selectionUnchanged ? shell.resolved_selected_page_id : pageSession.selectedPageId;
    if (selectionUnchanged) saveSelectedPageId(nextSelected);
    return nextSelected;
  }

  function applyAdditionalWorkspaceShell(shell: NotesWorkspaceShell): void {
    mergeNavigationDatabases(shell);
    const mergedPages = [...new Map([...allPages, ...shell.pages].map((page) => [page.id, page])).values()];
    replaceAllPages(mergedPages);
    replacePages(mergedPages);
    foldersController.merge(shell.folders);
    sidebarController.mergeMetadata({
      pageIdsWithChildren: shell.page_ids_with_children,
      missingParentPageIds: shell.missing_parent_page_ids,
      trashedParentPageIds: shell.trashed_parent_page_ids,
    });
  }

  function clearSelectedPageState(): void {
    resetDatabaseSelection();
    treeProjection.clearLoadedTree();
    pageSession.breadcrumbs = [];
    pageHistoryController.resetPageState();
  }

  const sidebarRefreshCoordinator = createNotesSidebarRefreshCoordinator({
    refresh: async (_impact: Exclude<NotesSidebarMetadataImpact, "none">) => {
      await reloadPages();
    },
  });

  function applyPostMutation(
    result: NotesPostMutationResult,
    options: { preserveCreatedPreview?: boolean } = {},
  ): void {
    if (!options.preserveCreatedPreview) pageCreationController.markChanged(pageSession.selectedPageId);
    treeProjection.applyPostMutation(result);
    for (const block of result.blocks ?? []) {
      if (block.type !== "child_database" || block.in_trash || block.archived || !block.child_database.data_source_id) continue;
      const pageId = block.parent.type === "page_id" ? block.parent.page_id
        : treeProjection.blockOutlines.find((outline) => outline.id === block.id)?.page_id ?? treeProjection.loadedPage?.id;
      if (!pageId) continue;
      const database: NotesNavigationDatabase = { id: block.id, page_id: pageId, title: block.child_database.title, data_source_id: block.child_database.data_source_id };
      navigationDatabases = [...navigationDatabases.filter((item) => item.id !== block.id), database];
    }
    if (result.removedBlockIds?.length || result.removedPageIds?.length) {
      const removedBlocks = new Set(result.removedBlockIds ?? []);
      const removedPages = new Set(result.removedPageIds ?? []);
      navigationDatabases = navigationDatabases.filter((database) => !removedBlocks.has(database.id) && !removedPages.has(database.page_id));
    }
    if (selectedDatabaseBlockId && (result.removedBlockIds?.includes(selectedDatabaseBlockId)
      || result.blocks?.some((block) => block.id === selectedDatabaseBlockId
        && (block.type !== "child_database" || block.in_trash || block.archived)))) closeDatabase();
    if (result.pages?.length || result.removedPageIds?.length) {
      for (const session of sessions) {
        if (session !== facade) session.receivePageMetadata(result.pages ?? [], result.removedPageIds ?? []);
      }
    }
    for (const page of result.pages ?? []) upsertPageInActiveCollections(page);
    if (result.removedPageIds?.length) {
      removePagesFromActiveCollections(new Set(result.removedPageIds));
      treeProjection.applyPostMutation({ removedBlockIds: result.removedPageIds });
      navigation.removePages(result.removedPageIds);
    }
    linksController.reconcileDestinationPages(result.pages ?? [], result.removedPageIds ?? []);
    sidebarRefreshCoordinator.schedule(result.sidebarImpact ?? "none");
  }

  let pendingPageOutline: { pageId: string; generation: number; promise: Promise<void> } | null = null;

  async function loadPageTree(pageId: string, options: NotesLoadPageTreeOptions = {}): Promise<boolean> {
    const requestId = pageSession.invalidate();
    treeProjection.primaryContentReady = false;
    const loaded = await openNotesPage(pageId);
    if (!pageSession.isCurrent(requestId, pageId)) return false;
    if (options.focusOnLoad) {
      requestLoadedPageFocus(loaded, options.focusBlockId ?? null);
    }
    setLoadedPage(loaded);
    replaceBlockOutlines(loaded.outlines, pageId);
    const bodyId = blockActions.ensurePageBody(pageId);
    if (options.focusOnLoad && loaded.outlines.length === 0) requestBlockFocus(bodyId, { start: 0, end: 0 });
    pageSession.applyBreadcrumbsIfCurrent(requestId, pageId, loaded.breadcrumb);
    const outlinePromise = hydrationController.loadOutlineDescendantFrontiers(pageId, requestId);
    pendingPageOutline = { pageId, generation: requestId, promise: outlinePromise };
    void outlinePromise.then(() => {
      if (pendingPageOutline?.promise === outlinePromise) pendingPageOutline = null;
    }, (error: unknown) => {
      if (pageSession.isCurrent(requestId, pageId)) {
        workspaceController.setError(error instanceof Error ? error.message : String(error));
      }
    });
    return true;
  }

  async function loadPageTreeForUndo(pageId: string): Promise<void> {
    openSelectedPage(pageId, pageSession.pageOpenMode);
    await loadPageTree(pageId);
    recordRecentPage(pageId);
  }

  async function reloadPageBreadcrumb(pageId: string | null = pageSession.selectedPageId): Promise<void> {
    if (!pageId) {
      pageSession.breadcrumbs = [];
      return;
    }
    const generation = pageSession.generation;
    const breadcrumbs = await getNotesPageBreadcrumb(pageId);
    pageSession.applyBreadcrumbsIfCurrent(generation, pageId, breadcrumbs);
  }

  async function loadOptionalSubsystem(
    subsystem: NotesOptionalSubsystem,
    pageId: string | null,
  ): Promise<void> {
    switch (subsystem) {
      case "templates":
        await pageTemplatesController.reloadPageTemplates();
        break;
      case "local-user":
        await collaborationController.loadLocalUser();
        break;
      case "history-settings":
        await pageHistoryController.loadSettings();
        break;
      case "undo":
        await undoController.hydrate(pageId);
        break;
      case "links":
        await Promise.all([
          linksController.reloadBacklinks(pageId),
          linksController.reloadPageAliases(pageId),
          linksController.reloadUnresolvedLinks(pageId),
        ]);
        break;
      case "comments":
        await Promise.all([
          collaborationController.loadLocalUser(),
          collaborationController.reloadComments(pageId),
          collaborationController.loadPageNotificationSettings(pageId),
        ]);
        break;
      case "suggestions":
        await collaborationController.reloadSuggestions(pageId);
        break;
      case "page-history":
        if (pageId) await pageHistoryController.reloadSnapshots(pageId);
        break;
      case "destinations":
        await linksController.reloadLinkResolutionPages();
        break;
    }
  }

  async function ensureOptionalSubsystem(
    subsystem: NotesOptionalSubsystem,
    pageId: string | null = pageSession.selectedPageId,
  ): Promise<void> {
    return optionalSubsystemController.ensure(subsystem, pageId);
  }

  function setPagePanelSubsystemOpen(
    subsystem: NotesPagePanelSubsystem,
    open: boolean,
  ): void {
    optionalSubsystemController.setPanelOpen(subsystem, open);
  }

  async function refreshOpenLinks(): Promise<void> {
    const pageId = pageSession.selectedPageId;
    if (!pageId || !optionalSubsystemController.isPanelOpen("links")) return;
    const generation = pageSession.generation;
    await Promise.all([
      linksController.reloadBacklinks(pageId),
      linksController.reloadPageAliases(pageId),
      linksController.reloadUnresolvedLinks(pageId),
      linksController.reloadLinkResolutionPages(),
    ]);
    if (!pageSession.isCurrent(generation, pageId)) return;
    optionalSubsystemController.markPageSubsystemLoaded("links", pageId, generation);
  }

  async function selectPageInternal(
    pageId: string | null,
    options: NotesSelectPageOptions,
  ): Promise<void> {
    const selectionRequest = ++pageSelectionRequest;
    if (pageId && pendingPageRemovalIds.has(pageId)) return;
    const alreadyLoaded = pageSession.selectedPageId === pageId && (!pageId || treeProjection.loadedPage?.id === pageId);
    if (!alreadyLoaded || options.databaseBlockId) {
      try {
        await flushPendingWrites();
      } catch {
        // The persistence queue displays the save error and retains the open draft.
        return;
      }
    }
    if (selectionRequest !== pageSelectionRequest || (pageId && pendingPageRemovalIds.has(pageId))) return;
    if (!options.databaseBlockId) closeDatabase();
    const openMode = notesPageOpenModeForSelection({
      requestedOpenMode: options.openMode,
      currentOpenMode: pageSession.pageOpenMode,
      hasOpenPage: pageSession.selectedPageId !== null,
    });
    if (pageId && alreadyLoaded && options.databaseBlockId) {
      resetDatabaseSelection();
      pageSession.pageOpenMode = openMode;
    } else if (pageId) {
      openSelectedPage(pageId, openMode);
    } else {
      saveSelectedPageId(null);
    }
    selectedDatabaseBlockId = options.databaseBlockId ?? null;
    if (alreadyLoaded) {
      if (options.databaseBlockId) {
        await selectDatabaseBlock(pageId, options.databaseBlockId);
        return;
      }
      if (options.focusBlockId) requestPageLoadFocus(options.focusBlockId);
      return;
    }
    pageSession.invalidate();
    hydrationController.invalidate();
    treeProjection.resetOutlines();
    optionalSubsystemController.resetPageScoped();
    undoController.reset(pageId);
    pageHistoryController.resetPageState();
    linksController.resetPageState();
    collaborationController.resetPageState();
    if (!pageId) {
      workspaceController.setError(null);
      workspaceController.setLoading(false);
      treeProjection.clearSelection();
      pageSession.breadcrumbs = [];
      linksController.resetAll();
      pageHistoryController.resetPageState();
      return;
    }
    workspaceController.setLoading(true);
    workspaceController.setError(null);
    const preview = pageCreationController.previewForSelection(pageId);
    if (preview) {
      const currentPage = allPages.find((page) => page.id === pageId) ?? preview.loaded.page;
      const loaded = { ...preview.loaded, page: currentPage };
      if (!options.databaseBlockId) requestLoadedPageFocus(loaded, options.focusBlockId ?? null);
      treeProjection.applyPostMutation({ loadedPage: loaded });
      pageSession.breadcrumbs = [];
      pageSession.recordRecentIfCurrent(pageSession.generation, pageId);
      workspaceController.setLoading(false);
      if (!preview.pending) {
        void reloadPageBreadcrumb(pageId).catch((error: unknown) => {
          console.error("load Notes page breadcrumb failed", error);
        });
      }
      if (options.databaseBlockId) await selectDatabaseBlock(pageId, options.databaseBlockId);
      return;
    }
    const load = loadPageTree(pageId, {
      focusOnLoad: !options.databaseBlockId,
      focusBlockId: options.focusBlockId ?? null,
    });
    const loadGeneration = pageSession.generation;
    try {
      const applied = await load;
      if (applied) pageSession.recordRecentIfCurrent(pageSession.generation, pageId);
      if (applied && options.databaseBlockId) await selectDatabaseBlock(pageId, options.databaseBlockId);
    } catch (error) {
      if (!pageSession.isCurrent(loadGeneration, pageId) || pendingPageRemovalIds.has(pageId)) return;
      if (options.databaseBlockId) closeDatabase();
      workspaceController.setError(error instanceof Error ? error.message : String(error));
      throw error;
    } finally {
      if (pageSession.isCurrent(loadGeneration, pageId)) workspaceController.setLoading(false);
    }
  }

  async function selectPage(
    pageId: string | null,
    options: NotesSelectPageOptions = {},
  ): Promise<void> {
    await navigation.select(pageId, options);
  }

  /** Select a replacement after the removed page's pending writes are settled or discarded. */
  async function selectPageAfterRemoval(pageId: string | null): Promise<void> {
    await navigation.afterRemoval(pageId);
  }

  async function openPageContextually(pageId: string): Promise<void> {
    const openMode = notesContextualPageOpenMode(
      pageSession.pageOpenMode,
      defaultNotesPageOpenMode(projectIdForPage(pageId)),
    );
    await selectPage(pageId, { openMode });
  }

  async function closeContextualPage(): Promise<void> {
    await navigation.close();
  }

  async function activateReturnedPage(
    loaded: NotesLoadedPage,
    sidebarImpact: Exclude<NotesSidebarMetadataImpact, "none">,
    openMode: NotesPageOpenMode = notesContextualPageOpenMode(pageSession.pageOpenMode, defaultNotesPageOpenMode(notesPageProjectId(loaded.page))),
  ): Promise<void> {
    await navigation.openLoaded(loaded, sidebarImpact, openMode);
  }

  async function acceptLoadedPage(loaded: NotesLoadedPage, sidebarImpact: Exclude<NotesSidebarMetadataImpact, "none">, openMode: NotesPageOpenMode): Promise<void> {
    openSelectedPage(loaded.page.id, openMode);
    pageSession.invalidate();
    treeProjection.resetOutlines();
    undoController.reset(loaded.page.id);
    pageHistoryController.resetPageState();
    optionalSubsystemController.resetPageScoped();
    recordRecentPage(loaded.page.id);
    applyPostMutation({
      loadedPage: loaded,
      pages: [loaded.page],
      sidebarImpact,
    });
    await reloadPageBreadcrumb(loaded.page.id);
  }

  async function activateProvisionalPage(
    loaded: NotesLoadedPage,
    openMode: NotesPageOpenMode,
  ): Promise<void> {
    await navigation.openProvisional(loaded, openMode);
  }

  function acceptProvisionalPage(loaded: NotesLoadedPage, openMode: NotesPageOpenMode): void {
    openSelectedPage(loaded.page.id, openMode);
    pageSession.invalidate();
    treeProjection.resetOutlines();
    undoController.reset(loaded.page.id);
    pageHistoryController.resetPageState();
    optionalSubsystemController.resetPageScoped();
    recordRecentPage(loaded.page.id);
    applyPostMutation({
      loadedPage: loaded,
      pages: [loaded.page],
      sidebarImpact: "hierarchy",
    });
  }

  async function reconcileCreatedPage(loaded: NotesLoadedPage): Promise<void> {
    const remainsSelected = pageSession.selectedPageId === loaded.page.id
      && treeProjection.loadedPage?.id === loaded.page.id;
    if (remainsSelected) treeProjection.loadedPage = loaded.page;
    const parent = loaded.page.parent;
    const ownsParent = parent.type === "page_id"
      ? parent.page_id === pageSession.selectedPageId
      : parent.type === "block_id" && Boolean(treeProjection.blocksById[parent.block_id]);
    const childRows = ownsParent && pageSession.selectedPageId && !treeProjection.blocksById[loaded.page.id]
      ? await hydrateNotesBlocks({ page_id: pageSession.selectedPageId, block_ids: [loaded.page.id] })
      : [];
    applyPostMutation({
      blocks: remainsSelected
        ? loaded.blocks.results.filter((block) => !hasLocalChanges(block.id))
        : childRows,
      pages: [loaded.page],
      sidebarImpact: "hierarchy",
    }, { preserveCreatedPreview: true });
    if (remainsSelected) await reloadPageBreadcrumb(loaded.page.id);
  }

  async function activateRestoredPage(page: NotesPage): Promise<void> {
    pageSession.invalidate();
    undoController.reset(page.id);
    pageHistoryController.resetPageState();
    optionalSubsystemController.resetPageScoped();
    openSelectedPage(page.id, "full");
    applyPostMutation({ pages: [page], sidebarImpact: "hierarchy" });
    await loadPageTree(page.id);
    recordRecentPage(page.id);
    requestPageLoadFocus();
  }

  function setFolderCollapsed(folderId: string, collapsed: boolean): void {
    sidebarController.setFolderCollapsed(folderId, collapsed);
  }

  function setPageFavorited(pageId: string, favorited: boolean): void {
    sidebarController.setPageFavorited(pageId, favorited);
  }

  function setFolderFavorited(folderId: string, favorited: boolean): void {
    sidebarController.setFolderFavorited(folderId, favorited);
  }

  /** Sidebar order set for `projectId`, or `null` when it uses the global setting. */
  function sidebarOrderOverride(projectId: string | null): SidebarSortOrder | null {
    return sidebarController.sidebarOrderOverride(projectId);
  }

  /** Sidebar order in effect for `projectId`: its own order, or the global setting. */
  function sidebarOrderForProject(projectId: string | null): SidebarSortOrder {
    return resolveSidebarSortOrder(preferences.notesSidebarOrder, sidebarController.sidebarOrderOverride(projectId));
  }

  /** Set the sidebar order of `projectId`, or return it to the global setting with `null`. */
  function setProjectSidebarOrder(projectId: string, order: SidebarSortOrder | null): void {
    sidebarController.setProjectSidebarOrder(projectId, order);
  }

  /** Navigation keys of the folders and notes of `projectId` in their custom order. */
  function customOrderForProject(projectId: string | null): readonly string[] {
    return sidebarController.customOrder(projectId);
  }

  /** Save the custom order of one level's siblings in `projectId`, given as navigation keys. */
  function setCustomSiblingOrder(projectId: string, orderedSiblingKeys: readonly string[]): void {
    sidebarController.setCustomSiblingOrder(projectId, orderedSiblingKeys);
  }

  async function openArchive(): Promise<void> {
    viewMode = "archive";
    if (!archiveController.archiveLoaded && !archiveController.archiveLoading) {
      await reloadArchivedPages();
    }
  }

  function closeArchive(): void {
    viewMode = "pages";
  }

  async function openTrash(): Promise<void> {
    viewMode = "trash";
    if (!archiveController.trashLoaded && !archiveController.trashLoading) {
      await reloadTrashedPages();
    }
  }

  function closeTrash(): void {
    viewMode = "pages";
  }

  function flatBlockItems(): NotesBlockTreeItem[] {
    return flatNotesBlockItems(blockTreeSnapshot());
  }

  function flatBlockItemsForBlockContext(blockId: string): NotesBlockTreeItem[] {
    return flatNotesBlockItemsForContext(blockTreeSnapshot(), blockId);
  }

  function blockById(blockId: string): NotesBlock | undefined {
    return treeProjection.blocksById[blockId];
  }

  function tableRowsForBlock(blockId: string): NotesTableRowBlock[] {
    return notesTableRowsForBlock(blockTreeSnapshot(), blockId);
  }

  function columnItemsForBlock(blockId: string): NotesColumnBlockItems[] {
    return notesColumnItemsForBlock(blockTreeSnapshot(), blockId);
  }

  function tabItemsForBlock(blockId: string): NotesTabBlockItems[] {
    return notesTabItemsForBlock(blockTreeSnapshot(), blockId);
  }

  function previousBlockType(blockId: string): NotesBlockType | null {
    return previousNotesBlockType(blockTreeSnapshot(), blockId);
  }

  function visibleBlockIds(): string[] {
    return flatBlockItems().map((item) => item.block.id);
  }

  /** Find complete subtree identities from outlines, including unhydrated block bodies. */
  function outlineSubtreeIds(rootBlockIds: readonly string[]): string[] {
    const pageId = pageSession.selectedPageId;
    return pageId ? notesOutlineSubtreeIds(treeProjection.blockOutlines, pageId, rootBlockIds) : [];
  }

  function requestPageLoadFocus(requestedBlockId: string | null = null): void {
    requestBlockFocus(planNotesPageLoadFocus(visibleBlockIds(), requestedBlockId));
  }

  let editorSaveError = $state<string | null>(null);

  const {
    readCanonicalRevision,
    acknowledgeCanonicalBlocks,
    retryEditorMutations,
    enqueueEditorMutation: queueEditorMutation,
    hasLocalChanges,
    localApplyBlockUpdate: applyBlockUpdateLocally,
    markBlockLocallyChanged,
    saveBlockNow,
    scheduleBlockSave: queueBlockSave,
    flushBlockSave,
    flushPendingBlockSaves,
    discardPendingEditorWrites: discardQueuedEditorWrites,
  } = createNotesBlockPersistence({
    reconcileCanonicalBlocks: (blocks) => undoController.reconcileCanonicalBlocks(blocks),
    readBlock: (blockId) => treeProjection.blocksById[blockId],
    beforeSave: () => pageCreationController.awaitReady(pageSession.selectedPageId),
    replaceBlock,
    setLoadError: (message) => {
      editorSaveError = message;
    },
    debounceMs: BLOCK_SAVE_DEBOUNCE_MS,
  });

  function enqueueEditorMutation(mutation: () => Promise<void>): Promise<void> {
    pageCreationController.markChanged(pageSession.selectedPageId);
    return queueEditorMutation(mutation);
  }

  /** Abandon queued writes together with the database reservations that only those writes could complete. */
  function discardPendingEditorWrites(): void {
    discardQueuedEditorWrites();
    blockActions.discardPendingDatabaseCreations();
  }

  /** Reconcile an acknowledged database rename without another write or replacing its identity. */
  function reconcileDatabaseTitle(blockId: string, databaseId: string, title: string): void {
    const block = blockById(blockId);
    if (block?.type !== "child_database" || block.child_database.database_id !== databaseId) return;
    applyPostMutation({ blocks: [{ ...block, child_database: { ...block.child_database, title } }] });
  }

  function localApplyBlockUpdate(blockId: string, update: Parameters<typeof applyBlockUpdateLocally>[1]): void {
    pageCreationController.markChanged(pageSession.selectedPageId);
    applyBlockUpdateLocally(blockId, update);
  }

  function scheduleBlockSave(blockId: string, update: Parameters<typeof queueBlockSave>[1]): void {
    pageCreationController.markChanged(pageSession.selectedPageId);
    queueBlockSave(blockId, update);
  }
  treeProjection.setLocalChangeMarker(markBlockLocallyChanged);

  const undoController = createNotesUndoController({
    loadUndoReferences: async (pageId, ids) => {
      const blocks: NotesBlock[] = [];
      for (let offset = 0; offset < ids.length; offset += BLOCK_HYDRATION_LIMIT) {
        blocks.push(...await hydrateNotesBlocks({ page_id: pageId, block_ids: ids.slice(offset, offset + BLOCK_HYDRATION_LIMIT) }));
      }
      return blocks;
    },
    readCanonicalRevision,
    acknowledgeCanonicalBlocks,
    applyPostMutation,
    restoreDocumentSelection: (pageId, selection) => { documentSelectionRestore = { pageId, selection }; },
    enqueueEditorMutation,
    readSelectedPageId: () => pageSession.selectedPageId,
    readTreeState: treeState,
    loadPageTreeForUndo,
    requestBlockFocus,
    flushPendingMutations: async () => {
      await Promise.all([
        flushPendingBlockSaves(),
        blockActions.flushOptimisticBlockWrites(),
      ]);
    },
    applyLocalSnapshot: applyLocalUndoSnapshot,
  });

  const pageHistoryController = createNotesPageHistoryController({
    readSelectedPageId: () => pageSession.selectedPageId,
    applyPostMutation,
    requestPageLoadFocus,
    flushPendingBlockSaves,
    setLoadError: (message) => {
      workspaceController.setError(message);
    },
  });

  const optionalSubsystemController = createNotesOptionalSubsystemController({
    readPageGeneration: () => pageSession.generation,
    readSelectedPageId: () => pageSession.selectedPageId,
    readSelectedProjectId: () => projects.selectedProjectId,
    load: loadOptionalSubsystem,
  });

  const workspaceController = createNotesWorkspaceController({
    readNavigationMutationRevision: () => navigationMutationRevision,
    readRequestState: () => ({
      projectId: projects.selectedProjectId,
      expandedPageIds: [],
      seedPageIds: sidebarSeedPageIds(),
      selectedPageId: pageSession.selectedPageId,
    }),
    prepareLoad: prepareWorkspaceLoad,
    applyInitialShell: applyInitialWorkspaceShell,
    applyAdditionalShell: applyAdditionalWorkspaceShell,
    mergeReloadedShell: mergeReloadedWorkspaceShell,
    loadSelectedPage: async (pageId) => {
      await loadPageTree(pageId, { focusOnLoad: true });
    },
    clearSelectedPageState,
    readSelectedPageId: () => pageSession.selectedPageId,
  });

  const transferActions = createNotesTransferActions({
    readSelectedPageId: () => pageSession.selectedPageId,
    activateReturnedPage: (nextLoadedPage) => activateReturnedPage(nextLoadedPage, "hierarchy"),
    upsertPage: upsertPageInActiveCollections,
    showPages: () => {
      viewMode = "pages";
    },
    scheduleHierarchyRefresh: () => sidebarRefreshCoordinator.schedule("hierarchy"),
    queueDescendantHydration: () => queueDescendantHydration(),
    requestPageLoadFocus: () => requestPageLoadFocus(),
  });

  const pageTemplatesController = createNotesPageTemplatesController({
    readLoadedPage: () => treeProjection.loadedPage,
    flushPendingBlockSaves,
    activateReturnedPage: (nextLoadedPage) => activateReturnedPage(nextLoadedPage, "hierarchy"),
    queueDescendantHydration: () => queueDescendantHydration(),
    requestPageLoadFocus: () => requestPageLoadFocus(),
  });

  const pageActions = createNotesPageActions({
    readSelectedPageId: () => pageSession.selectedPageId,
    readPages: () => pages,
    readAllPages: () => allPages,
    readLoadedPage: () => treeProjection.loadedPage,
    readFolders: () => foldersController.folders,
    readBlocksById: () => treeProjection.blocksById,
    defaultOpenMode: defaultNotesPageOpenMode,
    activateReturnedPage,
    activateProvisionalPage,
    beginPageCreation: pageCreationController.begin,
    awaitPageReady: pageCreationController.awaitReady,
    awaitPageCreationAttempt: pageCreationController.awaitAttempt,
    discardFailedPageCreation: pageCreationController.discardFailed,
    activateRestoredPage,
    applyPostMutation,
    selectPage: (pageId) => selectPage(pageId),
    selectPageAfterRemoval,
    discardRemovedPageWrites: () => discardPendingEditorWrites(),
    reloadPageBreadcrumb: (pageId) => reloadPageBreadcrumb(pageId),
    flushBlockSave,
    flushPendingBlockSaves,
    flushPendingWrites: async () => {
      for (const session of sessions) {
        if (session !== facade && session.selectedPageId && pendingPageRemovalIds.has(session.selectedPageId)) {
          await session.flushPendingWrites();
        }
      }
      await flushPendingWrites();
    },
    enqueueEditorMutation,
    localApplyBlockUpdate,
    resetUndoHistory: (pageId) => undoController.reset(pageId),
    requestBlockFocus,
    requestTitleFocus,
    requestPageLoadFocus: () => requestPageLoadFocus(),
    queueDescendantHydration: () => queueDescendantHydration(),
    setFolderCollapsed,
    prependArchivedPage: archiveController.prependArchivedPage,
    prependTrashedPage: archiveController.prependTrashedPage,
    removeArchivedPages: archiveController.removeArchivedPages,
    removeTrashedPages: archiveController.removeTrashedPages,
    removePagesFromActiveCollections,
    removeSidebarPageIds: sidebarController.removePageIds,
    scheduleHierarchyRefresh: () => sidebarRefreshCoordinator.schedule("hierarchy"),
    beginPageRemoval,
    endPageRemoval,
    setTrashActionError: (message) => { trashActionError = message; },
  });

  const blockActions = createNotesBlockActions({
    readCanonicalRevision,
    reconcileCanonicalBlocks: undoController.reconcileCanonicalBlocks,
    reconcileCompoundUndo: undoController.reconcileCompoundUndo,
    retryEditorMutations,
    reconcileDatabaseIdentity: undoController.reconcileDatabaseIdentity,
    readPageGeneration: () => pageSession.generation,
    prepareBlockDeletion: async (blockId) => {
      const pageId = pageSession.selectedPageId;
      const generation = pageSession.generation;
      if (pendingPageOutline && pageSession.isCurrent(pendingPageOutline.generation, pendingPageOutline.pageId)) {
        await pendingPageOutline.promise;
      }
      if (!pageId || pageId !== pageSession.selectedPageId || generation !== pageSession.generation) return;
      const required = outlineSubtreeIds([blockId]);
      if (required.some((id) => !blockById(id))) await hydrateBlockRange(required, generation);
      if (pageId !== pageSession.selectedPageId || generation !== pageSession.generation) return;
      if (required.some((id) => !blockById(id))) throw new Error("Notes block descendants could not be loaded for deletion");
    },
    prepareIndentation: (ids, direction) => {
      const generation = pageSession.generation;
      const hydrate = () => {
        if (pageSession.generation !== generation) return;
        const required = notesIndentationContextIds(treeProjection.blockOutlines, ids, direction);
        if (required.some((id) => !blockById(id))) return hydrateBlockRange(required);
      };
      if (pendingPageOutline && pageSession.isCurrent(pendingPageOutline.generation, pendingPageOutline.pageId)) {
        return pendingPageOutline.promise.then(hydrate);
      }
      return hydrate();
    },
    enqueueEditorMutation,
    readPageRootBlockIds: () => [...new Set([
      ...treeProjection.flatBlockOutlines
        .filter(({ outline }) => outline.parent.type === "page_id" && outline.parent.page_id === pageSession.selectedPageId)
        .map(({ outline }) => outline.id),
      ...(treeProjection.childIdsByParentId[pageSession.selectedPageId ?? ""] ?? []),
    ])],
    readSelectedPageId: () => pageSession.selectedPageId,
    readBlocksById: () => treeProjection.blocksById,
    readChildIdsByParentId: () => treeProjection.childIdsByParentId,
    treeState,
    outlineSubtreeIds,
    blockById,
    flatBlockItemsForBlockContext,
    tableRowsForBlock,
    columnItemsForBlock,
    tabItemsForBlock,
    requestBlockFocus,
    createChildPageFromBlock: pageActions.createChildPageFromBlock,
    createChildPageAfterBlock: pageActions.createChildPageAfterBlock,
    applyPostMutation,
    loadPageTree: async (pageId) => {
      await loadPageTree(pageId);
    },
    refreshOpenLinks,
    localApplyBlockUpdate,
    localInsertBlockAfter: insertBlockAfter,
    localInsertBlockBefore: insertBlockBefore,
    localRemoveLeafBlock: removeLeafBlockLocally,
    saveBlockNow,
    scheduleBlockSave,
    flushBlockSave,
    flushPendingBlockSaves,
    awaitSelectedPageReady: () => pageCreationController.awaitReady(pageSession.selectedPageId),
    createUndoSnapshot: undoController.snapshot,
    createUndoSnapshotForBlocks: undoController.snapshotBlocks,
    recordUndo: undoController.record,
  });

  const databaseDeletion = createNotesDatabaseDeletionController({
    readPageId: () => pageSession.selectedPageId,
    blockById,
  });

  /** Confirm one deleted block while allowing its surviving descendants to be reparented. */
  async function deleteBlock(blockId: string): Promise<boolean> {
    const pageId = pageSession.selectedPageId;
    const generation = pageSession.generation;
    if (!pageId) return false;
    if (!blockById(blockId)) await hydrateBlockRange([blockId], generation);
    if (!pageSession.isCurrent(generation, pageId) || !blockById(blockId)) return false;
    if (!await databaseDeletion.request([blockId]) || !pageSession.isCurrent(generation, pageId)) return false;
    await blockActions.deleteBlock(blockId);
    return true;
  }

  /** Inspect the complete removed subtree before deleting a block selection. */
  async function deleteBlockSelection(ids: readonly string[]): Promise<boolean> {
    const pageId = pageSession.selectedPageId;
    const generation = pageSession.generation;
    if (!pageId || !ids.length) return false;
    if (pendingPageOutline && pageSession.isCurrent(pendingPageOutline.generation, pendingPageOutline.pageId)) {
      await pendingPageOutline.promise;
    }
    if (!pageSession.isCurrent(generation, pageId)) return false;
    const required = [...new Set([...ids, ...outlineSubtreeIds(ids)])];
    if (required.some((id) => !blockById(id))) await hydrateBlockRange(required, generation);
    if (!pageSession.isCurrent(generation, pageId)) return false;
    if (required.some((id) => !blockById(id))) throw new Error("Notes deletion descendants could not be loaded");
    if (!await databaseDeletion.request(required) || !pageSession.isCurrent(generation, pageId)) return false;
    await blockActions.deleteBlockSelection(ids);
    return true;
  }

  /** Preserve database source identities instead of turning their shells into unrelated blocks. */
  async function convertBlock(...args: Parameters<typeof blockActions.convertBlock>): Promise<void> {
    if (blockById(args[0])?.type === "child_database" && args[1] !== "child_database") {
      throw new Error(getLocalization().t("notes.databaseCannotConvert"));
    }
    await blockActions.convertBlock(...args);
  }

  /** Load affected descendants and confirm removed databases before changing a document range. */
  async function replaceDocumentRange(ids: readonly string[], start: number, end: number, text: string, html?: string, documentSelection?: NotesDocumentSelection): Promise<boolean> {
    const pageId = pageSession.selectedPageId;
    const generation = pageSession.generation;
    if (pendingPageOutline && pageSession.isCurrent(pendingPageOutline.generation, pendingPageOutline.pageId)) {
      await pendingPageOutline.promise;
    }
    if (!pageId || !ids.length || !pageSession.isCurrent(generation, pageId)) return false;
    if (ids.some((id) => !blockById(id))) await hydrateBlockRange(ids, generation);
    if (!pageSession.isCurrent(generation, pageId)) return false;
    const first = blockById(ids[0]);
    const firstIsFullySelected = first && start === 0
      && (ids.length > 1 || end >= (isTextEditableBlock(first.type) ? blockPlainText(first).length : 1));
    const required = [...new Set([
      ...ids,
      ...outlineSubtreeIds(firstIsFullySelected ? ids : ids.slice(1)),
    ])];
    if (required.some((id) => !blockById(id))) await hydrateBlockRange(required);
    if (!pageSession.isCurrent(generation, pageId)) return false;
    if (required.some((id) => !blockById(id))) throw new Error("Notes selection content could not be loaded");
    const selectedIds = ids.filter((_id, index) => (index > 0 || start === 0)
      && (index < ids.length - 1 || end > 0));
    const removedCollapsedRoots = ids.filter((id, index) => {
      const block = blockById(id);
      if (!block || blockChildrenAreVisible(block)) return false;
      const length = isTextEditableBlock(block.type) ? blockPlainText(block).length : 1;
      return (index > 0 || start === 0) && (index < ids.length - 1 || (end > 0 && end >= length));
    });
    const removedIds = [...new Set([...selectedIds, ...outlineSubtreeIds(removedCollapsedRoots)])];
    if (!await databaseDeletion.request(removedIds) || !pageSession.isCurrent(generation, pageId)) return false;
    await blockActions.replaceDocumentRange(ids, start, end, text, html, documentSelection);
    return true;
  }

  const collaborationController = createNotesCollaborationController({
    readSelectedPageId: () => pageSession.selectedPageId,
    readBlocksById: () => treeProjection.blocksById,
    flushBlockSave,
    requestBlockFocus: (blockId) => requestBlockFocus(blockId),
    updateBlockRichText: blockActions.updateBlockRichText,
    isPanelOpen: (panel) => optionalSubsystemController.isPanelOpen(panel),
  });

  const hydrationController = createNotesHydrationController({
    hasLocalChanges,
    readPageGeneration: () => pageSession.generation,
    readSelectedPageId: () => pageSession.selectedPageId,
    readBlockOutlines: () => treeProjection.blockOutlines,
    readFlatBlockOutlines: () => treeProjection.flatBlockOutlines,
    readBlocksById: () => treeProjection.blocksById,
    readFocusRequest: () => selectedDatabaseBlockId
      ? { ...focusRequest, blockId: selectedDatabaseBlockId }
      : focusRequest,
    mergeBlockOutlines,
    replaceHydratedBlocks: (nextBlocksById, nextChildIdsByParentId) => {
      treeProjection.replaceHydratedBlocks(nextBlocksById, nextChildIdsByParentId);
    },
    setLoadError: (message) => {
      workspaceController.setError(message);
    },
  });

  const {
    updateBlockText, updateBlockRichText,
    insertPageMention,
    insertDateMention,
    insertObjectMention,
    insertInlineEquation,
    updateBlockTextLink,
    updateBlockTextAnnotations,
    updateBookmark,
    updateEmbedUrl,
    updateLinkPreviewUrl,
    updateEquationExpression,
    updateMedia,
    updateTableCell,
    updateTableCellRichText,
    addTableRow,
    removeTableRow,
    addTableColumn,
    removeTableColumn,
    addColumn,
    removeColumn,
    moveColumn,
    resizeColumn,
    moveBlockToColumn,
    updateTabLabel,
    updateTabIcon,
    addTab,
    removeTab,
    moveTab,
    moveBlockToTab,
    toggleTodo,
    updateCodeLanguage,
    updateBlockColor,
    updateCalloutIcon,
    updateToggleOpen,
    convertBlockToToggleHeading,
    createSiblingAfter,
    splitTextBlockAtSelection,
    insertParagraphAdjacent,
    pastePlainTextIntoBlock,
    pasteRichHtmlIntoBlock,
    pasteBlockSelection,
    mergeBlockWithPrevious,
    indentBlockSelection,
    nestBlock,
    outdentBlock,
    moveBlockUp,
    moveBlockDown,
    moveBlockSelection,
    dropBlockWithinSiblings,
    dropBlockOnBlock,
    moveBlockToPage,
    duplicateBlock,
    duplicateBlockSelection,
    addTemplateChild,
    useTemplateBlock,
    addButtonChild,
    updateButtonIcon,
    updateButtonInsertPosition,
    useButtonBlock,
    createLinkedDatabaseViewAfter,
    convertUnsupportedBlock,
  } = blockActions;

  function isOnlyBlock(blockId: string): boolean {
    return isOnlyNotesBlockInContext(blockTreeSnapshot(), blockId);
  }

  function focusBlock(blockId: string, selection: NotesTextSelection | null = null, preventScroll = false): void {
    requestBlockFocus(blockId, selection, preventScroll);
  }

  async function undoNotesEdit(): Promise<boolean> {
    try {
      if (!undoController.canUndo() && pageSession.selectedPageId) {
        await ensureOptionalSubsystem("undo", pageSession.selectedPageId);
      }
      return await undoController.undo();
    } catch (error) {
      workspaceController.setError(error instanceof Error ? error.message : String(error));
      throw error;
    }
  }

  async function redoNotesEdit(): Promise<boolean> {
    try {
      if (!undoController.canRedo() && pageSession.selectedPageId) {
        await ensureOptionalSubsystem("undo", pageSession.selectedPageId);
      }
      return await undoController.redo();
    } catch (error) {
      workspaceController.setError(error instanceof Error ? error.message : String(error));
      throw error;
    }
  }

  async function openBlockLink(target: NotesBlockLinkTarget): Promise<boolean> {
    return openNotesLink(target);
  }

  /** Cancel pending database navigation without discarding the owner's document. */
  function resetDatabaseSelection(): void {
    databaseNavigationRequest += 1;
    selectedDatabaseBlockId = null;
  }

  /** Return to the containing note without reloading blocks or database snapshots. */
  function closeDatabase(): void {
    const blockId = selectedDatabaseBlockId;
    pageSelectionRequest += 1;
    resetDatabaseSelection();
    if (typeof window === "undefined" || !blockId) return;
    const target = parseNotesLinkHash(window.location.hash);
    if (target?.pageId !== pageSession.selectedPageId || target.blockId !== blockId) return;
    const url = new URL(window.location.href);
    url.hash = `notes?${new URLSearchParams({ page: target.pageId }).toString()}`;
    window.history.replaceState(window.history.state, "", url);
  }

  /** Hydrate a database reference under its owner and ignore late navigation results. */
  async function selectDatabaseBlock(pageId: string | null, blockId: string): Promise<void> {
    if (!pageId || treeProjection.loadedPage?.id !== pageId) return;
    const generation = pageSession.generation;
    const request = databaseNavigationRequest;
    const cached = treeProjection.blocksById[blockId];
    let block: NotesBlock | undefined;
    try {
      block = cached ?? (await hydrateNotesBlocks({ page_id: pageId, block_ids: [blockId] }))
        .find((candidate) => candidate.id === blockId);
    } catch (error: unknown) {
      if (pageSession.isCurrent(generation, pageId) && request === databaseNavigationRequest) closeDatabase();
      throw error;
    }
    if (!pageSession.isCurrent(generation, pageId) || request !== databaseNavigationRequest) return;
    if (block?.type !== "child_database" || block.in_trash || block.archived) {
      closeDatabase();
      return;
    }
    if (!cached) replaceBlock(block);
    selectedDatabaseBlockId = block.id;
  }

  /** Open a database alone in the owning pane after its pending note writes settle. */
  async function openDatabase(target: string | NotesBlockLinkTarget, options: NotesOpenDatabaseOptions = {}): Promise<boolean> {
    const blockId = typeof target === "string" ? target : target.blockId;
    const request = ++databaseNavigationRequest;
    const generation = pageSession.generation;
    const reference = await getNotesDatabaseReference(blockId);
    if (!pageSession.isCurrent(generation) || request !== databaseNavigationRequest) return false;
    if (typeof target !== "string" && reference.page_id !== target.pageId) return false;
    const pageId = options.followSource === false ? reference.page_id : reference.source_page_id;
    const databaseBlockId = options.followSource === false ? reference.block_id : reference.source_block_id;
    if (pendingPageRemovalIds.has(pageId)) return false;
    viewMode = "pages";
    if (options.viewId) notesDatabaseSession.selectView(databaseBlockId, options.viewId);
    await navigation.select(pageId, {
      databaseBlockId,
    });
    const opened = [...sessions].some((session) => session.selectedPageId === pageId
      && session.selectedDatabaseBlock?.id === databaseBlockId);
    if (opened && typeof window !== "undefined") {
      const hashTarget = parseNotesLinkHash(window.location.hash);
      if (hashTarget?.pageId === reference.page_id && hashTarget.blockId === reference.block_id) {
        const url = new URL(window.location.href);
        url.hash = `notes?${new URLSearchParams({ page: pageId, block: databaseBlockId }).toString()}`;
        window.history.replaceState(window.history.state, "", url);
      }
    }
    return opened;
  }

  /** Ask one editor showing the linked page, preferring this one, to reveal the linked thread. */
  function revealLinkedDiscussion(target: NotesPageLinkTarget): void {
    if (!target.discussionId) return;
    const showing = [...sessions].filter((session) => session.selectedPageId === target.pageId);
    (showing.find((session) => session === facade) ?? showing[0])?.requestDiscussion(target.discussionId);
  }

  /** Ask the editor showing the selected page to open its discussions on one thread. */
  function requestDiscussion(discussionId: string): void {
    discussionRequest = {
      pageId: pageSession.selectedPageId,
      discussionId,
      requestId: discussionRequest.requestId + 1,
    };
  }

  /** Mark a discussion request handled, so an editor mounted later for the same page does not reveal it again. */
  function completeDiscussionRequest(requestId: number): void {
    if (discussionRequest.requestId !== requestId || discussionRequest.discussionId === null) return;
    discussionRequest = { ...discussionRequest, discussionId: null };
  }

  async function openNotesLink(target: NotesPageLinkTarget): Promise<boolean> {
    viewMode = "pages";
    if (!workspaceLoaded) await workspaceController.ensureLoaded();
    if (!allPages.some((page) => page.id === target.pageId)) {
      await reloadPages(target.pageId);
    }
    if (!allPages.some((page) => page.id === target.pageId)) return false;
    if (target.blockId) {
      const generation = pageSession.generation;
      const request = ++databaseNavigationRequest;
      const cached = treeProjection.loadedPage?.id === target.pageId
        ? treeProjection.blocksById[target.blockId] : undefined;
      const block = cached ?? (await hydrateNotesBlocks({
        page_id: target.pageId,
        block_ids: [target.blockId],
      })).find((candidate) => candidate.id === target.blockId);
      if (!pageSession.isCurrent(generation) || request !== databaseNavigationRequest) return false;
      if (!block || block.in_trash || block.archived) return false;
      // A view link names a view of the linked database itself, so it opens that database instead of its source.
      if (block.type === "child_database") {
        return openDatabase({ pageId: target.pageId, blockId: block.id }, target.viewId ? { viewId: target.viewId, followSource: false } : {});
      }
    }
    if (treeProjection.loadedPage?.id !== target.pageId) {
      await selectPage(target.pageId, { focusBlockId: target.blockId ?? null });
      const opened = [...sessions].some((session) => session.selectedPageId === target.pageId
        && (!target.blockId || session.blockById(target.blockId)));
      if (opened) revealLinkedDiscussion(target);
      return opened;
    }
    if (!target.blockId) {
      const wasDatabase = selectedDatabaseBlockId !== null;
      closeDatabase();
      if (!wasDatabase && !target.discussionId) requestPageLoadFocus();
      revealLinkedDiscussion(target);
      return true;
    }
    if (!treeProjection.blocksById[target.blockId] || !visibleBlockIds().includes(target.blockId)) return false;
    closeDatabase();
    requestBlockFocus(planNotesPageLoadFocus(visibleBlockIds(), target.blockId));
    return true;
  }

  async function flushPendingWrites(): Promise<void> {
    await Promise.all([
      flushPendingBlockSaves(),
      blockActions.flushOptimisticBlockWrites(),
    ]);
  }

  const facade = {
    acceptLoadedPage,
    acceptProvisionalPage,
    reconcileCreatedPage,
    selectPageLocally: (id: string | null, options: NotesSelectPageOptions = {}) => selectPageInternal(id, options),
    setOpenMode: (mode: NotesPageOpenMode): void => { pageSession.pageOpenMode = mode; },
    requestTitleFocus,
    scheduleHierarchyRefresh: () => sidebarRefreshCoordinator.schedule("hierarchy"),
    receivePageMetadata: (changedPages: readonly NotesPage[], removedPageIds: readonly string[]): void => {
      treeProjection.applyPostMutation({ pages: changedPages, removedBlockIds: removedPageIds });
      linksController.reconcileDestinationPages(changedPages, removedPageIds);
    },
    clearRemovedPage: (): void => {
      blockActions.databasePaste.dismiss();
      databaseDeletion.cancel();
      resetDatabaseSelection();
      discardPendingEditorWrites();
      pageSession.select(null);
      hydrationController.invalidate();
      treeProjection.clearSelection();
      pageSession.breadcrumbs = [];
      undoController.reset(null);
    },
    prepareToClose: async (): Promise<void> => {
      await flushPendingWrites();
      if (undoController.canUndo() || undoController.canRedo()) await undoController.persist();
    },
    dispose: (): void => {
      blockActions.databasePaste.dismiss();
      databaseDeletion.cancel();
      resetDatabaseSelection();
      pageSession.invalidate();
      hydrationController.invalidate();
      sidebarRefreshCoordinator.cancel();
      undoController.dispose();
      sessions.delete(facade);
    },
    /** Whether the desktop Notes explorer is collapsed for this window session. */
    get explorerCollapsed(): boolean {
      return explorerCollapsed;
    },
    set explorerCollapsed(collapsed: boolean) {
      explorerCollapsed = collapsed;
    },
    get editorSaveError() { return editorSaveError; },
    get trashActionError(): string | null { return trashActionError; },
    retryEditorMutations: async () => {
      await retryEditorMutations();
      await undoController.persist();
    },
    get pages(): NotesPage[] {
      return pages;
    },
    get allPages(): NotesPage[] {
      return allPages;
    },
    get navigationPages(): NotesPage[] {
      return navigationPages;
    },
    /** Overlay live database identities and titles on the compact navigation metadata. */
    get navigationDatabases(): NotesNavigationDatabase[] {
      const byId = new Map(navigationDatabases.map((database) => [database.id, database]));
      const pageId = treeProjection.loadedPage?.id;
      if (pageId) {
        for (const block of Object.values(treeProjection.blocksById)) {
          if (block.type !== "child_database" || block.in_trash || block.archived || !block.child_database.data_source_id) continue;
          byId.set(block.id, { id: block.id, page_id: pageId, title: block.child_database.title, data_source_id: block.child_database.data_source_id });
        }
      }
      const activePageIds = new Set(allPages.filter((page) => !page.in_trash && !page.archived).map((page) => page.id));
      return [...byId.values()].filter((database) => activePageIds.has(database.page_id));
    },
    get folders() {
      return foldersController.folders;
    },
    get archivedPages(): NotesPage[] {
      return archiveController.archivedPages;
    },
    get archiveHasMore(): boolean {
      return archiveController.archiveHasMore;
    },
    get pageTemplates() {
      return pageTemplatesController.templates;
    },
    get workspacePages(): NotesPage[] {
      return allPages.filter((page) => page.parent.type === "workspace");
    },
    get favoritePageIds(): readonly string[] {
      return sidebarController.favoritePageIds;
    },
    get favoriteFolderIds(): readonly string[] {
      return sidebarController.favoriteFolderIds;
    },
    get recentPageIds(): readonly string[] {
      return sidebarController.recentPageIds;
    },
    get collapsedFolderIds(): readonly string[] {
      return sidebarController.collapsedFolderIds;
    },
    get sidebarPageIdsWithChildren(): readonly string[] {
      return sidebarController.pageIdsWithChildren;
    },
    get sidebarMissingParentPageIds(): readonly string[] {
      return sidebarController.missingParentPageIds;
    },
    get sidebarTrashedParentPageIds(): readonly string[] {
      return sidebarController.trashedParentPageIds;
    },
    get trashedPages(): NotesPage[] {
      return archiveController.trashedPages;
    },
    get trashHasMore(): boolean {
      return archiveController.trashHasMore;
    },
    get selectedPageId(): string | null {
      return pageSession.selectedPageId;
    },
    /** Database destination, including the brief interval while its owner loads. */
    get selectedDatabaseBlockId(): string | null {
      return selectedDatabaseBlockId;
    },
    /** The live database reference selected within this pane's owner note. */
    get selectedDatabaseBlock(): NotesChildDatabaseBlock | null {
      if (treeProjection.loadedPage?.id !== pageSession.selectedPageId) return null;
      const block = selectedDatabaseBlockId ? treeProjection.blocksById[selectedDatabaseBlockId] : undefined;
      return block?.type === "child_database" && !block.in_trash && !block.archived ? block : null;
    },
    isPagePendingRemoval: (pageId: string): boolean => pendingPageRemovalIds.has(pageId),
    get pageOpenMode(): NotesPageOpenMode {
      return pageSession.pageOpenMode;
    },
    get editorScrollTop(): number { return editorScrollTop; },
    rememberEditorScroll: (pageId: string, top: number): void => {
      if (pageId === pageSession.selectedPageId) editorScrollTop = top;
    },
    get loadedPage(): NotesPage | null {
      return treeProjection.loadedPage;
    },
    get primaryContentReady(): boolean {
      return treeProjection.primaryContentReady;
    },
    get pageCreationPending(): boolean {
      return pageCreationController.isPending(pageSession.selectedPageId);
    },
    get pageCreationError(): string | null {
      return pageCreationController.errorFor(pageSession.selectedPageId);
    },
    get pageBreadcrumbItems(): NotesPageBreadcrumbItem[] {
      return pageSession.breadcrumbs;
    },
    get backlinks() {
      return linksController.backlinks;
    },
    get backlinksLoading(): boolean {
      return linksController.backlinksLoading;
    },
    get backlinksError(): string | null {
      return linksController.backlinksError;
    },
    get pageAliases() {
      return linksController.aliases;
    },
    get pageAliasesLoading(): boolean {
      return linksController.aliasesLoading;
    },
    get pageAliasesError(): string | null {
      return linksController.aliasesError;
    },
    get unresolvedLinks() {
      return linksController.unresolvedLinks;
    },
    get unresolvedLinksLoading(): boolean {
      return linksController.unresolvedLoading;
    },
    get unresolvedLinksError(): string | null {
      return linksController.unresolvedError;
    },
    get linkResolutionPages(): NotesPage[] {
      return linksController.destinations;
    },
    get destinationHasMore(): boolean {
      return linksController.destinationHasMore;
    },
    get allCommentThreads() {
      return collaborationController.allCommentThreads;
    },
    get pageNotificationLevel() {
      return collaborationController.pageNotificationLevel;
    },
    get commentThreads() {
      return collaborationController.commentThreads;
    },
    get commentsLoading(): boolean {
      return collaborationController.commentsLoading;
    },
    get commentsError(): string | null {
      return collaborationController.commentsError;
    },
    get commentsIncludeResolved(): boolean {
      return collaborationController.commentsIncludeResolved;
    },
    get activeCommentParent() {
      return collaborationController.activeCommentParent;
    },
    get activeCommentAnchor() {
      return collaborationController.activeCommentAnchor;
    },
    get suggestions() {
      return collaborationController.suggestions;
    },
    get suggestionsLoading(): boolean {
      return collaborationController.suggestionsLoading;
    },
    get suggestionsError(): string | null {
      return collaborationController.suggestionsError;
    },
    get suggestionsIncludeDecided(): boolean {
      return collaborationController.suggestionsIncludeDecided;
    },
    get activeSuggestionDraft() {
      return collaborationController.activeSuggestionDraft;
    },
    get localUser() {
      return collaborationController.localUser;
    },
    get localUserLoading(): boolean {
      return collaborationController.localUserLoading;
    },
    get localUserError(): string | null {
      return collaborationController.localUserError;
    },
    get searchResults() {
      return searchController.results;
    },
    get searchHasMore(): boolean {
      return searchController.hasMore;
    },
    get searchLoading(): boolean {
      return searchController.loading;
    },
    get searchError(): string | null {
      return searchController.error;
    },
    get searchIncludeResolvedComments(): boolean {
      return searchController.includeResolvedComments;
    },
    get blocksById(): Record<string, NotesBlock> {
      return treeProjection.blocksById;
    },
    get childIdsByParentId(): Record<string, string[]> {
      return treeProjection.childIdsByParentId;
    },
    get flatBlockOutlines(): NotesBlockOutlineItem[] {
      return treeProjection.flatBlockOutlines;
    },
    get flatBlocks(): NotesBlockTreeItem[] {
      return flatBlockItems();
    },
    get loaded(): boolean {
      return workspaceLoaded;
    },
    /** Project whose Notes shell is loaded, or `null` for the unscoped workspace. */
    get workspaceProjectId(): string | null {
      return workspaceController.projectId;
    },
    get loading(): boolean {
      return workspaceController.loading;
    },
    get loadError(): string | null {
      return workspaceController.error;
    },
    get viewMode(): NotesViewMode {
      return viewMode;
    },
    get archiveLoaded(): boolean {
      return archiveController.archiveLoaded;
    },
    get archiveLoading(): boolean {
      return archiveController.archiveLoading;
    },
    get archiveError(): string | null {
      return archiveController.archiveError;
    },
    get trashLoaded(): boolean {
      return archiveController.trashLoaded;
    },
    get trashLoading(): boolean {
      return archiveController.trashLoading;
    },
    get trashError(): string | null {
      return archiveController.trashError;
    },
    /** Whether the hierarchy pickers list archived notes; one state shared by every picker. */
    get showArchivedInPickers(): boolean {
      return archiveController.showArchivedInPickers;
    },
    set showArchivedInPickers(shown: boolean) {
      archiveController.showArchivedInPickers = shown;
    },
    get pickerArchivedPages(): NotesPage[] {
      return archiveController.pickerArchivedPages;
    },
    get pickerArchiveLoading(): boolean {
      return archiveController.pickerArchiveLoading;
    },
    get pickerArchiveError(): string | null {
      return archiveController.pickerArchiveError;
    },
    reloadPickerArchivedPages: archiveController.reloadPickerArchivedPages,
    ensurePickerArchivedPages: archiveController.ensurePickerArchivedPages,
    get pageTemplatesLoading(): boolean {
      return pageTemplatesController.loading;
    },
    get pageTemplatesError(): string | null {
      return pageTemplatesController.error;
    },
    get pageHistorySnapshots(): NotesPageHistorySnapshot[] {
      return pageHistoryController.snapshots;
    },
    get pageHistorySnapshotsLoading(): boolean {
      return pageHistoryController.snapshotsLoading;
    },
    get pageHistorySnapshotsError(): string | null {
      return pageHistoryController.snapshotsError;
    },
    get pageHistoryVersion(): NotesLoadedPage | null {
      return pageHistoryController.version;
    },
    get pageHistoryVersionLoading(): boolean {
      return pageHistoryController.versionLoading;
    },
    get pageHistoryVersionError(): string | null {
      return pageHistoryController.versionError;
    },
    get pageHistorySettings(): NotesPageHistorySettings | null {
      return pageHistoryController.settings;
    },
    get pageHistorySettingsLoading(): boolean {
      return pageHistoryController.settingsLoading;
    },
    get pageHistorySettingsError(): string | null {
      return pageHistoryController.settingsError;
    },
    get pageHistoryActionLoading(): boolean {
      return pageHistoryController.actionLoading;
    },
    get pageHistoryActionError(): string | null {
      return pageHistoryController.actionError;
    },
    get focusBlockId(): string | null {
      return selectedDatabaseBlockId ? null : focusRequest.blockId;
    },
    get documentSelectionRestore() { return documentSelectionRestore; },
    get focusRequestId(): number {
      return focusRequest.requestId;
    },
    get focusPreventScroll(): boolean {
      return focusRequest.preventScroll ?? false;
    },
    get focusSelection(): NotesTextSelection | null {
      return focusRequest.selection;
    },
    /** Thread a followed discussion link or a block comment count asked this editor to reveal. */
    get discussionRequest(): NotesDiscussionRequest {
      return discussionRequest;
    },
    requestDiscussion,
    completeDiscussionRequest,
    get titleFocusPageId(): string | null {
      return pageSession.titleFocus.pageId;
    },
    get titleFocusRequestId(): number {
      return pageSession.titleFocus.requestId;
    },
    pageTitleDraftForPage,
    get canUndoNotesEdit(): boolean {
      return undoController.canUndo();
    },
    get canRedoNotesEdit(): boolean {
      return undoController.canRedo();
    },
    load: workspaceController.load,
    ensureLoaded: () => workspaceLoaded ? Promise.resolve() : workspaceController.ensureLoaded(),
    loadMoreWorkspaceWindow: workspaceController.loadMoreWorkspaceWindow,
    refreshProjectNavigation: workspaceController.refreshProjectNavigation,
    selectPage,
    openPageContextually,
    closeContextualPage,
    showSelectedPageAs,
    createPage: pageActions.createPage,
    createSubpage: pageActions.createSubpage,
    retryPageCreation: () => {
      const pageId = pageSession.selectedPageId;
      if (pageId) pageCreationController.retry(pageId);
    },
    createFolder: foldersController.createFolder,
    renameFolder: foldersController.renameFolder,
    moveFolder: foldersController.moveFolder,
    deleteFolder: foldersController.deleteFolder,
    importHtmlPage: transferActions.importHtmlPage,
    importNotionApi: transferActions.importNotionApi,
    importNotionExportFolder: transferActions.importNotionExportFolder,
    exportHtmlArchive: transferActions.exportHtmlArchive,
    exportJsonGraph: transferActions.exportJsonGraph,
    exportAgentBridge: transferActions.exportAgentBridge,
    createChildPageFromBlock: pageActions.createChildPageFromBlock,
    applyPageTemplate: pageTemplatesController.applyPageTemplate,
    createPageTemplateFromCurrentPage: pageTemplatesController.createPageTemplateFromCurrentPage,
    updatePageTemplateFromCurrentPage: pageTemplatesController.updatePageTemplateFromCurrentPage,
    renamePageTemplate: pageTemplatesController.renamePageTemplate,
    duplicatePageTemplate: pageTemplatesController.duplicatePageTemplate,
    deletePageTemplate: pageTemplatesController.deletePageTemplate,
    reloadPageTemplates: pageTemplatesController.reloadPageTemplates,
    ensureOptionalSubsystem,
    setPagePanelSubsystemOpen,
    loadLocalUser: collaborationController.loadLocalUser,
    updateLocalUserDisplayName: collaborationController.updateLocalUserDisplayName,
    loadPageHistorySettings: pageHistoryController.loadSettings,
    updatePageHistoryRetention: pageHistoryController.updateRetention,
    reloadPageHistory: pageHistoryController.reloadSnapshots,
    loadPageHistoryVersion: pageHistoryController.loadVersion,
    restorePageHistoryVersion: pageHistoryController.restoreVersion,
    copyPageHistoryBlocks: pageHistoryController.copyBlocks,
    renamePage: pageActions.renamePage,
    duplicatePage: pageActions.duplicatePage,
    movePage: pageActions.movePage,
    movePageToFolder: pageActions.movePageToFolder,
    archivePage: pageActions.archivePage,
    unarchivePage: pageActions.unarchivePage,
    trashPage: pageActions.trashPage,
    restorePage: pageActions.restorePage,
    permanentlyDeletePage: pageActions.permanentlyDeletePage,
    openArchive,
    closeArchive,
    reloadArchivedPages,
    loadMoreArchivedPages,
    openTrash,
    closeTrash,
    reloadTrashedPages,
    loadMoreTrashedPages,
    reloadBacklinks: linksController.reloadBacklinks,
    reloadPageAliases: linksController.reloadPageAliases,
    reloadUnresolvedLinks: linksController.reloadUnresolvedLinks,
    reloadLinkResolutionPages: linksController.reloadLinkResolutionPages,
    loadMoreDestinationCandidates: linksController.loadMoreDestinationCandidates,
    addPageAlias: linksController.addPageAlias,
    deletePageAlias: linksController.deletePageAlias,
    resolveUnresolvedLink: linksController.resolveUnresolvedLink,
    reloadComments: collaborationController.reloadComments,
    reloadSuggestions: collaborationController.reloadSuggestions,
    setCommentsIncludeResolved: collaborationController.setCommentsIncludeResolved,
    setSuggestionsIncludeDecided: collaborationController.setSuggestionsIncludeDecided,
    setActiveCommentParent: collaborationController.setActiveCommentParent,
    startBlockComment: collaborationController.startBlockComment,
    startInlineComment: collaborationController.startInlineComment,
    startInlineSuggestion: collaborationController.startInlineSuggestion,
    cancelSuggestionDraft: collaborationController.cancelSuggestionDraft,
    createSuggestion: collaborationController.createSuggestion,
    acceptSuggestion: collaborationController.acceptSuggestion,
    rejectSuggestion: collaborationController.rejectSuggestion,
    createComment: collaborationController.createComment,
    replyToCommentThread: collaborationController.replyToCommentThread,
    updateComment: collaborationController.updateComment,
    deleteComment: collaborationController.deleteComment,
    toggleCommentReaction: collaborationController.toggleCommentReaction,
    setCommentThreadMuted: collaborationController.setCommentThreadMuted,
    setPageNotificationLevel: collaborationController.setPageNotificationLevel,
    setCommentThreadResolved: collaborationController.setCommentThreadResolved,
    markCommentThreadsRead: collaborationController.markCommentThreadsRead,
    markVisibleCommentThreadsRead: collaborationController.markVisibleCommentThreadsRead,
    search: searchController.search,
    loadMoreSearchResults: searchController.loadMoreSearchResults,
    setSearchIncludeResolvedComments: searchController.setIncludeResolvedComments,
    notesCommentParentKey: collaborationController.notesCommentParentKey,
    blockById,
    tableRowsForBlock,
    columnItemsForBlock,
    tabItemsForBlock,
    parentIdForBlock,
    previousBlockType,
    isOnlyBlock,
    focusBlock,
    hydrateBlockRange,
    outlineSubtreeIds,
    setPageFavorited,
    setFolderFavorited,
    sidebarOrderOverride,
    sidebarOrderForProject,
    setProjectSidebarOrder,
    customOrderForProject,
    setCustomSiblingOrder,
    setFolderCollapsed,
    loadNavigationChildren,
    updatePageIcon: pageActions.updatePageIcon,
    updatePageCover: pageActions.updatePageCover,
    openBlockLink,
    openNotesLink,
    openDatabase,
    closeDatabase,
    undoNotesEdit,
    redoNotesEdit,
    setPageTitleDraft,
    clearPageTitleDraft,
    reconcileDatabaseTitle,
    updateBlockText, updateBlockRichText,
    insertPageMention,
    insertDateMention,
    insertObjectMention,
    insertInlineEquation,
    updateBlockTextLink,
    updateBlockTextAnnotations,
    flushBlockSave,
    flushPendingWrites,
    convertBlock,
    databasePaste: blockActions.databasePaste,
    databaseDeletion,
    isDatabaseCreationPending: blockActions.isDatabaseCreationPending,
    toggleTodo,
    updateCodeLanguage,
    updateBlockColor,
    updateCalloutIcon,
    updateToggleOpen,
    convertBlockToToggleHeading,
    createSiblingAfter,
    ensurePageBody: blockActions.ensurePageBody,
    splitTextBlockAtSelection,
    insertParagraphAdjacent,
    pastePlainTextIntoBlock,
    pasteRichHtmlIntoBlock,
    pasteBlockSelection,
    deleteBlock,
    formatDocumentRange: blockActions.formatDocumentRange,
    linkDocumentRange: blockActions.linkDocumentRange,
    replaceDocumentRange,
    deleteBlockSelection,
    mergeBlockWithPrevious,
    indentBlockSelection,
    nestBlock,
    outdentBlock,
    moveBlockUp,
    moveBlockDown,
    moveBlockSelection,
    dropBlockWithinSiblings,
    dropBlockOnBlock,
    moveBlockToPage,
    duplicateBlock,
    duplicateBlockSelection,
    addTemplateChild,
    useTemplateBlock,
    addButtonChild,
    updateButtonIcon,
    updateButtonInsertPosition,
    useButtonBlock,
    createLinkedDatabaseViewAfter,
    convertUnsupportedBlock,
    blockPlainText,
    updateBookmark,
    updateLinkPreviewUrl,
    updateEmbedUrl,
    updateEquationExpression,
    updateMedia,
    updateTableCell,
    updateTableCellRichText,
    addTableRow,
    removeTableRow,
    addTableColumn,
    removeTableColumn,
    addColumn,
    removeColumn,
    moveColumn,
    resizeColumn,
    moveBlockToColumn,
    updateTabLabel,
    updateTabIcon,
    addTab,
    removeTab,
    moveTab,
    moveBlockToTab,
  };
  sessions.add(facade);
  return facade;
}

export type NotesEditorStore = ReturnType<typeof createNotesEditorStore>;
