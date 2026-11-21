import { notesErrorDetail } from "$lib/notes/failure";
import { loadNotesWorkspaceShell } from "$lib/api/notes";
import type { NotesWorkspaceShell } from "$lib/notes/types";
import {
  IDLE_PAGE_LOAD,
  notesShellApplied,
  notesWorkspaceError,
  notesWorkspaceLoading,
  transitionPageLoad,
  transitionShellLoad,
  UNLOADED_SHELL,
  type NotesPageLoad,
  type NotesPageLoadEvent,
  type NotesShellLoad,
  type NotesShellLoadEvent,
} from "./workspace-state";

interface NotesWorkspaceShellRequestState {
  projectId: string | null;
  expandedPageIds: string[];
  seedPageIds: string[];
  selectedPageId: string | null;
}

interface NotesWorkspaceControllerContext {
  readRequestState: () => NotesWorkspaceShellRequestState;
  readNavigationMutationRevision: () => number;
  prepareLoad: () => void;
  /** Apply a freshly loaded shell for `projectId`, or the unscoped workspace when `null`, and return the page to open. */
  applyInitialShell: (
    shell: NotesWorkspaceShell,
    requestedSelection: string | null,
    projectId: string | null,
  ) => string | null;
  applyAdditionalShell: (shell: NotesWorkspaceShell) => void;
  mergeReloadedShell: (shell: NotesWorkspaceShell, projectId: string | null) => void;
  loadSelectedPage: (pageId: string) => Promise<void>;
  clearSelectedPageState: () => void;
  readSelectedPageId: () => string | null;
}

/** Own Notes workspace shell loading, pagination, request identity, and the selected page's load state. */
export function createNotesWorkspaceController(context: NotesWorkspaceControllerContext) {
  let shellLoad = $state<NotesShellLoad>(UNLOADED_SHELL);
  let pageLoad = $state<NotesPageLoad>(IDLE_PAGE_LOAD);
  let windowLoading = $state(false);
  let nextPageCursor = $state<string | null>(null);
  let nextFolderCursor = $state<string | null>(null);
  let totalPageCount = 0;
  let totalFolderCount = 0;
  let requestId = 0;
  let loadPromise: Promise<void> | null = null;

  function requestFromState(state: NotesWorkspaceShellRequestState) {
    return {
      project_id: state.projectId,
      expanded_page_ids: state.expandedPageIds,
      seed_page_ids: state.seedPageIds,
      selected_page_id: state.selectedPageId,
      include_navigation_index: true,
    };
  }

  function dispatchShell(event: NotesShellLoadEvent): void {
    shellLoad = transitionShellLoad(shellLoad, event);
  }

  function dispatchPage(event: NotesPageLoadEvent): void {
    pageLoad = transitionPageLoad(pageLoad, event);
  }

  async function load(): Promise<void> {
    const currentRequestId = ++requestId;
    context.prepareLoad();
    const state = context.readRequestState();
    const requestedSelection = state.selectedPageId;
    dispatchShell({ type: "started" });
    try {
      const shell = await loadNotesWorkspaceShell(requestFromState(state));
      if (currentRequestId !== requestId) return;
      nextPageCursor = shell.next_page_cursor;
      nextFolderCursor = shell.next_folder_cursor;
      totalPageCount = shell.total_page_count;
      totalFolderCount = shell.total_folder_count;
      const nextSelected = context.applyInitialShell(shell, requestedSelection, state.projectId);
      dispatchShell({ type: "applied" });
      if (nextSelected) {
        dispatchPage({ type: "started", pageId: nextSelected });
        await context.loadSelectedPage(nextSelected).then(() => {
          dispatchPage({ type: "settled", pageId: nextSelected });
        }, (caught: unknown) => {
          if (context.readSelectedPageId() === nextSelected) {
            dispatchPage({ type: "failed", pageId: nextSelected, error: notesErrorDetail(caught) });
          }
        });
      } else {
        context.clearSelectedPageState();
        dispatchPage({ type: "cleared" });
      }
    } catch (caught) {
      if (currentRequestId !== requestId) return;
      dispatchShell({ type: "failed", error: notesErrorDetail(caught) });
      throw caught;
    }
  }

  async function ensureLoaded(): Promise<void> {
    if (loadPromise) return loadPromise;
    if (notesShellApplied(shellLoad)) return;
    loadPromise = load().finally(() => {
      loadPromise = null;
    });
    return loadPromise;
  }

  async function loadMoreWorkspaceWindow(): Promise<void> {
    if (windowLoading || (!nextPageCursor && !nextFolderCursor)) return;
    const currentRequestId = requestId;
    const mutationRevision = context.readNavigationMutationRevision();
    const state = context.readRequestState();
    const pageCursor = nextPageCursor;
    const folderCursor = nextFolderCursor;
    windowLoading = true;
    try {
      const shell = await loadNotesWorkspaceShell({
        ...requestFromState({
          ...state,
          expandedPageIds: [],
          seedPageIds: [],
        }),
        include_navigation_index: false,
        page_cursor: pageCursor ?? "end",
        folder_cursor: folderCursor ?? "end",
      });
      if (currentRequestId !== requestId) return;
      if (mutationRevision !== context.readNavigationMutationRevision()) {
        await reloadPages();
        return;
      }
      context.applyAdditionalShell(shell);
      nextPageCursor = shell.next_page_cursor;
      nextFolderCursor = shell.next_folder_cursor;
    } finally {
      if (currentRequestId === requestId) windowLoading = false;
    }
  }

  async function reloadPages(
    selectedPageIdOverride = context.readSelectedPageId(),
    expandedPageIdsOverride?: readonly string[],
  ): Promise<void> {
    for (;;) {
      const state = context.readRequestState();
      const currentRequestId = requestId;
      const mutationRevision = context.readNavigationMutationRevision();
      const shell = await loadNotesWorkspaceShell({
        ...requestFromState({
          ...state,
          selectedPageId: selectedPageIdOverride,
          expandedPageIds: expandedPageIdsOverride
            ? [...expandedPageIdsOverride]
            : state.expandedPageIds,
        }),
        include_navigation_index: false,
      });
      if (currentRequestId !== requestId || state.projectId !== context.readRequestState().projectId) return;
      if (mutationRevision !== context.readNavigationMutationRevision()) continue;
      nextPageCursor = shell.next_page_cursor;
      nextFolderCursor = shell.next_folder_cursor;
      totalPageCount = shell.total_page_count;
      totalFolderCount = shell.total_folder_count;
      context.mergeReloadedShell(shell, state.projectId);
      return;
    }
  }

  /** Replace project-scoped pagination and refresh metadata without resetting the editor. */
  async function refreshProjectNavigation(): Promise<void> {
    requestId += 1;
    dispatchShell({ type: "superseded" });
    windowLoading = false;
    nextPageCursor = null;
    nextFolderCursor = null;
    await reloadPages();
  }

  return {
    /** Whether a shell has been applied, even if a later reload failed. */
    get loaded(): boolean { return notesShellApplied(shellLoad); },
    get loading(): boolean { return notesWorkspaceLoading(shellLoad, pageLoad); },
    /** A failed shell load, or else a failure of the selected page. */
    get error(): string | null { return notesWorkspaceError(shellLoad, pageLoad, context.readSelectedPageId()); },
    get windowLoading(): boolean { return windowLoading; },
    get totalPageCount(): number { return totalPageCount; },
    get totalFolderCount(): number { return totalFolderCount; },
    load,
    ensureLoaded,
    loadMoreWorkspaceWindow,
    reloadPages,
    refreshProjectNavigation,
    /** Moves the selected page's load state; failures of its outlines, history, and undo are page failures too. */
    dispatchPageLoad: dispatchPage,
  };
}
