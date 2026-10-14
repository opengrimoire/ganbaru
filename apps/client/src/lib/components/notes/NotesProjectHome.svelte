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
    type NotesNavigationTreeItem,
  } from "$lib/notes/navigation/tree";
  import {
    resolveNotesNavigationDrop,
    type NotesNavigationDragItem,
    type NotesNavigationDropResolution,
    type NotesNavigationDropTarget,
  } from "$lib/notes/navigation/drag";
  import {
    notesNavigationDragCard,
    notesNavigationDragCardOrigin,
    notesNavigationSubtreeEnd,
    type NotesNavigationDragCardGeometry,
    type NotesNavigationDragCardPhase,
    type NotesNavigationPoint,
    type NotesNavigationRect,
  } from "$lib/notes/navigation/drag-motion";
  import { notesPageMoveTargets } from "$lib/notes/pages/move";
  import {
    SIDEBAR_SORT_ORDER_LABEL_KEYS,
    SIDEBAR_SORT_ORDERS,
    type SidebarSortOrder,
  } from "$lib/utils/sidebar-order";
  import { notesPageContainingFolderId } from "$lib/notes/navigation/hierarchy-menu";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import { notesPagesForProject } from "$lib/notes/project-membership";
  import type {
    NotesFolder,
    NotesPage,
    NotesPageIcon,
    NotesWorkingMarkdownFileRef,
    NotesWorkingMarkdownTreeRead,
  } from "$lib/notes/types";
  import { getNotes } from "$lib/stores/notes.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { scrollOverflowAction } from "$lib/utils/scroll-overflow";
  import { suppressHoverOnToggle } from "$lib/utils/hover-suppression";
  import NotesFolderRow from "$lib/components/notes/navigation/NotesFolderRow.svelte";
  import NotesNavigationDragCard from "$lib/components/notes/navigation/NotesNavigationDragCard.svelte";
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
  const preferences = getPreferences();
  const { t } = getLocalization();
  const explorerIconStrokeWidth = 1.5;
  const sortMenuViewportGapPx = 8;
  const sortMenuTriggerGapPx = 4;
  const navigationFolderOpenDelayMs = 520;
  const navigationAutoScrollEdgePx = 36;
  const navigationAutoScrollStepPx = 12;
  const navigationPointerDragThresholdPx = 5;
  const navigationDragSettleMs = 200;
  const navigationLayoutShiftMs = 180;
  const navigationRowContentSelector = ".notes-folder-row-content, .notes-page-row-content";

  interface PendingNavigationPointerDrag {
    pointerId: number;
    startX: number;
    startY: number;
    item: NotesNavigationDragItem;
    sourceElement: HTMLElement;
  }

  type NavigationPointerPosition = NotesNavigationPoint;

  /** The lifted row while it follows the pointer and while it settles after release. */
  interface NavigationDragCardState {
    id: number;
    item: NotesNavigationDragItem;
    title: string;
    icon: NotesPageIcon | null;
    geometry: NotesNavigationDragCardGeometry;
    frame: NotesNavigationRect;
    phase: NotesNavigationDragCardPhase;
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
    navigationDropTarget = null;
    navigationDropResolution = null;
  }

  function clearNavigationAutoScrollFrame(): void {
    if (navigationAutoScrollFrame === null) return;
    window.cancelAnimationFrame(navigationAutoScrollFrame);
    navigationAutoScrollFrame = null;
  }

  /** Stop following the pointer. The lifted card stays until it settles. */
  function stopNavigationPointerTracking(): void {
    clearNavigationAutoScrollFrame();
    clearNavigationDropTarget();
    pendingNavigationPointerDrag = null;
    navigationPointerPosition = null;
    draggingNavigationItem = null;
  }

  function prefersReducedMotion(): boolean {
    return typeof window.matchMedia === "function"
      && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  function viewportRect(element: Element): NotesNavigationRect {
    const bounds = element.getBoundingClientRect();
    return { left: bounds.left, top: bounds.top, width: bounds.width, height: bounds.height };
  }

  function explorerItemMatches(item: NotesNavigationTreeItem, dragItem: NotesNavigationDragItem): boolean {
    if (item.kind !== dragItem.kind) return false;
    return (item.kind === "folder" ? item.folder.id : item.page.id) === dragItem.id;
  }

  function navigationRowContent(item: NotesNavigationDragItem): HTMLElement | null {
    const wrapper = explorerScrollElement?.querySelector<HTMLElement>(
      `[data-notes-navigation-drag-kind="${item.kind}"][data-notes-navigation-drag-id="${CSS.escape(item.id)}"]`,
    );
    return wrapper?.querySelector<HTMLElement>(navigationRowContentSelector) ?? null;
  }

  function navigationDragCardDetails(
    item: NotesNavigationDragItem,
  ): { title: string; icon: NotesPageIcon | null } | null {
    if (item.kind === "folder") {
      const folder = projectFolders.find((candidate) => candidate.id === item.id);
      return folder ? { title: folder.name, icon: null } : null;
    }
    const page = projectPages.find((candidate) => candidate.id === item.id);
    if (!page) return null;
    return {
      title: notes.pageTitleDraftForPage(page.id) ?? notesPageTitle(page, t("notes.untitled")),
      icon: page.icon,
    };
  }

  function clearNavigationDragCardTimer(): void {
    if (navigationDragCardTimer === null) return;
    window.clearTimeout(navigationDragCardTimer);
    navigationDragCardTimer = null;
  }

  function liftNavigationDragCard(pending: PendingNavigationPointerDrag): void {
    const content = pending.sourceElement.querySelector<HTMLElement>(navigationRowContentSelector);
    const details = navigationDragCardDetails(pending.item);
    if (!content || !details) return;
    const pointer = { x: pending.startX, y: pending.startY };
    const source = viewportRect(content);
    const geometry = notesNavigationDragCard(
      source,
      pointer,
      explorerScrollElement?.clientWidth ?? source.width,
    );
    const origin = notesNavigationDragCardOrigin(geometry, pointer);
    clearNavigationDragCardTimer();
    navigationDragCardSequence += 1;
    navigationDragCard = {
      id: navigationDragCardSequence,
      item: pending.item,
      ...details,
      geometry,
      frame: { left: origin.x, top: origin.y, width: geometry.width, height: geometry.height },
      phase: "lifted",
    };
  }

  function followNavigationDragCard(position: NavigationPointerPosition): void {
    const card = navigationDragCard;
    if (!card || card.phase !== "lifted") return;
    const origin = notesNavigationDragCardOrigin(card.geometry, position);
    card.frame.left = origin.x;
    card.frame.top = origin.y;
  }

  /**
   * End the card's flight, then hand the row back to the list. `settle` glides the
   * card onto `frame` and takes its size; `absorb` drifts it toward `frame` while fading, for a row that went into a
   * collapsed folder; `fade` fades it where it was released.
   */
  function finishNavigationDragCard(
    cardId: number,
    landing: { kind: "settle" | "absorb"; frame: NotesNavigationRect } | { kind: "fade" },
  ): void {
    const card = navigationDragCard;
    if (!card || card.id !== cardId) return;
    if (landing.kind === "settle") card.frame = landing.frame;
    if (landing.kind === "absorb") card.frame = { ...card.frame, left: landing.frame.left, top: landing.frame.top };
    card.phase = landing.kind === "settle" ? "settling" : "vanishing";
    clearNavigationDragCardTimer();
    navigationDragCardTimer = window.setTimeout(() => {
      navigationDragCardTimer = null;
      if (navigationDragCard?.id !== cardId) return;
      navigationDragCard = null;
    }, prefersReducedMotion() ? 0 : navigationDragSettleMs);
  }

  function returnNavigationDragCard(): void {
    const card = navigationDragCard;
    if (!card) return;
    const source = navigationRowContent(card.item);
    finishNavigationDragCard(card.id, source ? { kind: "settle", frame: viewportRect(source) } : { kind: "fade" });
  }

  /** Land the card on the moved row, or on the folder that now hides it. */
  function landNavigationDragCard(cardId: number, source: NotesNavigationDragItem, target: NotesNavigationDropTarget): void {
    const destination = navigationRowContent(source);
    if (destination) {
      destination.scrollIntoView({ block: "nearest" });
      finishNavigationDragCard(cardId, { kind: "settle", frame: viewportRect(destination) });
      return;
    }
    const folderRow = target.kind === "folder" ? navigationRowContent({ kind: "folder", id: target.id }) : null;
    finishNavigationDragCard(cardId, folderRow ? { kind: "absorb", frame: viewportRect(folderRow) } : { kind: "fade" });
  }

  function navigationRowTops(): Map<string, number> {
    const tops = new Map<string, number>();
    for (const row of explorerScrollElement?.querySelectorAll<HTMLElement>("[data-notes-navigation-item-key]") ?? []) {
      const key = row.dataset.notesNavigationItemKey;
      if (key) tops.set(key, row.getBoundingClientRect().top);
    }
    return tops;
  }

  /**
   * Slide rows from where they were to where the list now puts them, so opening a folder or landing a drop makes room
   * instead of jumping. `skipKey` names the row the card is gliding onto.
   */
  function shiftNavigationRows(previousTops: ReadonlyMap<string, number>, skipKey: string | null = null): void {
    if (prefersReducedMotion()) return;
    for (const row of explorerScrollElement?.querySelectorAll<HTMLElement>("[data-notes-navigation-item-key]") ?? []) {
      const key = row.dataset.notesNavigationItemKey;
      const previousTop = key ? previousTops.get(key) : undefined;
      if (previousTop === undefined || key === skipKey || typeof row.animate !== "function") continue;
      const offset = previousTop - row.getBoundingClientRect().top;
      if (Math.abs(offset) < 1) continue;
      row.animate(
        [{ transform: `translateY(${offset}px)` }, { transform: "none" }],
        { duration: navigationLayoutShiftMs, easing: "cubic-bezier(0.2, 0, 0, 1)" },
      );
    }
  }

  async function openNavigationFolderDuringDrag(folderId: string): Promise<void> {
    const previousTops = navigationRowTops();
    notes.setFolderCollapsed(folderId, false);
    await tick();
    shiftNavigationRows(previousTops);
  }

  async function dropNavigationItem(
    source: NotesNavigationDragItem,
    target: NotesNavigationDropTarget,
  ): Promise<void> {
    const cardId = navigationDragCard?.id ?? null;
    const previousTops = navigationRowTops();
    try {
      await moveNavigationItem(source, target);
    } catch (error: unknown) {
      navigationDragError = error instanceof Error ? error.message : String(error);
      returnNavigationDragCard();
      return;
    }
    await tick();
    const destinationKey = explorerItems.find((item) => explorerItemMatches(item, source))?.key ?? null;
    shiftNavigationRows(previousTops, destinationKey);
    if (cardId !== null) landNavigationDragCard(cardId, source, target);
  }

  function updateNavigationDropTarget(target: NotesNavigationDropTarget | null): void {
    const source = draggingNavigationItem;
    if (!source || !target) {
      clearNavigationDropTarget();
      return;
    }
    const resolution = resolveNotesNavigationDrop(source, target, projectPages, projectFolders);
    const previousKey = navigationDropTarget ? navigationTargetKey(navigationDropTarget) : null;
    if (navigationTargetKey(target) === previousKey && resolution === navigationDropResolution) return;
    navigationDropTarget = target;
    navigationDropResolution = resolution;
    scheduleFolderOpen(target, resolution === "move");
  }

  function updateNavigationDropTargetAtPointer(): void {
    const position = navigationPointerPosition;
    if (!position) return;
    updateNavigationDropTarget(explorerDropTargetAtPoint(position.x, position.y));
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
    liftNavigationDragCard(pending);
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
      void openNavigationFolderDuringDrag(target.id);
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
    navigationClickSuppressedUntilPointerUp = false;
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
    followNavigationDragCard(position);
    updateNavigationDropTargetAtPointer();
    syncNavigationAutoScroll();
  }

  function suppressNavigationClick(event: MouseEvent): void {
    if (!suppressNextNavigationClick) return;
    suppressNextNavigationClick = false;
    event.preventDefault();
    event.stopPropagation();
  }

  /** Swallow the click that follows the release of a drag, so dropping a row never also opens it. */
  function suppressClickAfterPointerUp(): void {
    suppressNextNavigationClick = true;
    window.setTimeout(() => {
      suppressNextNavigationClick = false;
    }, 0);
  }

  function handleNavigationPointerUp(event: PointerEvent): void {
    if (navigationClickSuppressedUntilPointerUp) {
      navigationClickSuppressedUntilPointerUp = false;
      suppressClickAfterPointerUp();
      return;
    }
    const pending = pendingNavigationPointerDrag;
    if (!pending || pending.pointerId !== event.pointerId) return;
    const source = draggingNavigationItem;
    pendingNavigationPointerDrag = null;
    if (!source) return;
    event.preventDefault();
    const target = explorerDropTargetAtPoint(event.clientX, event.clientY);
    const resolution = target
      ? resolveNotesNavigationDrop(source, target, projectPages, projectFolders)
      : null;
    stopNavigationPointerTracking();
    suppressClickAfterPointerUp();
    if (!target || resolution !== "move") {
      returnNavigationDragCard();
      return;
    }
    void dropNavigationItem(source, target);
  }

  /**
   * Put the lifted row back. Escape keeps swallowing the click of the release that is still to come; a lost pointer
   * or window has no release to wait for.
   */
  function cancelNavigationPointerDrag(awaitPointerUp = false): void {
    if (!pendingNavigationPointerDrag && !draggingNavigationItem) return;
    const wasDragging = draggingNavigationItem !== null;
    stopNavigationPointerTracking();
    if (!wasDragging) return;
    navigationClickSuppressedUntilPointerUp = awaitPointerUp;
    returnNavigationDragCard();
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
  type SortMenuOption = "global" | SidebarSortOrder;
  const sortOrder = $derived(notes.sidebarOrderForProject(projectId));
  const sortOrderOverride = $derived(notes.sidebarOrderOverride(projectId));
  const sortMenuOptions = $derived<readonly SortMenuOption[]>(
    projectId ? ["global", ...SIDEBAR_SORT_ORDERS] : SIDEBAR_SORT_ORDERS,
  );
  let currentFileHighlightRequestId = $state(0);
  let hoveredSortOrder = $state<SortMenuOption | null>(null);
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
  let navigationDropTarget = $state.raw<NotesNavigationDropTarget | null>(null);
  let navigationDropResolution = $state<NotesNavigationDropResolution | null>(null);
  let navigationFolderOpenTimer: number | null = null;
  let navigationAutoScrollFrame: number | null = null;
  let navigationDragCard = $state<NavigationDragCardState | null>(null);
  let navigationDragCardSequence = 0;
  let navigationDragCardTimer: number | null = null;
  let navigationClickSuppressedUntilPointerUp = false;
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
      favoriteFolderIds: notes.favoriteFolderIds,
      favoritePageIds: notes.favoritePageIds,
      titleForPage: (page) => notesPageTitle(page, t("notes.untitled")),
    })
  );
  const explorerItems = $derived(treeItems);
  const workingMarkdownHasMatches = $derived(
    buildWorkingMarkdownTreeItems(workingMarkdownTree.roots, new Set(), search).length > 0,
  );
  const explorerItemDepths = $derived(explorerItems.map((item) => item.depth));
  const navigationFolderDropArea = $derived.by(() => {
    const result = new Map<string, {
      state: "valid" | "invalid";
      position: "single" | "start" | "middle" | "end";
    }>();
    const target = navigationDropTarget;
    if (!draggingNavigationItem || target?.kind !== "folder") return result;
    if (navigationDropResolution === null || navigationDropResolution === "current") return result;
    const startIndex = explorerItems.findIndex(
      (item) => item.kind === "folder" && item.folder.id === target.id,
    );
    if (startIndex < 0) return result;
    const endIndex = notesNavigationSubtreeEnd(explorerItemDepths, startIndex);
    const state = navigationDropResolution === "move" ? "valid" : "invalid";
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
  /** Rows carried by the lifted card: the dragged row is `held`, and rows nested under a dragged folder ride along. */
  const navigationDragRows = $derived.by(() => {
    const result = new Map<string, "held" | "carried">();
    const card = navigationDragCard;
    if (!card) return result;
    const dragItem = card.item;
    const startIndex = explorerItems.findIndex((item) => explorerItemMatches(item, dragItem));
    if (startIndex < 0) return result;
    const endIndex = notesNavigationSubtreeEnd(explorerItemDepths, startIndex);
    for (let index = startIndex; index <= endIndex; index += 1) {
      const item = explorerItems[index];
      if (item) result.set(item.key, index === startIndex ? "held" : "carried");
    }
    return result;
  });
  const navigationDragHint = $derived.by(() => {
    const target = navigationDropTarget;
    if (!target || navigationDropResolution !== "move") return null;
    if (target.kind === "root") return t("notes.navigationDragMoveTo", t("notes.projectRoot"));
    if (target.kind !== "folder") return null;
    const folder = projectFolders.find((candidate) => candidate.id === target.id);
    return folder ? t("notes.navigationDragMoveTo", folder.name) : null;
  });
  /**
   * The held row dims to mark where it came from while lifted and hides while the card settles onto it. Rows nested
   * under a dragged folder stay dimmed throughout.
   */
  function navigationDragRowState(key: string): "source" | "placeholder" | null {
    const role = navigationDragRows.get(key);
    if (!role) return null;
    return role === "held" && navigationDragCard?.phase !== "lifted" ? "placeholder" : "source";
  }

  $effect(() => clearNavigationDragCardTimer);

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

  const observeExplorerCollapsedForHover = suppressHoverOnToggle();
  $effect(() => observeExplorerCollapsedForHover(explorerCollapsed));

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

  /** Write the chosen order to the project, or to the global setting when no project is open. */
  function selectSortOrder(option: SortMenuOption): void {
    if (projectId) notes.setProjectSidebarOrder(projectId, option === "global" ? null : option);
    else if (option !== "global") preferences.setNotesSidebarOrder(option);
    sortMenuOpen = false;
  }

  function sortOptionChecked(option: SortMenuOption): boolean {
    if (option === "global") return sortOrderOverride === null;
    return projectId ? sortOrderOverride === option : sortOrder === option;
  }

  function sortOptionLabel(option: SortMenuOption): string {
    return option === "global" ? t("common.useGlobalSetting") : t(SIDEBAR_SORT_ORDER_LABEL_KEYS[option]);
  }

  /** Separate the global item from the orders, and the orders into name, modified, and created pairs. */
  function sortOptionStartsGroup(option: SortMenuOption, index: number): boolean {
    if (index === 0 || option === "global") return false;
    return sortMenuOptions[index - 1] === "global" || SIDEBAR_SORT_ORDERS.indexOf(option) % 2 === 0;
  }

  function sortOptionClass(option: SortMenuOption): string {
    return cn("menu-item grid grid-cols-[max-content_1rem]", hoveredSortOrder === option && "bg-accent");
  }

  function highlightNearestSortOption(
    event: MouseEvent,
    before: SortMenuOption,
    after: SortMenuOption,
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
    if (event.key !== "Escape") return;
    if (draggingNavigationItem) {
      event.preventDefault();
      cancelNavigationPointerDrag(true);
      return;
    }
    if (sortMenuOpen) sortMenuOpen = false;
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
  onpointercancel={() => cancelNavigationPointerDrag()}
  onblur={() => cancelNavigationPointerDrag()}
  onkeydown={handleWindowKeydown}
  onresize={handleWindowResize}
  onwheel={handleNavigationDragWheel}
/>

<aside
  class="explorer-sidebar notes-project-explorer relative h-full min-h-0 shrink-0 overflow-hidden"
  class:explorer-touch={mobileLayout}
  class:notes-project-explorer-mobile={mobileLayout}
  class:explorer-dock={!mobileLayout}
  class:explorer-dock-collapsed={!mobileLayout && explorerCollapsed}
  class:notes-project-explorer-dragging={draggingNavigationItem !== null}
  aria-label={t("notes.explorerLabel")}
  data-notes-explorer
  data-notes-first-use-state
>
  {#if !mobileLayout}
    <div
      class="explorer-dock-rail"
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
    class="flex h-full min-h-0 flex-col"
    class:explorer-dock-content={!mobileLayout}
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
            {#each sortMenuOptions as option, index (option)}
              {#if sortOptionStartsGroup(option, index)}
                <div
                  class="menu-separator"
                  role="separator"
                  onmousemove={(event) => highlightNearestSortOption(event, sortMenuOptions[index - 1], option)}
                ></div>
              {/if}
              <button
                type="button"
                class={sortOptionClass(option)}
                role="menuitemradio"
                aria-checked={sortOptionChecked(option)}
                onmouseenter={() => {
                  hoveredSortOrder = option;
                }}
                onclick={() => selectSortOrder(option)}
              >
                <span>{sortOptionLabel(option)}</span>
                {#if sortOptionChecked(option)}<Check class="size-4" />{/if}
              </button>
            {/each}
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
    use:scrollOverflowAction
    class="explorer-list min-h-0 flex-1 overflow-x-auto pb-2"
    class:notes-navigation-root-drop-valid={navigationDropTarget?.kind === "root" && navigationDropResolution === "move"}
    class:notes-navigation-root-drop-invalid={navigationDropTarget?.kind === "root" && navigationDropResolution === "forbidden"}
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
          data-notes-navigation-item-key={item.key}
          class:notes-navigation-folder-drop-area={Boolean(folderDropArea)}
          class:notes-navigation-folder-drop-area-valid={folderDropArea?.state === "valid"}
          class:notes-navigation-folder-drop-area-invalid={folderDropArea?.state === "invalid"}
          class:notes-navigation-folder-drop-area-single={folderDropArea?.position === "single"}
          class:notes-navigation-folder-drop-area-start={folderDropArea?.position === "start"}
          class:notes-navigation-folder-drop-area-end={folderDropArea?.position === "end"}
          class:notes-navigation-drag-source={navigationDragRowState(item.key) === "source"}
          class:notes-navigation-drag-placeholder={navigationDragRowState(item.key) === "placeholder"}
        >
        {#if item.kind === "folder"}
          <NotesFolderRow
            folder={item.folder}
            favorited={notes.favoriteFolderIds.includes(item.folder.id)}
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
            onToggleFavorite={(favorited) => {
              notes.setFolderFavorited(item.folder.id, favorited);
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

{#if navigationDragCard}
  <NotesNavigationDragCard
    kind={navigationDragCard.item.kind}
    title={navigationDragCard.title}
    icon={navigationDragCard.icon}
    frame={navigationDragCard.frame}
    phase={navigationDragCard.phase}
    forbidden={navigationDropResolution === "forbidden"}
    hint={navigationDragHint}
    durationMs={navigationDragSettleMs}
  />
{/if}

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
  .notes-project-explorer-mobile {
    width: 100%;
    min-width: 0;
    flex: 1 1 0%;
  }

  .notes-project-explorer-mobile :global(.notes-page-row-content),
  .notes-project-explorer-mobile :global(.notes-folder-row-content),
  .notes-project-explorer-mobile :global(.notes-page-row-content > button),
  .notes-project-explorer-mobile :global(.notes-folder-row-content > button),
  .notes-project-explorer-mobile :global(.notes-page-action-menu > button),
  .notes-project-explorer-mobile :global(.notes-folder-action-menu > button) {
    min-height: var(--touch-target-min);
  }

  .notes-navigation-folder-drop-area {
    border-radius: 0;
  }

  .notes-navigation-folder-drop-area-valid {
    --notes-folder-drop-tint: var(--selection-background);
    background: var(--notes-folder-drop-tint);
  }

  .notes-navigation-folder-drop-area-invalid {
    --notes-folder-drop-tint: color-mix(in oklab, var(--accent) 58%, transparent);
    background: var(--notes-folder-drop-tint);
  }

  /* The selected row paints an opaque highlight, so the folder's drop tint is layered over it to keep the area uniform. */
  .notes-navigation-folder-drop-area :global(.explorer-row.explorer-selected) {
    background:
      linear-gradient(var(--notes-folder-drop-tint, transparent), var(--notes-folder-drop-tint, transparent)),
      var(--explorer-row-highlight);
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

  .notes-navigation-drag-source :global(.explorer-row) {
    opacity: 0.4;
    transition: opacity 150ms ease-out;
  }

  .notes-navigation-drag-placeholder :global(.explorer-row) {
    opacity: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    .notes-navigation-drag-source :global(.explorer-row) {
      transition: none;
    }
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
</style>
