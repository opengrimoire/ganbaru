<script lang="ts">
  import SearchField from "$lib/components/ui/SearchField.svelte";
  import { onMount, tick } from "svelte";
  import ArrowUpNarrowWide from "@lucide/svelte/icons/arrow-up-narrow-wide";
  import Check from "@lucide/svelte/icons/check";
  import ChevronsLeft from "@lucide/svelte/icons/chevrons-left";
  import ChevronsRight from "@lucide/svelte/icons/chevrons-right";
  import ChevronsDownUp from "@lucide/svelte/icons/chevrons-down-up";
  import ChevronsUpDown from "@lucide/svelte/icons/chevrons-up-down";
  import FileQuestionMark from "@lucide/svelte/icons/file-question-mark";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import Search from "@lucide/svelte/icons/search";
  import SquarePen from "@lucide/svelte/icons/square-pen";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    buildWorkingMarkdownTreeItems,
    listNotesWorkingMarkdown,
    NotesWorkingMarkdownTree,
  } from "$lib/components/notes/working-markdown/platform";
  import {
    beginLazyComponentLoad,
    rejectLazyComponentLoad,
    resolveLazyComponentLoad,
    type LazyComponentLoadState,
  } from "$lib/lazy-component-loader";
  import {
    buildNotesNavigationTree,
    notesFolderMoveTargets,
    notesFoldersForProject,
    notesPageFolderMoveTargets,
    type NotesNavigationSortOrder,
    type NotesNavigationTreeItem,
  } from "$lib/notes/navigation/tree";
  import {
    canDropNotesNavigationItem,
    type NotesNavigationDragItem,
    type NotesNavigationDropTarget,
  } from "$lib/notes/navigation/drag";
  import { notesPageMoveTargets } from "$lib/notes/pages/move";
  import { notesPageContainingFolderId } from "$lib/notes/navigation/hierarchy-menu";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import { notesPagesForProject } from "$lib/notes/project-membership";
  import type {
    NotesFolder,
    NotesPage,
    NotesWorkingMarkdownFileRef,
    NotesWorkingMarkdownTreeRead,
  } from "$lib/notes/types";
  import { getNotes } from "$lib/stores/notes.svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import NotesFolderRow from "$lib/components/notes/navigation/NotesFolderRow.svelte";
  import NotesPageRow from "$lib/components/notes/navigation/NotesPageRow.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import {
    loadNotesOptionalComponent,
    retryNotesOptionalComponent,
    type LoadedNotesOptionalComponent,
  } from "./component-registry";

  let {
    mobileLayout = false,
    projectId = null,
    explorerCollapsed = $bindable(false),
    creationFolderId = null,
    onCreationFolderChange,
    selectedWorkingMarkdownFile = null,
    onSelectWorkingMarkdownFile,
    onBeforeDocumentNavigation,
  }: {
    mobileLayout?: boolean;
    projectId?: string | null;
    explorerCollapsed?: boolean;
    creationFolderId?: string | null;
    onCreationFolderChange: (folderId: string | null) => void;
    selectedWorkingMarkdownFile?: NotesWorkingMarkdownFileRef | null;
    onSelectWorkingMarkdownFile: (file: NotesWorkingMarkdownFileRef) => void;
    onBeforeDocumentNavigation: () => boolean;
  } = $props();

  const notes = getNotes();
  const { t } = getLocalization();
  const explorerIconStrokeWidth = 1.5;
  const sortMenuViewportGapPx = 8;
  const sortMenuTriggerGapPx = 4;
  const navigationFolderOpenDelayMs = 520;
  const navigationAutoScrollEdgePx = 36;
  const navigationAutoScrollStepPx = 12;
  const navigationPointerDragThresholdPx = 5;

  interface PendingNavigationPointerDrag {
    pointerId: number;
    startX: number;
    startY: number;
    item: NotesNavigationDragItem;
    sourceElement: HTMLElement;
  }

  interface NavigationPointerPosition {
    x: number;
    y: number;
  }

  function handleWorkspaceScroll(event: Event): void {
    const viewport = event.currentTarget;
    if (!(viewport instanceof HTMLDivElement)) return;
    if (viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight > 240) return;
    void notes.loadMoreWorkspaceWindow().catch((error) => {
      console.error("load more notes workspace pages failed", error);
    });
  }

  function resetCreationLocationOnBlankPointer(node: HTMLElement) {
    const handlePointerDown = (event: PointerEvent): void => {
      if (event.button === 0 && event.target === node) onCreationFolderChange(null);
    };
    node.addEventListener("pointerdown", handlePointerDown);
    return {
      destroy(): void {
        node.removeEventListener("pointerdown", handlePointerDown);
      },
    };
  }

  function navigationTargetKey(target: NotesNavigationDropTarget): string {
    return target.kind === "root" ? "root" : `${target.kind}:${target.id}`;
  }

  function pageNavigationDropTarget(pageId: string): NotesNavigationDropTarget {
    const folderId = notesPageContainingFolderId(pageId, projectPages);
    return folderId ? { kind: "folder", id: folderId } : { kind: "root" };
  }

  function explorerItemDropTarget(item: NotesNavigationTreeItem): NotesNavigationDropTarget {
    if (item.kind === "page") return pageNavigationDropTarget(item.page.id);
    return { kind: "folder", id: item.folder.id };
  }

  function explorerDropTargetAtPoint(
    x: number,
    y: number,
  ): NotesNavigationDropTarget | null {
    const explorer = explorerScrollElement;
    if (!explorer) return null;
    const bounds = explorer.getBoundingClientRect();
    if (x < bounds.left || x > bounds.right || y < bounds.top || y > bounds.bottom) return null;
    const hit = document.elementFromPoint(x, y);
    const targetElement = hit?.closest<HTMLElement>("[data-notes-navigation-drop-kind]");
    if (!targetElement || !explorer.contains(targetElement)) return { kind: "root" };
    if (targetElement.dataset.notesNavigationDropKind !== "folder") return { kind: "root" };
    const folderId = targetElement.dataset.notesNavigationDropId;
    return folderId ? { kind: "folder", id: folderId } : { kind: "root" };
  }

  function clearNavigationFolderOpenTimer(): void {
    if (navigationFolderOpenTimer === null) return;
    window.clearTimeout(navigationFolderOpenTimer);
    navigationFolderOpenTimer = null;
  }

  function clearNavigationDropTarget(): void {
    clearNavigationFolderOpenTimer();
    navigationDropTargetKey = null;
    navigationDropAllowed = false;
  }

  function clearNavigationAutoScrollFrame(): void {
    if (navigationAutoScrollFrame === null) return;
    window.cancelAnimationFrame(navigationAutoScrollFrame);
    navigationAutoScrollFrame = null;
  }

  function removeNavigationDragPreview(): void {
    navigationDragPreview?.remove();
    navigationDragPreview = null;
    navigationDragPreviewAnchor = null;
  }

  function clearNavigationDrag(): void {
    clearNavigationAutoScrollFrame();
    clearNavigationDropTarget();
    removeNavigationDragPreview();
    pendingNavigationPointerDrag = null;
    navigationPointerPosition = null;
    draggingNavigationItem = null;
  }

  function createNavigationDragPreview(
    source: HTMLElement,
    pointer: NavigationPointerPosition,
  ): void {
    const content = source.querySelector<HTMLElement>(
      ".notes-folder-row-content, .notes-page-row-content",
    );
    if (!content) return;
    const preview = content.cloneNode(true);
    if (!(preview instanceof HTMLElement)) return;
    const sourceBounds = content.getBoundingClientRect();
    const width = Math.min(sourceBounds.width, 224);
    navigationDragPreviewAnchor = {
      x: Math.min(Math.max(pointer.x - sourceBounds.left, 0), width),
      y: Math.min(Math.max(pointer.y - sourceBounds.top, 0), sourceBounds.height),
    };
    Object.assign(preview.style, {
      position: "fixed",
      left: "0",
      top: "0",
      zIndex: "9999",
      width: `${Math.max(width, 144)}px`,
      margin: "0",
      background: "transparent",
      color: "hsl(var(--foreground))",
      border: "0",
      borderRadius: "0",
      boxShadow: "none",
      opacity: "0.96",
      pointerEvents: "none",
    });
    document.body.append(preview);
    navigationDragPreview = preview;
  }

  function positionNavigationDragPreview(position: NavigationPointerPosition): void {
    if (!navigationDragPreview || !navigationDragPreviewAnchor) return;
    const x = position.x - navigationDragPreviewAnchor.x;
    const y = position.y - navigationDragPreviewAnchor.y;
    navigationDragPreview.style.transform = `translate3d(${x}px, ${y}px, 0)`;
  }

  function updateNavigationDropTarget(target: NotesNavigationDropTarget | null): boolean {
    const source = draggingNavigationItem;
    if (!source || !target) {
      clearNavigationDropTarget();
      return false;
    }
    const allowed = canDropNotesNavigationItem(source, target, projectPages, projectFolders);
    const targetKey = navigationTargetKey(target);
    if (targetKey !== navigationDropTargetKey || allowed !== navigationDropAllowed) {
      navigationDropTargetKey = targetKey;
      navigationDropAllowed = allowed;
      scheduleFolderOpen(target, allowed);
    }
    return allowed;
  }

  function updateNavigationDropTargetAtPointer(): NotesNavigationDropTarget | null {
    const position = navigationPointerPosition;
    if (!position) return null;
    const target = explorerDropTargetAtPoint(position.x, position.y);
    updateNavigationDropTarget(target);
    return target;
  }

  function navigationAutoScrollDelta(position: NavigationPointerPosition): number {
    const explorer = explorerScrollElement;
    if (!explorer) return 0;
    const bounds = explorer.getBoundingClientRect();
    if (position.y < bounds.top + navigationAutoScrollEdgePx) return -navigationAutoScrollStepPx;
    if (position.y > bounds.bottom - navigationAutoScrollEdgePx) return navigationAutoScrollStepPx;
    return 0;
  }

  function runNavigationAutoScrollFrame(): void {
    navigationAutoScrollFrame = null;
    const explorer = explorerScrollElement;
    const position = navigationPointerPosition;
    if (!draggingNavigationItem || !explorer || !position) return;
    const delta = navigationAutoScrollDelta(position);
    if (delta === 0) return;
    const previousScrollTop = explorer.scrollTop;
    explorer.scrollTop += delta;
    if (explorer.scrollTop === previousScrollTop) return;
    updateNavigationDropTargetAtPointer();
    navigationAutoScrollFrame = window.requestAnimationFrame(runNavigationAutoScrollFrame);
  }

  function syncNavigationAutoScroll(): void {
    clearNavigationAutoScrollFrame();
    if (!navigationPointerPosition || navigationAutoScrollDelta(navigationPointerPosition) === 0) return;
    navigationAutoScrollFrame = window.requestAnimationFrame(runNavigationAutoScrollFrame);
  }

  function beginNavigationPointerDrag(pending: PendingNavigationPointerDrag): void {
    draggingNavigationItem = pending.item;
    navigationDragError = null;
    sortMenuOpen = false;
    createNavigationDragPreview(pending.sourceElement, {
      x: pending.startX,
      y: pending.startY,
    });
  }

  function handleNavigationDragWheel(event: WheelEvent): void {
    if (!draggingNavigationItem || !explorerScrollElement) return;
    event.preventDefault();
    explorerScrollElement.scrollBy({ top: event.deltaY, left: 0 });
    window.requestAnimationFrame(() => {
      if (draggingNavigationItem) updateNavigationDropTargetAtPointer();
    });
  }

  function scheduleFolderOpen(target: NotesNavigationDropTarget, allowed: boolean): void {
    clearNavigationFolderOpenTimer();
    if (!allowed || target.kind !== "folder") return;
    if (!notes.collapsedFolderIds.includes(target.id)) return;
    navigationFolderOpenTimer = window.setTimeout(() => {
      navigationFolderOpenTimer = null;
      notes.setFolderCollapsed(target.id, false);
    }, navigationFolderOpenDelayMs);
  }

  function handleExplorerNavigationPointerDown(event: PointerEvent): void {
    if (mobileLayout || event.button !== 0 || event.pointerType === "touch") return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    if (target.closest("input, textarea, [contenteditable='true'], .explorer-row-action")) return;
    const sourceElement = target.closest<HTMLElement>("[data-notes-navigation-drag-kind]");
    if (!sourceElement || !explorerScrollElement?.contains(sourceElement)) return;
    const kind = sourceElement.dataset.notesNavigationDragKind;
    const id = sourceElement.dataset.notesNavigationDragId;
    if ((kind !== "folder" && kind !== "page") || !id) return;
    pendingNavigationPointerDrag = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      item: { kind, id },
      sourceElement,
    };
  }

  function handleNavigationPointerMove(event: PointerEvent): void {
    const pending = pendingNavigationPointerDrag;
    if (!pending || pending.pointerId !== event.pointerId) return;
    const position = { x: event.clientX, y: event.clientY };
    if (!draggingNavigationItem) {
      const distance = Math.hypot(event.clientX - pending.startX, event.clientY - pending.startY);
      if (distance < navigationPointerDragThresholdPx) return;
      beginNavigationPointerDrag(pending);
    }
    event.preventDefault();
    navigationPointerPosition = position;
    positionNavigationDragPreview(position);
    updateNavigationDropTargetAtPointer();
    syncNavigationAutoScroll();
  }

  function suppressNavigationClick(event: MouseEvent): void {
    if (!suppressNextNavigationClick) return;
    suppressNextNavigationClick = false;
    event.preventDefault();
    event.stopPropagation();
  }

  function handleNavigationPointerUp(event: PointerEvent): void {
    const pending = pendingNavigationPointerDrag;
    if (!pending || pending.pointerId !== event.pointerId) return;
    const source = draggingNavigationItem;
    pendingNavigationPointerDrag = null;
    if (!source) return;
    event.preventDefault();
    navigationPointerPosition = { x: event.clientX, y: event.clientY };
    const target = explorerDropTargetAtPoint(event.clientX, event.clientY);
    const allowed = target !== null
      && canDropNotesNavigationItem(source, target, projectPages, projectFolders);
    clearNavigationDrag();
    suppressNextNavigationClick = true;
    window.setTimeout(() => {
      suppressNextNavigationClick = false;
    }, 0);
    if (!target || !allowed) return;
    void moveNavigationItem(source, target).catch((error: unknown) => {
      navigationDragError = error instanceof Error ? error.message : String(error);
    });
  }

  function cancelNavigationPointerDrag(): void {
    if (!pendingNavigationPointerDrag && !draggingNavigationItem) return;
    clearNavigationDrag();
  }

  async function moveNavigationItem(
    source: NotesNavigationDragItem,
    target: NotesNavigationDropTarget,
  ): Promise<void> {
    if (source.kind === "folder") {
      await notes.moveFolder(source.id, target.kind === "folder" ? target.id : null);
      if (target.kind === "folder") notes.setFolderCollapsed(target.id, false);
      return;
    }
    if (target.kind === "page") {
      await notes.movePage(
        source.id,
        { type: "page_id", page_id: target.id },
        { preserveSelection: true },
      );
      return;
    }
    await notes.movePageToFolder(
      source.id,
      target.kind === "folder" ? target.id : null,
      { preserveSelection: true },
    );
    if (target.kind === "folder") notes.setFolderCollapsed(target.id, false);
  }

  let search = $state("");
  let searchOpen = $state(false);
  let searchButton: HTMLButtonElement;
  let sortMenuOpen = $state(false);
  let sortOrder = $state<NotesNavigationSortOrder>("name-asc");
  let currentFileHighlightRequestId = $state(0);
  let hoveredSortOrder = $state<NotesNavigationSortOrder | null>(null);
  let sortMenuStyle = $state("");
  let sortButtonElement = $state<HTMLButtonElement | null>(null);
  let sortMenuElement = $state<HTMLDivElement | null>(null);
  let explorerScrollElement = $state<HTMLDivElement | null>(null);
  let expandExplorerButtonElement = $state<HTMLButtonElement | null>(null);
  let collapseExplorerButtonElement = $state<HTMLButtonElement | null>(null);
  let pendingArchivePage = $state<NotesPage | null>(null);
  let pendingTrashPage = $state<NotesPage | null>(null);
  let pendingDeleteFolder = $state<NotesFolder | null>(null);
  let folderRenameTargetId = $state<string | null>(null);
  let folderRenameRequestId = $state(0);
  let folderActionError = $state<string | null>(null);
  let navigationDragError = $state<string | null>(null);
  let draggingNavigationItem = $state<NotesNavigationDragItem | null>(null);
  let navigationDropTargetKey = $state<string | null>(null);
  let navigationDropAllowed = $state(false);
  let navigationFolderOpenTimer: number | null = null;
  let navigationAutoScrollFrame: number | null = null;
  let navigationDragPreview: HTMLElement | null = null;
  let navigationDragPreviewAnchor: NavigationPointerPosition | null = null;
  let navigationPointerPosition: NavigationPointerPosition | null = null;
  let pendingNavigationPointerDrag: PendingNavigationPointerDrag | null = null;
  let suppressNextNavigationClick = false;
  let confirmDialogLoadState = $state<LazyComponentLoadState<
    "confirm-dialog",
    LoadedNotesOptionalComponent
  > | null>(null);
  let workingMarkdownTree = $state<NotesWorkingMarkdownTreeRead>({
    roots: [],
    unavailableWorkingFolderIds: [],
  });
  let workingMarkdownLoading = $state(false);
  let workingMarkdownError = $state<string | null>(null);
  let workingMarkdownGeneration = 0;
  let workingMarkdownTreeProjectId: string | null = null;

  const projectPages = $derived.by(() => notesPagesForProject(
    notes.navigationPages,
    projectId,
  ));
  const projectFolders = $derived.by(() => notesFoldersForProject(notes.folders, projectId));
  const selectedRootPageId = $derived(projectPages.find((page) =>
    page.parent.type === "workspace" && (page.id === notes.selectedPageId
      || notes.pageBreadcrumbItems.some((crumb) => crumb.id === page.id)))?.id ?? null);
  const treeItems = $derived.by(() =>
    buildNotesNavigationTree(projectPages, projectFolders, {
      rootPagesOnly: true,
      activePageId: selectedRootPageId,
      collapsedFolderIds: notes.collapsedFolderIds,
      pageIdsWithChildren: notes.sidebarPageIdsWithChildren,
      missingParentPageIds: notes.sidebarMissingParentPageIds,
      trashedParentPageIds: notes.sidebarTrashedParentPageIds,
      query: search,
      sortOrder,
      titleForPage: (page) => notesPageTitle(page, t("notes.untitled")),
    })
  );
  const explorerItems = $derived(treeItems);
  const workingMarkdownHasMatches = $derived(
    buildWorkingMarkdownTreeItems(workingMarkdownTree.roots, new Set(), search).length > 0,
  );
  const navigationFolderDropArea = $derived.by(() => {
    const result = new Map<string, {
      state: "valid" | "invalid";
      position: "single" | "start" | "middle" | "end";
    }>();
    if (!draggingNavigationItem || !navigationDropTargetKey?.startsWith("folder:")) return result;
    const folderId = navigationDropTargetKey.slice("folder:".length);
    const startIndex = explorerItems.findIndex(
      (item) => item.kind === "folder" && item.folder.id === folderId,
    );
    if (startIndex < 0) return result;
    const folderDepth = explorerItems[startIndex]?.depth;
    if (folderDepth === undefined) return result;
    let endIndex = startIndex;
    while (
      endIndex + 1 < explorerItems.length
      && (explorerItems[endIndex + 1]?.depth ?? 0) > folderDepth
    ) {
      endIndex += 1;
    }
    const state = navigationDropAllowed ? "valid" : "invalid";
    for (let index = startIndex; index <= endIndex; index += 1) {
      const item = explorerItems[index];
      if (!item) continue;
      const position = startIndex === endIndex
        ? "single"
        : index === startIndex
          ? "start"
          : index === endIndex
            ? "end"
            : "middle";
      result.set(item.key, { state, position });
    }
    return result;
  });

  $effect(() => {
    if (!draggingNavigationItem) return;
    const root = document.documentElement;
    root.classList.add("notes-navigation-pointer-dragging");
    return () => {
      root.classList.remove("notes-navigation-pointer-dragging");
    };
  });
  const shouldExpandExplorer = $derived(
    projectFolders.some((folder) => notes.collapsedFolderIds.includes(folder.id))
  );

  async function setExplorerCollapsed(collapsed: boolean): Promise<void> {
    sortMenuOpen = false;
    explorerCollapsed = collapsed;
    await tick();
    if (collapsed) {
      expandExplorerButtonElement?.focus();
    } else {
      collapseExplorerButtonElement?.focus();
    }
  }

  function requestConfirmDialog(retry = false): void {
    if (!retry && confirmDialogLoadState?.key === "confirm-dialog") return;
    const loadingState = beginLazyComponentLoad(confirmDialogLoadState, "confirm-dialog");
    confirmDialogLoadState = loadingState;
    const request = retry
      ? retryNotesOptionalComponent("confirm-dialog")
      : loadNotesOptionalComponent("confirm-dialog");
    void request.then((component) => {
      if (!confirmDialogLoadState) return;
      confirmDialogLoadState = resolveLazyComponentLoad(
        confirmDialogLoadState,
        "confirm-dialog",
        loadingState.requestId,
        component,
      );
    }).catch((error: unknown) => {
      if (!confirmDialogLoadState) return;
      confirmDialogLoadState = rejectLazyComponentLoad(
        confirmDialogLoadState,
        "confirm-dialog",
        loadingState.requestId,
        error,
      );
      console.error("load Notes confirmation dialog failed", error);
    });
  }

  function clearPendingConfirmation(): void {
    pendingArchivePage = null;
    pendingTrashPage = null;
    pendingDeleteFolder = null;
  }

  $effect(() => {
    if (pendingArchivePage || pendingTrashPage || pendingDeleteFolder) requestConfirmDialog();
  });

  function createPage(folderId: string | null = creationFolderId): void {
    if (!onBeforeDocumentNavigation()) return;
    if (folderId) notes.setFolderCollapsed(folderId, false);
    if (notes.viewMode === "archive") notes.closeArchive();
    if (notes.viewMode === "trash") notes.closeTrash();
    void notes.createPage("", { projectId, folderId, openMode: "full" });
  }

  function createSubpage(parentPageId: string): void {
    if (!onBeforeDocumentNavigation()) return;
    void notes.createSubpage(parentPageId, "", { openMode: "full" });
  }

  async function renamePage(pageId: string, title: string): Promise<boolean> {
    try {
      await notes.renamePage(pageId, title);
      return true;
    } catch (error) {
      console.error("rename Notes page failed", error);
      return false;
    }
  }

  function selectPrimaryPage(pageId: string): void {
    if (!onBeforeDocumentNavigation()) return;
    if (notes.viewMode === "archive") notes.closeArchive();
    if (notes.viewMode === "trash") notes.closeTrash();
    void notes.selectPage(pageId, { openMode: "full" }).catch((error) => {
      console.error("open Notes page failed", error);
    });
  }

  async function refreshWorkingMarkdown(): Promise<void> {
    if (mobileLayout) {
      workingMarkdownGeneration += 1;
      workingMarkdownTreeProjectId = null;
      workingMarkdownTree = { roots: [], unavailableWorkingFolderIds: [] };
      workingMarkdownLoading = false;
      workingMarkdownError = null;
      return;
    }
    const requestedProjectId = projectId;
    const generation = ++workingMarkdownGeneration;
    if (requestedProjectId !== workingMarkdownTreeProjectId) {
      workingMarkdownTreeProjectId = requestedProjectId;
      workingMarkdownTree = { roots: [], unavailableWorkingFolderIds: [] };
    }
    if (!requestedProjectId) {
      workingMarkdownLoading = false;
      workingMarkdownError = null;
      return;
    }
    workingMarkdownLoading = true;
    workingMarkdownError = null;
    try {
      const tree = await listNotesWorkingMarkdown(requestedProjectId);
      if (generation !== workingMarkdownGeneration || projectId !== requestedProjectId) return;
      workingMarkdownTree = tree;
    } catch (error) {
      if (generation !== workingMarkdownGeneration || projectId !== requestedProjectId) return;
      workingMarkdownTree = { roots: [], unavailableWorkingFolderIds: [] };
      workingMarkdownError = error instanceof Error ? error.message : String(error);
    } finally {
      if (generation === workingMarkdownGeneration) workingMarkdownLoading = false;
    }
  }

  $effect(() => {
    const activeProjectId = projectId;
    void activeProjectId;
    void refreshWorkingMarkdown();
  });

  onMount(() => {
    if (mobileLayout) return;
    const refreshOnFocus = () => { void refreshWorkingMarkdown(); };
    window.addEventListener("focus", refreshOnFocus);
    return () => {
      workingMarkdownGeneration += 1;
      window.removeEventListener("focus", refreshOnFocus);
    };
  });

  function toggleSearch(): void {
    searchOpen = !searchOpen;
    if (!searchOpen) {
      search = "";
      searchButton?.focus();
    }
  }

  function selectSortOrder(nextOrder: NotesNavigationSortOrder): void {
    sortOrder = nextOrder;
    sortMenuOpen = false;
  }

  function sortOptionClass(order: NotesNavigationSortOrder): string {
    return cn("menu-item grid grid-cols-[max-content_1rem]", hoveredSortOrder === order && "bg-accent");
  }

  function highlightNearestSortOption(
    event: MouseEvent,
    before: NotesNavigationSortOrder,
    after: NotesNavigationSortOrder,
  ): void {
    const divider = event.currentTarget;
    if (!(divider instanceof HTMLDivElement)) return;
    const rect = divider.getBoundingClientRect();
    hoveredSortOrder = event.clientY < rect.top + rect.height / 2 ? before : after;
  }

  function positionSortMenu(): void {
    if (!sortButtonElement) return;
    const triggerRect = sortButtonElement.getBoundingClientRect();
    const availableWidth = Math.max(0, window.innerWidth - sortMenuViewportGapPx * 2);
    const measuredWidth = sortMenuElement?.getBoundingClientRect().width ?? 0;
    const width = Math.min(measuredWidth, availableWidth);
    const maxLeft = Math.max(sortMenuViewportGapPx, window.innerWidth - width - sortMenuViewportGapPx);
    const left = Math.min(Math.max(sortMenuViewportGapPx, triggerRect.left), maxLeft);
    const top = triggerRect.bottom + sortMenuTriggerGapPx;
    const maxHeight = Math.max(0, window.innerHeight - top - sortMenuViewportGapPx);
    sortMenuStyle = `left: ${left}px; top: ${top}px; width: max-content; max-width: ${availableWidth}px; max-height: ${maxHeight}px;`;
  }

  function toggleSortMenu(): void {
    if (sortMenuOpen) {
      sortMenuOpen = false;
      return;
    }
    hoveredSortOrder = null;
    positionSortMenu();
    sortMenuOpen = true;
    void tick().then(positionSortMenu);
  }

  async function highlightCurrentFile(): Promise<void> {
    const selectedPageId = selectedRootPageId;
    if (!selectedPageId) return;
    const pageById = new Map(projectPages.map((page) => [page.id, page]));
    const folderById = new Map(projectFolders.map((folder) => [folder.id, folder]));
    const visitedPageIds = new Set<string>();
    let currentPage = pageById.get(selectedPageId);
    let folderId: string | null = null;

    while (currentPage && !visitedPageIds.has(currentPage.id)) {
      visitedPageIds.add(currentPage.id);
      if (currentPage.parent.type !== "page_id") {
        folderId = currentPage.folder_id;
        break;
      }
      currentPage = pageById.get(currentPage.parent.page_id);
    }

    const visitedFolderIds = new Set<string>();
    while (folderId && !visitedFolderIds.has(folderId)) {
      visitedFolderIds.add(folderId);
      notes.setFolderCollapsed(folderId, false);
      folderId = folderById.get(folderId)?.parent_folder_id ?? null;
    }

    await tick();
    currentFileHighlightRequestId += 1;
  }

  function handleWindowPointerDown(event: PointerEvent): void {
    const target = event.target;
    if (!(target instanceof Node) || !sortMenuOpen) return;
    if (sortButtonElement?.contains(target) || sortMenuElement?.contains(target)) return;
    sortMenuOpen = false;
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape" && sortMenuOpen) sortMenuOpen = false;
  }

  function handleWindowResize(): void {
    if (sortMenuOpen) positionSortMenu();
  }

  function toggleExplorerExpansion(): void {
    const collapsed = !shouldExpandExplorer;
    for (const folder of projectFolders) {
      notes.setFolderCollapsed(folder.id, collapsed);
    }
  }

  function nextFolderName(parentFolderId: string | null): string {
    const baseName = t("notes.defaultFolderName");
    const siblingNames = new Set(
      projectFolders
        .filter((folder) => folder.parent_folder_id === parentFolderId)
        .map((folder) => folder.name.trim().toLocaleLowerCase()),
    );
    if (!siblingNames.has(baseName.toLocaleLowerCase())) return baseName;
    let suffix = 2;
    while (siblingNames.has(`${baseName} ${suffix}`.toLocaleLowerCase())) suffix += 1;
    return `${baseName} ${suffix}`;
  }

  async function createFolder(
    parentFolderId: string | null = creationFolderId,
  ): Promise<void> {
    if (!projectId) return;
    folderActionError = null;
    try {
      const folder = await notes.createFolder(
        projectId,
        nextFolderName(parentFolderId),
        parentFolderId,
      );
      if (parentFolderId) notes.setFolderCollapsed(parentFolderId, false);
      notes.setFolderCollapsed(folder.id, false);
      folderRenameTargetId = folder.id;
      folderRenameRequestId += 1;
    } catch (error) {
      folderActionError = error instanceof Error ? error.message : String(error);
    }
  }

  async function renameFolder(folderId: string, name: string): Promise<boolean> {
    folderActionError = null;
    try {
      await notes.renameFolder(folderId, name);
      return true;
    } catch (error) {
      folderActionError = error instanceof Error ? error.message : String(error);
      return false;
    }
  }

  async function moveFolder(folderId: string, parentFolderId: string | null): Promise<void> {
    folderActionError = null;
    try {
      await notes.moveFolder(folderId, parentFolderId);
      if (parentFolderId) notes.setFolderCollapsed(parentFolderId, false);
    } catch (error) {
      folderActionError = error instanceof Error ? error.message : String(error);
    }
  }

  function duplicatePage(page: NotesPage): void {
    const title = notesPageTitle(page, t("notes.untitled"));
    void notes.duplicatePage(page.id, t("notes.duplicatePageTitle", title));
  }

  function pageMoveTargets(page: NotesPage) {
    return notesPageMoveTargets(
      projectPages,
      page.id,
      t("notes.workspace"),
      (candidate) => notesPageTitle(candidate, t("notes.untitled")),
      notes.recentPageIds,
    );
  }

  function pageFolderMoveTargets(page: NotesPage) {
    return notesPageFolderMoveTargets(
      projectPages,
      projectFolders,
      page.id,
      t("notes.projectRoot"),
      (candidate) => notesPageTitle(candidate, t("notes.untitled")),
      notes.recentPageIds,
    ).filter((target) => target.kind !== "page");
  }

  function folderMoveTargets(folder: NotesFolder) {
    return notesFolderMoveTargets(projectFolders, folder.id, t("notes.projectRoot"));
  }

  function confirmArchivePage(): void {
    const page = pendingArchivePage;
    pendingArchivePage = null;
    if (page) void notes.archivePage(page.id);
  }

  function confirmTrashPage(): void {
    const page = pendingTrashPage;
    pendingTrashPage = null;
    if (page) void notes.trashPage(page.id).catch((error) => {
      console.error("trash Notes page failed", error);
    });
  }

  async function confirmDeleteFolder(): Promise<void> {
    const folder = pendingDeleteFolder;
    pendingDeleteFolder = null;
    if (!folder) return;
    folderActionError = null;
    try {
      await notes.deleteFolder(folder.id);
    } catch (error) {
      folderActionError = error instanceof Error ? error.message : String(error);
    }
  }
</script>

<svelte:window
  onpointerdown={handleWindowPointerDown}
  onpointermove={handleNavigationPointerMove}
  onpointerup={handleNavigationPointerUp}
  onpointercancel={cancelNavigationPointerDrag}
  onblur={cancelNavigationPointerDrag}
  onkeydown={handleWindowKeydown}
  onresize={handleWindowResize}
  onwheel={handleNavigationDragWheel}
/>

<aside
  class="explorer-sidebar notes-project-explorer relative h-full min-h-0 shrink-0 overflow-hidden"
  class:explorer-touch={mobileLayout}
  class:notes-project-explorer-mobile={mobileLayout}
  class:notes-project-explorer-collapsed={!mobileLayout && explorerCollapsed}
  class:notes-project-explorer-dragging={draggingNavigationItem !== null}
  aria-label={t("notes.explorerLabel")}
  data-notes-explorer
  data-notes-first-use-state
>
  {#if !mobileLayout}
    <div
      class="notes-project-explorer-collapsed-rail absolute inset-y-0 left-0 z-10 w-11"
      aria-hidden={!explorerCollapsed}
      inert={!explorerCollapsed}
    >
      <div class="explorer-toolbar justify-center">
        <button
          bind:this={expandExplorerButtonElement}
          type="button"
          class="explorer-icon"
          aria-label={t("notes.expandExplorerSidebar")}
          aria-expanded="false"
          data-app-tooltip={t("notes.expandExplorerSidebar")}
          onclick={() => {
            void setExplorerCollapsed(false);
          }}
        >
          <ChevronsRight class="size-4" strokeWidth={explorerIconStrokeWidth} />
        </button>
      </div>
    </div>
  {/if}

  <div
    class="notes-project-explorer-content flex h-full min-h-0 flex-col"
    inert={!mobileLayout && explorerCollapsed}
  >
    <div class="explorer-toolbar">
      <button
        type="button"
        class="explorer-icon"
        aria-label={t("notes.newPage")}
        data-app-tooltip={t("notes.newPage")}
        disabled={!projectId}
        onclick={() => createPage()}
      >
        <SquarePen class="size-4" strokeWidth={explorerIconStrokeWidth} />
      </button>
    <button
      type="button"
      class="explorer-icon"
      aria-label={t("notes.newFolder")}
      data-app-tooltip={t("notes.newFolder")}
      disabled={!projectId}
      onclick={() => {
        void createFolder();
      }}
    >
      <FolderPlus class="size-4" strokeWidth={explorerIconStrokeWidth} />
    </button>
    <div class="relative">
      <button
        bind:this={sortButtonElement}
        type="button"
        class="explorer-icon"
        class:explorer-icon-active={sortMenuOpen}
        aria-label={t("notes.sortExplorer")}
        aria-expanded={sortMenuOpen}
        aria-haspopup="menu"
        data-app-tooltip={t("notes.sortExplorer")}
        onclick={toggleSortMenu}
      >
        <ArrowUpNarrowWide class="size-4" strokeWidth={explorerIconStrokeWidth} />
      </button>
      {#if sortMenuOpen}
        <div
          bind:this={sortMenuElement}
          class="surface-floating fixed z-60 flex flex-col overflow-hidden"
          style={sortMenuStyle}
          role="menu"
          tabindex="-1"
          aria-label={t("notes.sortExplorer")}
          onmouseleave={() => {
            hoveredSortOrder = null;
          }}
        >
          <div class="surface-floating-body flex min-h-0 flex-1 flex-col overflow-y-auto" use:scrollEdgeFadeAction>
            <button
              type="button"
              class={sortOptionClass("name-asc")}
              role="menuitemradio"
              aria-checked={sortOrder === "name-asc"}
              onmouseenter={() => {
                hoveredSortOrder = "name-asc";
              }}
              onclick={() => selectSortOrder("name-asc")}
            >
              <span>{t("notes.sortNameAscending")}</span>
              {#if sortOrder === "name-asc"}<Check class="size-4" />{/if}
            </button>
            <button
              type="button"
              class={sortOptionClass("name-desc")}
              role="menuitemradio"
              aria-checked={sortOrder === "name-desc"}
              onmouseenter={() => {
                hoveredSortOrder = "name-desc";
              }}
              onclick={() => selectSortOrder("name-desc")}
            >
              <span>{t("notes.sortNameDescending")}</span>
              {#if sortOrder === "name-desc"}<Check class="size-4" />{/if}
            </button>
            <div
              class="menu-separator"
              role="separator"
              onmousemove={(event) => highlightNearestSortOption(event, "name-desc", "modified-desc")}
            ></div>
            <button
              type="button"
              class={sortOptionClass("modified-desc")}
              role="menuitemradio"
              aria-checked={sortOrder === "modified-desc"}
              onmouseenter={() => {
                hoveredSortOrder = "modified-desc";
              }}
              onclick={() => selectSortOrder("modified-desc")}
            >
              <span>{t("notes.sortModifiedDescending")}</span>
              {#if sortOrder === "modified-desc"}<Check class="size-4" />{/if}
            </button>
            <button
              type="button"
              class={sortOptionClass("modified-asc")}
              role="menuitemradio"
              aria-checked={sortOrder === "modified-asc"}
              onmouseenter={() => {
                hoveredSortOrder = "modified-asc";
              }}
              onclick={() => selectSortOrder("modified-asc")}
            >
              <span>{t("notes.sortModifiedAscending")}</span>
              {#if sortOrder === "modified-asc"}<Check class="size-4" />{/if}
            </button>
            <div
              class="menu-separator"
              role="separator"
              onmousemove={(event) => highlightNearestSortOption(event, "modified-asc", "created-desc")}
            ></div>
            <button
              type="button"
              class={sortOptionClass("created-desc")}
              role="menuitemradio"
              aria-checked={sortOrder === "created-desc"}
              onmouseenter={() => {
                hoveredSortOrder = "created-desc";
              }}
              onclick={() => selectSortOrder("created-desc")}
            >
              <span>{t("notes.sortCreatedDescending")}</span>
              {#if sortOrder === "created-desc"}<Check class="size-4" />{/if}
            </button>
            <button
              type="button"
              class={sortOptionClass("created-asc")}
              role="menuitemradio"
              aria-checked={sortOrder === "created-asc"}
              onmouseenter={() => {
                hoveredSortOrder = "created-asc";
              }}
              onclick={() => selectSortOrder("created-asc")}
            >
              <span>{t("notes.sortCreatedAscending")}</span>
              {#if sortOrder === "created-asc"}<Check class="size-4" />{/if}
            </button>
          </div>
        </div>
      {/if}
    </div>
    {#if !mobileLayout}
      <button
        type="button"
        class="explorer-icon"
        aria-label={t("notes.highlightCurrentFile")}
        data-app-tooltip={t("notes.highlightCurrentFile")}
        disabled={!notes.selectedPageId}
        onclick={() => {
          void highlightCurrentFile();
        }}
      >
        <FileQuestionMark class="size-4" strokeWidth={explorerIconStrokeWidth} />
      </button>
    {/if}
    <button
      type="button"
      class="explorer-icon"
      aria-label={shouldExpandExplorer ? t("notes.expandExplorer") : t("notes.collapseExplorer")}
      data-app-tooltip={shouldExpandExplorer ? t("notes.expandExplorer") : t("notes.collapseExplorer")}
      onclick={toggleExplorerExpansion}
    >
      {#if shouldExpandExplorer}
        <ChevronsUpDown class="size-4" strokeWidth={explorerIconStrokeWidth} />
      {:else}
        <ChevronsDownUp class="size-4" strokeWidth={explorerIconStrokeWidth} />
      {/if}
    </button>
    <button
      type="button"
      class="explorer-icon"
      class:explorer-icon-active={searchOpen}
      bind:this={searchButton}
      aria-label={t("notes.searchLabel")}
      aria-pressed={searchOpen}
      data-app-tooltip={t("notes.searchLabel")}
      onclick={toggleSearch}
    >
      <Search class="size-4" strokeWidth={explorerIconStrokeWidth} />
    </button>
    {#if !mobileLayout}
      <button
        bind:this={collapseExplorerButtonElement}
        type="button"
        class="explorer-icon ml-auto"
        aria-label={t("notes.collapseExplorerSidebar")}
        aria-expanded="true"
        data-app-tooltip={t("notes.collapseExplorerSidebar")}
        onclick={() => {
          void setExplorerCollapsed(true);
        }}
      >
        <ChevronsLeft class="size-4" strokeWidth={explorerIconStrokeWidth} />
      </button>
    {/if}
  </div>

  {#if searchOpen}
    <SearchField bind:value={search} label={t("notes.searchLabel")} placeholder={t("notes.searchPlaceholder")} clearLabel={t("notes.clearSearch")} onClose={toggleSearch} />
  {/if}

  {#if folderActionError}
    <Notice class="shrink-0 px-3 py-2 text-[0.8rem]" message={t("notes.folderActionFailed", folderActionError)} />
  {/if}
  {#if notes.trashActionError}
    <Notice class="shrink-0 px-3 py-2 text-[0.8rem]" message={t("notes.trashFailed", notes.trashActionError)} />
  {/if}
  {#if navigationDragError}
    <Notice class="shrink-0 px-3 py-2 text-[0.8rem]" message={t("notes.navigationMoveFailed", navigationDragError)} />
  {/if}

  <div
    bind:this={explorerScrollElement}
    class="explorer-list min-h-0 flex-1 overflow-x-auto pb-2"
    class:notes-navigation-root-drop-valid={navigationDropTargetKey === "root" && navigationDropAllowed}
    class:notes-navigation-root-drop-invalid={navigationDropTargetKey === "root" && !navigationDropAllowed}
    role="region"
    aria-label={t("notes.explorerLabel")}
    data-notes-explorer-scroll
    onscroll={handleWorkspaceScroll}
    onpointerdowncapture={handleExplorerNavigationPointerDown}
    onclickcapture={suppressNavigationClick}
    use:resetCreationLocationOnBlankPointer
  >
    {#if notes.loadError}
      <Notice class="px-1 py-2 text-[0.8rem]" message={t("notes.loadFailed", notes.loadError)} />
    {/if}
    {#if notes.loaded && treeItems.length === 0 && (mobileLayout || !workingMarkdownHasMatches) && search.trim()}
      <div class="px-1 py-2 text-[0.8rem] text-muted-foreground">
        {t("notes.noSearchResults")}
      </div>
    {:else if mobileLayout && notes.loaded && treeItems.length === 0}
      <div class="flex min-h-48 flex-col items-center justify-center gap-3 px-4 py-8 text-center">
        <p class="text-sm text-muted-foreground">{t("notes.noPages")}</p>
        <button
          type="button"
          class="min-h-12 rounded-xl bg-primary px-5 text-sm font-semibold text-primary-foreground active:opacity-85 disabled:opacity-45"
          disabled={!projectId}
          onclick={() => createPage()}
        >
          {t("notes.newPage")}
        </button>
      </div>
    {:else}
      {#each explorerItems as item (item.key)}
        {@const folderDropArea = navigationFolderDropArea.get(item.key)}
        {@const itemDropTarget = explorerItemDropTarget(item)}
        <div
          role="group"
          data-notes-navigation-drop-kind={itemDropTarget.kind}
          data-notes-navigation-drop-id={itemDropTarget.kind === "folder" ? itemDropTarget.id : undefined}
          data-notes-navigation-drag-kind={item.kind}
          data-notes-navigation-drag-id={item.kind === "folder" ? item.folder.id : item.page.id}
          class:notes-navigation-folder-drop-area={Boolean(folderDropArea)}
          class:notes-navigation-folder-drop-area-valid={folderDropArea?.state === "valid"}
          class:notes-navigation-folder-drop-area-invalid={folderDropArea?.state === "invalid"}
          class:notes-navigation-folder-drop-area-single={folderDropArea?.position === "single"}
          class:notes-navigation-folder-drop-area-start={folderDropArea?.position === "start"}
          class:notes-navigation-folder-drop-area-end={folderDropArea?.position === "end"}
        >
        {#if item.kind === "folder"}
          <NotesFolderRow
            folder={item.folder}
            depth={item.depth}
            collapsed={item.collapsed}
            renameRequestId={folderRenameTargetId === item.folder.id ? folderRenameRequestId : 0}
            moveTargets={folderMoveTargets(item.folder)}
            navigationDragging={draggingNavigationItem?.kind === "folder" && draggingNavigationItem.id === item.folder.id}
            onActivate={() => {
              onCreationFolderChange(item.folder.id);
            }}
            onToggleCollapsed={(collapsed) => {
              notes.setFolderCollapsed(item.folder.id, collapsed);
            }}
            onCreatePage={() => createPage(item.folder.id)}
            onCreateFolder={() => {
              void createFolder(item.folder.id);
            }}
            onRename={(name) => {
              return renameFolder(item.folder.id, name);
            }}
            onMove={(parentFolderId) => {
              void moveFolder(item.folder.id, parentFolderId);
            }}
            onDelete={() => {
              pendingDeleteFolder = item.folder;
            }}
          />
        {:else}
          <NotesPageRow
            page={item.page}
            displayTitle={notes.pageTitleDraftForPage(item.page.id) ?? undefined}
            depth={item.depth}
            hasChildren={item.hasChildren}
            collapsed={item.collapsed}
            parentStatus={item.parentStatus}
            favorited={notes.favoritePageIds.includes(item.page.id)}
            selected={item.page.id === selectedRootPageId}
            showDisclosure={false}
            highlightRequestId={item.page.id === selectedRootPageId ? currentFileHighlightRequestId : 0}
            navigationDragging={draggingNavigationItem?.kind === "page" && draggingNavigationItem.id === item.page.id}
            onSelect={() => {
              onCreationFolderChange(notesPageContainingFolderId(item.page.id, projectPages));
              selectPrimaryPage(item.page.id);
            }}
            onRename={(title) => {
              return renamePage(item.page.id, title);
            }}
            onToggleFavorite={(favorited) => {
              notes.setPageFavorited(item.page.id, favorited);
            }}
            onCreateChild={() => {
              createSubpage(item.page.id);
            }}
            onDuplicate={() => {
              duplicatePage(item.page);
            }}
            moveTargets={pageMoveTargets(item.page)}
            onRequestMoveTargets={() => notes.ensureOptionalSubsystem("destinations")}
            onMove={(parent) => {
              void notes.movePage(item.page.id, parent);
            }}
            folderMoveTargets={pageFolderMoveTargets(item.page)}
            onMoveToFolder={(folderId) => {
              void notes.movePageToFolder(item.page.id, folderId);
            }}
            onArchive={() => {
              pendingArchivePage = item.page;
            }}
            onTrash={() => {
              pendingTrashPage = item.page;
            }}
          />
        {/if}
        </div>
      {/each}
    {/if}
    {#if !mobileLayout}
      <NotesWorkingMarkdownTree
        tree={workingMarkdownTree}
        query={search}
        selectedFile={selectedWorkingMarkdownFile}
        loading={workingMarkdownLoading}
        error={workingMarkdownError}
        onSelect={onSelectWorkingMarkdownFile}
        onRefresh={() => { void refreshWorkingMarkdown(); }}
      />
    {/if}
  </div>
  </div>

</aside>

{#if pendingArchivePage || pendingTrashPage || pendingDeleteFolder}
  {#if confirmDialogLoadState?.status === "ready" && confirmDialogLoadState.component.kind === "confirm-dialog"}
    {@const ConfirmDialog = confirmDialogLoadState.component.component}
    {#if pendingArchivePage}
      <ConfirmDialog
        title={t("notes.archiveConfirmTitle", notesPageTitle(pendingArchivePage, t("notes.untitled")))}
        message={t("notes.archiveConfirmMessage")}
        confirmLabel={t("notes.archiveConfirm")}
        cancelLabel={t("common.cancel")}
        onConfirm={confirmArchivePage}
        onCancel={() => {
          pendingArchivePage = null;
        }}
      />
    {:else if pendingTrashPage}
      <ConfirmDialog
        title={t("notes.trashConfirmTitle", notesPageTitle(pendingTrashPage, t("notes.untitled")))}
        message={t("notes.trashConfirmMessage")}
        confirmLabel={t("notes.trashConfirm")}
        cancelLabel={t("common.cancel")}
        onConfirm={confirmTrashPage}
        onCancel={() => {
          pendingTrashPage = null;
        }}
      />
    {:else if pendingDeleteFolder}
      <ConfirmDialog
        title={t("notes.deleteFolderConfirmTitle", pendingDeleteFolder.name)}
        message={t("notes.deleteFolderConfirmMessage")}
        confirmLabel={t("notes.deleteFolderConfirm")}
        cancelLabel={t("common.cancel")}
        onConfirm={() => {
          void confirmDeleteFolder();
        }}
        onCancel={() => {
          pendingDeleteFolder = null;
        }}
      />
    {/if}
  {:else if confirmDialogLoadState?.status === "failed"}
    <div class="fixed inset-0 z-100 flex items-center justify-center surface-backdrop p-4" role="dialog" aria-modal="true">
      <div class="surface-dialog p-4 text-sm">
        <Notice message={t("common.viewLoadFailed", t("common.confirm"))} copy={false} onRetry={() => requestConfirmDialog(true)}>
          {#snippet actions()}<button type="button" class="notice-action-text" onclick={clearPendingConfirmation}>{t("common.cancel")}</button>{/snippet}
        </Notice>
      </div>
    </div>
  {:else}
    <div class="fixed inset-0 z-100 flex items-center justify-center surface-backdrop p-4" role="dialog" aria-modal="true" aria-busy="true">
      <Loading layout="fill" />
    </div>
  {/if}
{/if}

<style>
  .notes-project-explorer {
    width: var(--explorer-width);
    transition: width 180ms cubic-bezier(0.2, 0, 0, 1);
  }

  .notes-project-explorer-content {
    width: var(--explorer-width);
    min-width: var(--explorer-width);
  }

  .notes-project-explorer-mobile {
    width: 100%;
    min-width: 0;
    flex: 1 1 0%;
  }

  .notes-project-explorer-mobile .notes-project-explorer-content {
    width: 100%;
    min-width: 0;
  }

  .notes-project-explorer-mobile :global(.notes-page-row-content),
  .notes-project-explorer-mobile :global(.notes-folder-row-content),
  .notes-project-explorer-mobile :global(.notes-page-row-content > button),
  .notes-project-explorer-mobile :global(.notes-folder-row-content > button),
  .notes-project-explorer-mobile :global(.notes-page-action-menu > button),
  .notes-project-explorer-mobile :global(.notes-folder-action-menu > button) {
    min-height: var(--touch-target-min);
  }

  .notes-project-explorer-collapsed {
    width: 2.75rem;
  }

  .notes-project-explorer-collapsed-rail {
    background-color: var(--explorer-background);
    opacity: 0;
    pointer-events: none;
    transition: opacity 60ms linear;
  }

  .notes-project-explorer-collapsed .notes-project-explorer-collapsed-rail {
    opacity: 1;
    pointer-events: auto;
    transition-delay: 100ms;
  }

  .notes-navigation-folder-drop-area {
    border-radius: 0;
  }

  .notes-navigation-folder-drop-area-valid {
    background: var(--selection-background);
  }

  .notes-navigation-folder-drop-area-invalid {
    background: color-mix(in oklab, var(--accent) 58%, transparent);
  }

  .notes-navigation-root-drop-valid {
    background: color-mix(in oklab, var(--selection-background) 55%, transparent);
  }

  .notes-navigation-root-drop-invalid {
    background: color-mix(in oklab, var(--accent) 36%, transparent);
  }

  .notes-navigation-folder-drop-area-single {
    border-radius: 0.375rem;
  }

  .notes-navigation-folder-drop-area-start {
    border-radius: 0.375rem 0.375rem 0 0;
  }

  .notes-navigation-folder-drop-area-end {
    border-radius: 0 0 0.375rem 0.375rem;
  }

  .notes-project-explorer-dragging,
  .notes-project-explorer-dragging :global(*) {
    cursor: grabbing !important;
    user-select: none;
  }

  :global(html.notes-navigation-pointer-dragging),
  :global(html.notes-navigation-pointer-dragging *) {
    cursor: grabbing !important;
    user-select: none;
  }

  .notes-project-explorer-dragging :global(.notes-folder-row-content:hover),
  .notes-project-explorer-dragging :global(.notes-page-row-content:hover) {
    background-color: transparent;
  }

  @media (prefers-reduced-motion: reduce) {
    .notes-project-explorer,
    .notes-project-explorer-collapsed-rail {
      transition: none;
    }
  }
</style>
