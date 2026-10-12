import { loadNotesWorkspaceShell } from "$lib/api/notes";
import type { NotesWorkspaceShell } from "$lib/notes/types";

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
  applyInitialShell: (
    shell: NotesWorkspaceShell,
    requestedSelection: string | null,
  ) => string | null;
  applyAdditionalShell: (shell: NotesWorkspaceShell) => void;
  mergeReloadedShell: (shell: NotesWorkspaceShell) => void;
  loadSelectedPage: (pageId: string) => Promise<void>;
  clearSelectedPageState: () => void;
  readSelectedPageId: () => string | null;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Own Notes workspace shell loading, pagination, and request identity. */
export function createNotesWorkspaceController(context: NotesWorkspaceControllerContext) {
  let loaded = $state(false);
  let loading = $state(false);
  let error = $state<string | null>(null);
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

  async function load(): Promise<void> {
    const currentRequestId = ++requestId;
    context.prepareLoad();
    const state = context.readRequestState();
    const requestedSelection = state.selectedPageId;
    loading = true;
    error = null;
    try {
      const shell = await loadNotesWorkspaceShell(requestFromState(state));
      if (currentRequestId !== requestId) return;
      nextPageCursor = shell.next_page_cursor;
      nextFolderCursor = shell.next_folder_cursor;
      totalPageCount = shell.total_page_count;
      totalFolderCount = shell.total_folder_count;
      const nextSelected = context.applyInitialShell(shell, requestedSelection);
      loaded = true;
      if (nextSelected) {
        await context.loadSelectedPage(nextSelected).catch((caught) => {
          if (context.readSelectedPageId() === nextSelected) error = errorMessage(caught);
        });
      } else {
        context.clearSelectedPageState();
      }
    } catch (caught) {
      if (currentRequestId !== requestId) return;
      error = errorMessage(caught);
      throw caught;
    } finally {
      if (currentRequestId === requestId) loading = false;
    }
  }

  async function ensureLoaded(): Promise<void> {
    if (loadPromise) return loadPromise;
    if (loaded) return;
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
      context.mergeReloadedShell(shell);
      return;
    }
  }

  /** Replace project-scoped pagination and refresh metadata without resetting the editor. */
  async function refreshProjectNavigation(): Promise<void> {
    requestId += 1;
    loading = false;
    windowLoading = false;
    nextPageCursor = null;
    nextFolderCursor = null;
    await reloadPages();
  }

  return {
    get loaded(): boolean { return loaded; },
    get loading(): boolean { return loading; },
    get error(): string | null { return error; },
    get windowLoading(): boolean { return windowLoading; },
    get totalPageCount(): number { return totalPageCount; },
    get totalFolderCount(): number { return totalFolderCount; },
    load,
    ensureLoaded,
    loadMoreWorkspaceWindow,
    reloadPages,
    refreshProjectNavigation,
    setLoading(value: boolean): void { loading = value; },
    setError(value: string | null): void { error = value; },
  };
}
