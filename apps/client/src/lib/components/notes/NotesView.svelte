<script lang="ts">
  import { onMount, untrack, type Snippet } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { hasOnlyShortcutModifier } from "$lib/keyboard-shortcuts";
  import {
    beginLazyComponentLoad,
    rejectLazyComponentLoad,
    resolveLazyComponentLoad,
    type LazyComponentLoadState,
  } from "$lib/lazy-component-loader";
  import { parseNotesLinkHash } from "$lib/notes/links/block-link";
  import { notesPageContainingFolderId } from "$lib/notes/navigation/hierarchy-menu";
  import { firstNotesNavigationPage, notesFoldersForProject } from "$lib/notes/navigation/tree";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import { notesPageProjectId, notesPagesForProject } from "$lib/notes/project-membership";
  import { notesUndoShortcutAction } from "$lib/notes/history/undo-history";
  import type { NotesWorkingMarkdownFileRef } from "$lib/notes/types";
  import { getNotes } from "$lib/stores/notes.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import { isAppShortcutBlockedTarget, isEditableKeyboardTarget } from "$lib/utils";
  import {
    loadNotesOptionalComponent,
    loadNotesSurface,
    retryNotesOptionalComponent,
    retryNotesSurface,
    type LoadedNotesOptionalComponent,
    type LoadedNotesSurface,
    type NotesOptionalComponentKind,
    type NotesSurfaceKind,
  } from "./component-registry";
  import NotesEditor from "./NotesEditor.svelte";
  import NotesDatabasePage from "$lib/components/notes/database/NotesDatabasePage.svelte";
  import NotesLoadingSkeleton from "./NotesLoadingSkeleton.svelte";
  import NotesProjectEmptyState from "./NotesProjectEmptyState.svelte";
  import NotesProjectHome from "./NotesProjectHome.svelte";
  import NotesProjectSettingsPanel from "$lib/components/notes/NotesProjectSettingsPanel.svelte";
  import NotesWorkspaceHeader from "./NotesWorkspaceHeader.svelte";
  import NotesWorkingMarkdownEditor from "$lib/components/notes/working-markdown/NotesWorkingMarkdownEditor.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  let {
    mobileLayout = false,
    projectSidebar = undefined,
  }: {
    mobileLayout?: boolean;
    /**
     * Projects sidebar from a host that opens Notes as one place of a project. It replaces the Notes explorer, so the
     * header breadcrumb carries page navigation. The snippet receives the handler for a project switch.
     */
    projectSidebar?: Snippet<[() => void]>;
  } = $props();

  const notes = getNotes();
  const projects = getProjects();
  const mobileBackStack = getMobileBackStack();
  const viewport = getViewport();
  const { t } = getLocalization();

  const CENTER_PEEK_FULL_PAGE_MIN_WIDTH_PX = 608;
  const CENTER_PEEK_FULL_PAGE_MIN_HEIGHT_PX = 520;
  type ActiveNotesSurfaceKind = NotesSurfaceKind | "home" | "editor";

  let creationFolderOverride = $state<string | null | undefined>(undefined);
  let projectSelectionInFlight: { projectId: string | null } | null = null;
  /** Project hosted Notes last tried to align with, so a failed load is not retried in a loop. */
  let hostedAlignmentProjectId: string | null | undefined;
  let creationContextProjectId = $state<string | null>(null);
  let creationContextPageId = $state<string | null>(null);
  let notesRootElement = $state<HTMLDivElement | null>(null);
  let pageActionsTarget = $state<HTMLElement | null>(null);
  let projectSettingsOpen = $state(false);
  /** The phone shell opens the explorer as a floating panel over an open note; the landing level shows it in place. */
  let mobileExplorerPanelOpen = $state(false);
  let projectSettingsDirty = $state(false);
  let projectSettingsDiscardConfirmOpen = $state(false);
  let projectVersionHistoryOpen = $state(false);
  let pendingProjectSettingsAction: (() => void) | null = null;
  let activeNotesHistoryShortcut: "undo" | "redo" | null = null;
  let selectedWorkingMarkdownFile = $state<NotesWorkingMarkdownFileRef | null>(null);
  let workingMarkdownDirty = $state(false);
  let workingMarkdownProjectId = $state<string | null>(null);
  let retainedEditorPages = $state<Record<string, string>>({});
  let surfaceLoadStates = $state<Partial<Record<
    NotesSurfaceKind,
    LazyComponentLoadState<NotesSurfaceKind, LoadedNotesSurface>
  >>>({});
  let optionalLoadStates = $state<Partial<Record<
    NotesOptionalComponentKind,
    LazyComponentLoadState<NotesOptionalComponentKind, LoadedNotesOptionalComponent>
  >>>({});
  let databaseDeletionDialogLoadState = $state<LazyComponentLoadState<
    "database-deletion",
    typeof import("$lib/components/notes/database/NotesDatabaseDeletionDialog.svelte").default
  > | null>(null);
  const selectedProject = $derived(projects.selectedProject);
  const selectedGroup = $derived(projects.selectedGroup);
  const selectedProjectId = $derived(selectedProject?.id ?? null);
  const creationFolderId = $derived.by(() => {
    if (creationFolderOverride !== undefined) return creationFolderOverride;
    return notesPageContainingFolderId(
      notes.selectedPageId,
      notes.navigationPages,
    );
  });
  const topBarSelectedPage = $derived.by(() => {
    if (!notes.selectedPageId) return null;
    if (notes.loadedPage?.id === notes.selectedPageId) return notes.loadedPage;
    return notes.allPages.find((page) => page.id === notes.selectedPageId)
      ?? notes.linkResolutionPages.find((page) => page.id === notes.selectedPageId)
      ?? null;
  });
  const showDatabasePage = $derived(notes.viewMode === "pages" && notes.selectedDatabaseBlockId != null);
  const contextualPane = $derived(notes.previewPane
    ?? notes.editorPanes.find((pane) => pane.store.pageOpenMode !== "full") ?? null);
  const hasContextualSelection = $derived(
    notes.viewMode === "pages" && contextualPane?.store.selectedPageId != null,
  );
  const peekPromotesToFullPage = $derived(
    hasContextualSelection && (
      viewport.width < CENTER_PEEK_FULL_PAGE_MIN_WIDTH_PX
      || viewport.height < CENTER_PEEK_FULL_PAGE_MIN_HEIGHT_PX
    ),
  );
  const showPagePeek = $derived(
    hasContextualSelection && !peekPromotesToFullPage,
  );
  const showCenterPeek = $derived(showPagePeek && contextualPane?.store.pageOpenMode === "center");
  const showSidePeek = $derived(showPagePeek && contextualPane?.store.pageOpenMode === "side");
  const activeSurfaceKind = $derived.by((): ActiveNotesSurfaceKind => {
    if (notes.viewMode === "archive") return "archive";
    if (notes.viewMode === "trash") return "trash";
    if (notes.selectedPageId !== null
      && !notes.isPagePendingRemoval(notes.selectedPageId)) return "editor";
    return "home";
  });
  const activeSurfaceLoadState = $derived(
    activeSurfaceKind === "archive" || activeSurfaceKind === "trash"
      ? surfaceLoadStates[activeSurfaceKind] ?? null
      : null,
  );
  const showProjectExplorer = $derived(!mobileLayout || activeSurfaceKind === "home");
  const showPrimaryContent = $derived(!mobileLayout || activeSurfaceKind !== "home");
  const showMobileExplorerPanel = $derived(mobileLayout && mobileExplorerPanelOpen && !showProjectExplorer);
  const showEmptyProjectState = $derived(
    !mobileLayout
    && notes.loaded
    && activeSurfaceKind === "home"
    && !hasContextualSelection
    && selectedProjectId !== null
    && notes.workspaceProjectId === selectedProjectId
    && notesPagesForProject(notes.navigationPages, selectedProjectId).length === 0,
  );
  const projectHistoryLoadState = $derived(optionalLoadStates["project-history"] ?? null);
  const confirmDialogLoadState = $derived(optionalLoadStates["confirm-dialog"] ?? null);
  const databaseDeletionPanes = $derived(notes.editorPanes.filter(({ store }) => store.databaseDeletion.prompt));

  $effect(() => {
    const previous = untrack(() => retainedEditorPages);
    const retained = Object.fromEntries(notes.editorPanes.flatMap(({ id, store }) => {
      const pageId = store.selectedPageId;
      if (!pageId) return [];
      if (!store.selectedDatabaseBlockId && store.loadedPage?.id === pageId && store.primaryContentReady) {
        return [[id, pageId]];
      }
      return previous[id] === pageId ? [[id, pageId]] : [];
    }));
    if (Object.keys(retained).length !== Object.keys(previous).length
      || Object.entries(retained).some(([id, pageId]) => previous[id] !== pageId)) {
      retainedEditorPages = retained;
    }
  });

  function requestNotesSurface(kind: NotesSurfaceKind, retry = false): void {
    const current = surfaceLoadStates[kind] ?? null;
    if (!retry && current?.key === kind) return;
    const loadingState = beginLazyComponentLoad(current, kind);
    surfaceLoadStates = { ...surfaceLoadStates, [kind]: loadingState };
    const request = retry ? retryNotesSurface(kind) : loadNotesSurface(kind);
    void request.then((component) => {
      const latest = surfaceLoadStates[kind];
      if (!latest) return;
      surfaceLoadStates = {
        ...surfaceLoadStates,
        [kind]: resolveLazyComponentLoad(latest, kind, loadingState.requestId, component),
      };
    }).catch((error: unknown) => {
      const latest = surfaceLoadStates[kind];
      if (!latest) return;
      surfaceLoadStates = {
        ...surfaceLoadStates,
        [kind]: rejectLazyComponentLoad(latest, kind, loadingState.requestId, error),
      };
      console.error(`load Notes ${kind} surface failed`, error);
    });
  }

  function requestNotesOptionalComponent(
    kind: NotesOptionalComponentKind,
    retry = false,
  ): void {
    const current = optionalLoadStates[kind] ?? null;
    if (!retry && current?.key === kind) return;
    const loadingState = beginLazyComponentLoad(current, kind);
    optionalLoadStates = { ...optionalLoadStates, [kind]: loadingState };
    const request = retry
      ? retryNotesOptionalComponent(kind)
      : loadNotesOptionalComponent(kind);
    void request.then((component) => {
      const latest = optionalLoadStates[kind];
      if (!latest) return;
      optionalLoadStates = {
        ...optionalLoadStates,
        [kind]: resolveLazyComponentLoad(latest, kind, loadingState.requestId, component),
      };
    }).catch((error: unknown) => {
      const latest = optionalLoadStates[kind];
      if (!latest) return;
      optionalLoadStates = {
        ...optionalLoadStates,
        [kind]: rejectLazyComponentLoad(latest, kind, loadingState.requestId, error),
      };
      console.error(`load optional Notes ${kind} component failed`, error);
    });
  }

  /** Load the shared database confirmation only after a pane requests deletion. */
  function requestDatabaseDeletionDialog(retry = false): void {
    if (!retry && databaseDeletionDialogLoadState) return;
    const loading = beginLazyComponentLoad(databaseDeletionDialogLoadState, "database-deletion");
    databaseDeletionDialogLoadState = loading;
    void import("$lib/components/notes/database/NotesDatabaseDeletionDialog.svelte").then((module) => {
      if (!databaseDeletionDialogLoadState) return;
      databaseDeletionDialogLoadState = resolveLazyComponentLoad(
        databaseDeletionDialogLoadState, "database-deletion", loading.requestId, module.default,
      );
    }).catch((error: unknown) => {
      if (!databaseDeletionDialogLoadState) return;
      databaseDeletionDialogLoadState = rejectLazyComponentLoad(
        databaseDeletionDialogLoadState, "database-deletion", loading.requestId, error,
      );
      console.error("Load Notes database deletion confirmation failed", error);
    });
  }

  $effect(() => {
    if (databaseDeletionPanes.length) untrack(() => requestDatabaseDeletionDialog());
  });

  $effect(() => {
    if (showProjectExplorer) mobileExplorerPanelOpen = false;
  });

  $effect(() => {
    if (!mobileLayout || notes.selectedPageId === null) return;
    return mobileBackStack.activate({
      handle: () => {
        if (!beforeDocumentNavigation()) return;
        if (notes.selectedDatabaseBlockId) {
          notes.closeDatabase();
          return;
        }
        closePagePeek();
      },
    });
  });

  $effect(() => {
    if (!mobileLayout || notes.viewMode !== "archive") return;
    return mobileBackStack.activate({
      handle: () => {
        notes.closeArchive();
      },
    });
  });

  $effect(() => {
    if (!mobileLayout || notes.viewMode !== "trash") return;
    return mobileBackStack.activate({
      handle: () => {
        notes.closeTrash();
      },
    });
  });

  onMount(() => {
    async function openHashTarget(): Promise<void> {
      const target = parseNotesLinkHash(window.location.hash);
      if (!target) {
        return;
      }
      const opened = await notes.openNotesLink(target);
      if (!opened) console.warn("notes link target was not found");
    }

    void notes.ensureLoaded()
      .then(openHashTarget)
      .catch((error) => {
        console.error("load notes failed", error);
      })
      ;
    void projects.ensureLoaded().catch((error) => {
      console.error("load projects failed", error);
    });
    const handleHashChange = () => {
      void openHashTarget().catch((error) => {
        console.error("open notes block link failed", error);
      });
    };
    window.addEventListener("hashchange", handleHashChange);
    return () => {
      window.removeEventListener("hashchange", handleHashChange);
    };
  });

  $effect(() => {
    const projectId = selectedProjectId;
    const pageId = notes.selectedPageId;
    if (projectId === creationContextProjectId && pageId === creationContextPageId) return;
    creationContextProjectId = projectId;
    creationContextPageId = pageId;
    creationFolderOverride = undefined;
  });

  $effect(() => {
    if (activeSurfaceKind === "archive" || activeSurfaceKind === "trash") {
      requestNotesSurface(activeSurfaceKind);
    }
  });

  $effect(() => {
    if (!mobileLayout && projectVersionHistoryOpen) {
      requestNotesOptionalComponent("project-history");
    }
  });

  $effect(() => {
    if (!mobileLayout && projectSettingsDiscardConfirmOpen) {
      requestNotesOptionalComponent("confirm-dialog");
    }
  });

  function showProjectHome(): Promise<void> {
    if (!beforeDocumentNavigation()) return Promise.resolve();
    if (notes.viewMode === "archive") notes.closeArchive();
    if (notes.viewMode === "trash") notes.closeTrash();
    return notes.selectPage(null);
  }

  function handleProjectSelected(): void {
    if (!beforeDocumentNavigation()) {
      if (workingMarkdownProjectId) projects.selectedProjectId = workingMarkdownProjectId;
      return;
    }
    const projectId = projects.selectedProjectId;
    if (projectSelectionInFlight && projectSelectionInFlight.projectId === projectId) return;
    const selection = { projectId };
    projectSelectionInFlight = selection;
    hostedAlignmentProjectId = projectId;
    pendingProjectSettingsAction = null;
    projectSettingsOpen = false;
    projectSettingsDirty = false;
    projectSettingsDiscardConfirmOpen = false;
    projectVersionHistoryOpen = false;
    // The shell request carries the selected page, so the previous project's page must be cleared before loading.
    void showProjectHome()
      .then(() => notes.load())
      .then(() => openFirstProjectNote(projectId))
      .catch((error) => {
        console.error("load selected Notes project failed", error);
      })
      .finally(() => {
        if (projectSelectionInFlight === selection) projectSelectionInFlight = null;
      });
  }

  /**
   * Whether hosted Notes shows the selected project: the open note belongs to it, or the project home or list is
   * loaded for it. A note of the selected project counts even before its shell refresh finishes, and `null` means the
   * main pane is still loading.
   */
  function hostedNotesShowProject(projectId: string): boolean | null {
    const main = notes.editorPanes.find((pane) => pane.id === notes.mainPaneId)?.store;
    if (!main || !main.loaded || main.loading) return null;
    if (main.selectedPageId) {
      const page = main.loadedPage;
      if (page?.id !== main.selectedPageId) return null;
      if (notesPageProjectId(page) === projectId) return true;
    }
    return !main.selectedPageId && main.workspaceProjectId === projectId;
  }

  // Beside the Projects sidebar the selected project leads, so a note or shell left from another project, such as the
  // Notes tab's last document, is replaced by the selected project's notes instead of pulling the project back to it.
  // Only the main pane counts, so a peek at another project's note stays open, and unsaved working Markdown is kept.
  $effect(() => {
    notes.setPreviewsFollowProject(!projectSidebar);
    return () => notes.setPreviewsFollowProject(true);
  });

  $effect(() => {
    if (!projectSidebar || workingMarkdownDirty) return;
    const projectId = projects.selectedProjectId;
    if (!projectId) return;
    const aligned = hostedNotesShowProject(projectId);
    if (aligned === null) return;
    if (aligned) {
      hostedAlignmentProjectId = undefined;
      return;
    }
    if (hostedAlignmentProjectId === projectId) return;
    hostedAlignmentProjectId = projectId;
    untrack(handleProjectSelected);
  });

  /**
   * Opens the first note of a newly selected project in sidebar order, unless the project changed or the user opened
   * something else while it loaded. Mobile keeps the project's note list, which is its navigation surface.
   */
  async function openFirstProjectNote(projectId: string | null): Promise<void> {
    if (mobileLayout || !projectId || projects.selectedProjectId !== projectId) return;
    if (notes.viewMode !== "pages" || notes.selectedPageId !== null || selectedWorkingMarkdownFile) return;
    const firstPage = firstNotesNavigationPage(
      notesPagesForProject(notes.navigationPages, projectId),
      notesFoldersForProject(notes.folders, projectId),
      {
        rootPagesOnly: true,
        collapsedFolderIds: notes.collapsedFolderIds,
        pageIdsWithChildren: notes.sidebarPageIdsWithChildren,
        missingParentPageIds: notes.sidebarMissingParentPageIds,
        trashedParentPageIds: notes.sidebarTrashedParentPageIds,
        sortOrder: notes.sidebarOrderForProject(projectId),
        favoriteFolderIds: notes.favoriteFolderIds,
        favoritePageIds: notes.favoritePageIds,
        customOrderKeys: notes.customOrderForProject(projectId),
        titleForPage: (page) => notesPageTitle(page, t("notes.untitled")),
      },
    );
    if (firstPage) await notes.selectPage(firstPage.id, { openMode: "full" });
  }

  function beforeDocumentNavigation(): boolean {
    if (!workingMarkdownDirty) {
      selectedWorkingMarkdownFile = null;
      workingMarkdownProjectId = null;
      return true;
    }
    if (!window.confirm(t("notes.workingMarkdown.discardConfirm"))) return false;
    workingMarkdownDirty = false;
    selectedWorkingMarkdownFile = null;
    workingMarkdownProjectId = null;
    return true;
  }

  function selectWorkingMarkdownFile(file: NotesWorkingMarkdownFileRef): void {
    if (mobileLayout) return;
    const unchanged = selectedWorkingMarkdownFile?.workingFolderId === file.workingFolderId
      && selectedWorkingMarkdownFile.relativePath === file.relativePath;
    if (unchanged) return;
    if (!beforeDocumentNavigation()) return;
    if (notes.viewMode === "archive") notes.closeArchive();
    if (notes.viewMode === "trash") notes.closeTrash();
    void notes.selectPage(null);
    selectedWorkingMarkdownFile = file;
    workingMarkdownProjectId = selectedProjectId;
  }

  function closeProjectSettingsImmediately(): void {
    pendingProjectSettingsAction = null;
    projectSettingsDiscardConfirmOpen = false;
    projectSettingsOpen = false;
    projectSettingsDirty = false;
  }

  function runAfterProjectSettingsClose(action: () => void): void {
    if (projectSettingsDirty) {
      pendingProjectSettingsAction = action;
      projectSettingsDiscardConfirmOpen = true;
      return;
    }
    action();
  }

  function requestProjectSettingsClose(): void {
    runAfterProjectSettingsClose(closeProjectSettingsImmediately);
  }

  function confirmDiscardProjectSettings(): void {
    const action = pendingProjectSettingsAction;
    pendingProjectSettingsAction = null;
    projectSettingsDiscardConfirmOpen = false;
    projectSettingsDirty = false;
    action?.();
  }

  function cancelDiscardProjectSettings(): void {
    pendingProjectSettingsAction = null;
    projectSettingsDiscardConfirmOpen = false;
  }

  function openArchiveFromProjectSettings(): void {
    runAfterProjectSettingsClose(() => {
      closeProjectSettingsImmediately();
      void notes.openArchive();
    });
  }

  function openTrashFromProjectSettings(): void {
    runAfterProjectSettingsClose(() => {
      closeProjectSettingsImmediately();
      void notes.openTrash();
    });
  }

  function openVersionHistoryFromProjectSettings(): void {
    runAfterProjectSettingsClose(() => {
      closeProjectSettingsImmediately();
      projectVersionHistoryOpen = true;
    });
  }

  function toggleProjectSettings(): void {
    if (projectSettingsOpen) {
      requestProjectSettingsClose();
      return;
    }
    projectSettingsOpen = true;
  }

  function createPage(): void {
    void notes.createPage("", {
      projectId: selectedProjectId,
      folderId: creationFolderId,
      openMode: "full",
    });
  }

  function closePagePeek(): void {
    void notes.closeContextualPage().then(() => {
      if (mobileLayout) clearMobileNotesHash();
    }).catch((error: unknown) => console.warn("Close Notes preview failed", error));
  }

  function clearMobileNotesHash(): void {
    if (!window.location.hash) return;
    const previousUrl = window.location.href;
    const nextUrl = new URL(previousUrl);
    nextUrl.hash = "";
    window.history.replaceState(window.history.state, "", nextUrl);
    window.dispatchEvent(new HashChangeEvent("hashchange", {
      oldURL: previousUrl,
      newURL: nextUrl.href,
    }));
  }

  function notesShortcutTargetBlocked(target: EventTarget | Element | null): boolean {
    if (!(target instanceof Element)) return false;
    return (
      isAppShortcutBlockedTarget(target)
      || target.closest("[role='dialog']:not([data-notes-page-peek])") !== null
    );
  }

  function notesBlockEditorContainsTarget(target: EventTarget | null): boolean {
    if (!(target instanceof Element) || !notesRootElement?.contains(target)) return false;
    return target.closest("[data-notes-selectable-block-id]") !== null;
  }

  function notesHistoryShortcutTargetBlocked(target: EventTarget | null): boolean {
    return target instanceof Element
      && target.closest("[role='dialog']:not([data-notes-page-peek])") !== null;
  }

  function runNotesHistoryShortcut(action: "undo" | "redo"): void {
    void (action === "undo" ? notes.undoNotesEdit() : notes.redoNotesEdit());
  }

  function handleNotesWindowKeydown(event: KeyboardEvent): void {
    const historyAction = notesUndoShortcutAction(event);
    if (historyAction) {
      if (
        isEditableKeyboardTarget(event.target)
        || isEditableKeyboardTarget(document.activeElement)
      ) {
        return;
      }
      const fromBlockEditor = notesBlockEditorContainsTarget(event.target);
      const continuesInterruptedRepeat = event.repeat
        && activeNotesHistoryShortcut === historyAction;
      if (!fromBlockEditor && !continuesInterruptedRepeat) return;
      if (
        notesHistoryShortcutTargetBlocked(event.target)
        || notesRootElement?.querySelector("[role='dialog']:not([data-notes-page-peek])") !== null
      ) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      activeNotesHistoryShortcut = historyAction;
      runNotesHistoryShortcut(historyAction);
      return;
    }
    if (event.defaultPrevented) return;
    if (event.key.toLowerCase() !== "n" || !hasOnlyShortcutModifier(event)) return;
    if (
      notesShortcutTargetBlocked(event.target)
      || notesShortcutTargetBlocked(document.activeElement)
      || notesRootElement?.querySelector("[role='dialog']:not([data-notes-page-peek])") !== null
    ) {
      return;
    }
    event.preventDefault();
    createPage();
  }

  function handleNotesWindowKeyup(event: KeyboardEvent): void {
    const key = event.key.toLowerCase();
    if (key === "z" || key === "y" || (!event.ctrlKey && !event.metaKey)) {
      activeNotesHistoryShortcut = null;
    }
  }
</script>

<svelte:window
  onkeydowncapture={handleNotesWindowKeydown}
  onkeyup={handleNotesWindowKeyup}
  onblur={() => {
    activeNotesHistoryShortcut = null;
  }}
/>

<div
  bind:this={notesRootElement}
  class="notes-view-root flex h-full min-h-0 flex-col overflow-hidden text-foreground"
  class:notes-view-mobile={mobileLayout}
  data-first-use-shell="notes"
>
  {#snippet workspaceHeader()}
    <NotesWorkspaceHeader
      {mobileLayout}
      {selectedProject}
      {selectedGroup}
      {selectedProjectId}
      selectedPage={topBarSelectedPage}
      {creationFolderId}
      showInactiveProjects={projects.showInactiveProjects}
      onShowInactiveProjectsChange={(value) => {
        projects.showInactiveProjects = value;
      }}
      onProjectSelected={handleProjectSelected}
      onShowHome={showProjectHome}
      onOpenExplorer={mobileLayout ? () => { mobileExplorerPanelOpen = true; } : undefined}
      {projectSettingsOpen}
      onToggleProjectSettings={toggleProjectSettings}
      bind:pageActionsTarget
    />
  {/snippet}
  {#if mobileLayout}{@render workspaceHeader()}{/if}
  {#if !mobileLayout && projectSettingsOpen && selectedProjectId}
    <NotesProjectSettingsPanel
        projectId={selectedProjectId}
        popoverBoundaryElement={notesRootElement}
        onRequestClose={requestProjectSettingsClose}
        onDirtyChange={(dirty) => {
          projectSettingsDirty = dirty;
        }}
        onOpenVersionHistory={openVersionHistoryFromProjectSettings}
        onOpenArchive={openArchiveFromProjectSettings}
        onOpenTrash={openTrashFromProjectSettings}
    />
  {/if}
  {#if !mobileLayout && projectVersionHistoryOpen && selectedProject}
    {#if projectHistoryLoadState?.status === "ready" && projectHistoryLoadState.component.kind === "project-history"}
      {@const NotesProjectVersionHistoryModal = projectHistoryLoadState.component.component}
      <NotesProjectVersionHistoryModal
        projectId={selectedProject.id}
        onClose={() => {
          projectVersionHistoryOpen = false;
        }}
        onRestored={() => {
          void notes.load().catch((error) => {
            console.error("reload notes after project history restore failed", error);
          });
        }}
      />
    {:else}
      <div class="fixed inset-0 z-90 flex items-center justify-center surface-backdrop p-4" role="dialog" aria-modal="true" aria-busy={projectHistoryLoadState?.status !== "failed"}>
        {#if projectHistoryLoadState?.status === "failed"}
          <div class="surface-dialog p-4 text-sm">
            <Notice message={t("common.viewLoadFailed", t("notes.projectSettingsVersionHistory"))} copy={false} onRetry={() => requestNotesOptionalComponent("project-history", true)}>
              {#snippet actions()}<button type="button" class="notice-action-text" onclick={() => { projectVersionHistoryOpen = false; }}>{t("common.close")}</button>{/snippet}
            </Notice>
          </div>
        {:else}
          <Loading layout="fill" label={t("notes.projectSettingsVersionHistory")} />
        {/if}
      </div>
    {/if}
  {/if}
  {#if !mobileLayout && projectSettingsDiscardConfirmOpen}
    {#if confirmDialogLoadState?.status === "ready" && confirmDialogLoadState.component.kind === "confirm-dialog"}
      {@const ConfirmDialog = confirmDialogLoadState.component.component}
      <ConfirmDialog
        title={t("calendar.view.discardUnsavedTitle")}
        message={t("calendar.view.changesLost")}
        confirmLabel={t("calendar.view.discard")}
        cancelLabel={t("common.cancel")}
        onConfirm={confirmDiscardProjectSettings}
        onCancel={cancelDiscardProjectSettings}
      />
    {:else if confirmDialogLoadState?.status === "failed"}
      <div class="fixed inset-0 z-100 flex items-center justify-center surface-backdrop p-4" role="dialog" aria-modal="true">
        <div class="surface-dialog p-4 text-sm">
          <Notice message={t("common.viewLoadFailed", t("calendar.view.discard"))} copy={false} onRetry={() => requestNotesOptionalComponent("confirm-dialog", true)}>
            {#snippet actions()}<button type="button" class="notice-action-text" onclick={cancelDiscardProjectSettings}>{t("common.cancel")}</button>{/snippet}
          </Notice>
        </div>
      </div>
    {:else}
      <div class="fixed inset-0 z-100 flex items-center justify-center surface-backdrop p-4" role="dialog" aria-modal="true" aria-busy="true">
        <Loading layout="fill" />
      </div>
    {/if}
  {/if}
  {#each databaseDeletionPanes as pane (pane.id)}
    {#if databaseDeletionDialogLoadState?.status === "ready"}
      {@const DatabaseDeletionDialog = databaseDeletionDialogLoadState.component}
      <DatabaseDeletionDialog controller={pane.store.databaseDeletion} />
    {:else}
      <div class="fixed inset-0 z-100 flex items-center justify-center surface-backdrop p-4" role="dialog" aria-modal="true" aria-busy={databaseDeletionDialogLoadState?.status !== "failed"}>
        <div class="surface-dialog p-4 text-sm">
          {#if databaseDeletionDialogLoadState?.status === "failed"}
            <Notice message={t("common.viewLoadFailed", t("notes.databaseDelete"))} copy={false} onRetry={() => requestDatabaseDeletionDialog(true)}>
              {#snippet actions()}<button type="button" class="notice-action-text" onclick={pane.store.databaseDeletion.cancel}>{t("common.cancel")}</button>{/snippet}
            </Notice>
          {:else}
            <Loading layout="fill" label={t("notes.databaseDelete")} class="p-2" />
            <button type="button" class="mt-3 min-h-9 rounded-floating-item border border-border px-3 hover:bg-accent" onclick={pane.store.databaseDeletion.cancel}>{t("common.cancel")}</button>
          {/if}
        </div>
      </div>
    {/if}
  {/each}
  <div class="notes-view-layout relative flex min-h-0 flex-1 overflow-hidden" class:explorer-chrome={!mobileLayout}>
    {#if projectSidebar && !mobileLayout}
      {@render projectSidebar(handleProjectSelected)}
    {:else if showProjectExplorer || showMobileExplorerPanel}
      <NotesProjectHome
        {mobileLayout}
        panel={showMobileExplorerPanel}
        onClosePanel={() => {
          mobileExplorerPanelOpen = false;
        }}
        projectId={selectedProjectId}
        bind:explorerCollapsed={notes.explorerCollapsed}
        {creationFolderId}
        {selectedWorkingMarkdownFile}
        onSelectWorkingMarkdownFile={selectWorkingMarkdownFile}
        onBeforeDocumentNavigation={beforeDocumentNavigation}
        onCreationFolderChange={(folderId) => {
          creationFolderOverride = folderId;
        }}
      />
    {/if}
    {#if showPrimaryContent}
      <div class="notes-view-canvas flex min-w-0 flex-1 flex-col overflow-hidden" class:surface-content={!mobileLayout}>
        {#if !mobileLayout}{@render workspaceHeader()}{/if}
        <div class="flex min-h-0 min-w-0 flex-1 overflow-hidden">
          {#if !notes.loaded && notes.loadError}
            <div class="flex min-w-0 flex-1 text-sm" data-notes-first-use-state>
              <Notice layout="fill" message={t("notes.loadFailed", notes.loadError)} onRetry={() => notes.load()} />
            </div>
          {:else if !mobileLayout && selectedWorkingMarkdownFile}
            <NotesWorkingMarkdownEditor
              file={selectedWorkingMarkdownFile}
              onDirtyChange={(dirty) => { workingMarkdownDirty = dirty; }}
            />
          {:else if activeSurfaceKind === "archive" || activeSurfaceKind === "trash"}
            {#if activeSurfaceLoadState?.status === "ready" && activeSurfaceLoadState.component.kind === activeSurfaceKind}
              {@const ActiveNotesSurface = activeSurfaceLoadState.component.component}
              <ActiveNotesSurface />
            {:else if activeSurfaceLoadState?.status === "failed"}
              <div class="flex min-w-0 flex-1 text-sm">
                <Notice
                  layout="fill"
                  message={t("common.viewLoadFailed", activeSurfaceKind === "archive" ? t("notes.archive") : t("notes.trash"))}
                  copy={false}
                  onRetry={() => requestNotesSurface(activeSurfaceKind, true)}
                />
              </div>
            {:else}
              <Loading layout="fill" label={activeSurfaceKind === "archive" ? t("notes.loadingArchive") : t("notes.loadingTrash")} class="min-w-0 flex-1" />
            {/if}
          {:else if showEmptyProjectState}
            <NotesProjectEmptyState onCreate={createPage} />
          {:else}
            {#each notes.editorPanes as pane (pane.id)}
              {@const store = pane.store}
              {@const isContextual = contextualPane?.id === pane.id}
              {@const center = !showDatabasePage && isContextual && showCenterPeek}
              {@const side = !showDatabasePage && isContextual && showSidePeek}
              {@const hidden = showDatabasePage
                ? notes.activePaneId !== pane.id
                : !isContextual && peekPromotesToFullPage}
              {@const selected = store.selectedPageId !== null && !store.isPagePendingRemoval(store.selectedPageId)}
              <div
                class={hidden ? "hidden" : center
                  ? "surface-backdrop fixed inset-x-0 bottom-0 z-50 flex items-center justify-center px-3 py-4 sm:px-6 sm:py-8"
                  : side ? "ml-auto flex min-w-0 basis-1/2 overflow-hidden"
                    : !showDatabasePage && showSidePeek ? "flex min-w-0 basis-1/2 overflow-hidden" : "flex min-w-0 flex-1 overflow-hidden"}
                style={center ? "top: calc(var(--titlebar-h) + var(--toolbar-h));" : undefined}
                inert={hidden || (!showDatabasePage && !isContextual && showCenterPeek)}
                data-notes-pane={pane.id}
                data-floating-root
                data-notes-main-page={pane.id === notes.mainPaneId ? store.selectedPageId : undefined}
                role="presentation"
                onpointerdowncapture={(event) => {
                  if (event.target !== event.currentTarget || !center) notes.activatePane(pane.id, !projectSidebar);
                }}
                onfocusincapture={() => notes.activatePane(pane.id, !projectSidebar)}
                onclick={(event) => { if (center && event.target === event.currentTarget) closePagePeek(); }}
              >
                <div
                  class={center
                    ? "notes-center-peek-panel flex min-w-0 overflow-hidden rounded-floating border border-border shadow-floating"
                    : side ? "flex min-w-0 flex-1 overflow-hidden border-l border-border" : "flex min-w-0 flex-1 overflow-hidden"}
                  style="background-color: var(--cal-bg);"
                  role={!showDatabasePage && isContextual && showPagePeek ? "dialog" : undefined}
                  aria-modal={!showDatabasePage && isContextual && showPagePeek ? center : undefined}
                  data-notes-page-peek={!showDatabasePage && isContextual && showPagePeek || undefined}
                >
                  {#if selected}
                    {#if store.selectedDatabaseBlockId}
                      {#key store.selectedDatabaseBlockId}
                        <NotesDatabasePage
                          editorStore={store}
                          {mobileLayout}
                          pageActionsTarget={notes.activePaneId === pane.id ? pageActionsTarget : null}
                        />
                      {/key}
                    {/if}
                    <div
                      class={store.selectedDatabaseBlockId ? "hidden" : "flex min-h-0 min-w-0 flex-1 overflow-hidden"}
                      inert={store.selectedDatabaseBlockId != null}
                      data-notes-editor-container
                    >
                    {#key store.selectedPageId}
                      <NotesLoadingSkeleton
                        ready={store.loadedPage?.id === store.selectedPageId && store.primaryContentReady || !!store.loadError}
                        page={store.allPages.find((page) => page.id === store.selectedPageId) ?? store.linkResolutionPages.find((page) => page.id === store.selectedPageId) ?? null}
                        openMode={isContextual && showPagePeek ? store.pageOpenMode : "full"}
                      >
                        {#snippet children()}
                          {#if store.loadedPage?.id === store.selectedPageId && store.primaryContentReady
                            && (!store.selectedDatabaseBlockId || retainedEditorPages[pane.id] === store.selectedPageId)}
                            <NotesEditor
                              projectId={selectedProjectId}
                              editorStore={store}
                              active={!store.selectedDatabaseBlockId && notes.activePaneId === pane.id}
                              openMode={isContextual && showPagePeek ? store.pageOpenMode : "full"}
                              pageActionsTarget={!store.selectedDatabaseBlockId && notes.activePaneId === pane.id ? pageActionsTarget : null}
                              onClose={() => { void notes.closePane(pane.id).catch((error: unknown) => console.warn("Close Notes pane failed", error)); }}
                              onOpenModeChange={(mode) => { void notes.showPaneAs(pane.id, mode).catch((error: unknown) => console.warn("Change Notes pane mode failed", error)); }}
                            />
                          {:else if store.loadError}
                            <div class="flex min-w-0 flex-1 text-sm">
                              <Notice layout="fill" message={t("notes.loadFailed", store.loadError)} onRetry={() => store.selectPageLocally(store.selectedPageId)}>
                                {#snippet actions()}
                                  {#if isContextual && showPagePeek}
                                    <button type="button" class="notice-action-text" onclick={() => { void notes.closePane(pane.id).catch((error: unknown) => console.warn("Close Notes pane failed", error)); }}>{t("notes.closePeek")}</button>
                                  {/if}
                                {/snippet}
                              </Notice>
                            </div>
                          {/if}
                        {/snippet}
                      </NotesLoadingSkeleton>
                    {/key}
                    </div>
                  {/if}
                </div>
              </div>
            {/each}
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .notes-view-root {
    container: notes-view / inline-size;
  }

  .notes-view-mobile,
  .notes-view-canvas {
    background-color: var(--cal-bg);
  }

  .notes-center-peek-panel {
    width: clamp(560px, calc(100vw - 214px), 960px);
    height: min(667px, calc(100dvh - 214px));
  }

  @container notes-view (max-width: 34rem) {
    .notes-view-root:not(.notes-view-mobile) :global(.notes-project-explorer) {
      display: none;
    }
  }
</style>
