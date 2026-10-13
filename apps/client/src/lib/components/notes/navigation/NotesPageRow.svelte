<script lang="ts">
  import EllipsisVertical from "@lucide/svelte/icons/ellipsis-vertical";
  import { onDestroy, tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    COMPACT_IDENTITY_EMOJI_SCALE,
    COMPACT_IDENTITY_ICON_SIZE,
    COMPACT_IDENTITY_ICON_STROKE_WIDTH,
  } from "$lib/icon-sizing";
  import {
    beginLazyComponentLoad,
    rejectLazyComponentLoad,
    resolveLazyComponentLoad,
    type LazyComponentLoadState,
  } from "$lib/lazy-component-loader";
  import { dismissOnOutside } from "$lib/utils/dismiss-on-outside";
  import { overflowTooltip } from "$lib/utils/overflow-tooltip";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    createInlineRenameHistory,
    inlineRenameHistoryAction,
    inlineRenameInputKind,
    inlineRenameHistoryValue,
    recordInlineRenameValue,
    stepInlineRenameHistory,
  } from "$lib/notes/history/inline-rename-history";
  import type { NotesDestinationPickerTarget } from "$lib/notes/pages/destination-picker";
  import {
    notesRowContextMenuGeometry,
    notesRowContextMenuStyle,
  } from "$lib/notes/database/row-context-menu";
  import type { NotesPageMoveTarget } from "$lib/notes/pages/move";
  import type { NotesPageParentStatus } from "$lib/notes/pages/tree";
  import { notesPageTitle } from "$lib/notes/pages/title";
  import type { NotesPage, NotesParent } from "$lib/notes/types";
  import Archive from "@lucide/svelte/icons/archive";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Copy from "@lucide/svelte/icons/copy";
  import FilePlus2 from "@lucide/svelte/icons/file-plus-2";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import FolderTree from "@lucide/svelte/icons/folder-tree";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Star from "@lucide/svelte/icons/star";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import {
    loadNotesOptionalComponent,
    retryNotesOptionalComponent,
    type LoadedNotesOptionalComponent,
  } from "$lib/components/notes/component-registry";

  let {
    page,
    depth,
    hasChildren,
    collapsed,
    parentStatus,
    favorited,
    selected,
    onSelect,
    onRename,
    onToggleCollapsed,
    onToggleFavorite,
    onCreateChild,
    onDuplicate,
    moveTargets,
    onRequestMoveTargets = undefined,
    onMove,
    folderMoveTargets = undefined,
    onMoveToFolder = undefined,
    onArchive,
    onTrash,
    blockDropActive = false,
    onBlockDragOver,
    onBlockDragLeave,
    onBlockDrop,
    readOnly = false,
    showDisclosure = true,
    highlightRequestId = 0,
    navigationDragging = false,
    navigationDropState = "none",
    onNavigationDragStart = undefined,
    onNavigationDragEnd = undefined,
    onNavigationDragOver = undefined,
    onNavigationDragLeave = undefined,
    onNavigationDrop = undefined,
    displayTitle,
  }: {
    page: NotesPage;
    depth: number;
    hasChildren: boolean;
    collapsed: boolean;
    parentStatus: NotesPageParentStatus | null;
    favorited: boolean;
    selected: boolean;
    onSelect: () => void;
    onRename: (title: string) => boolean | void | Promise<boolean | void>;
    onToggleCollapsed?: (collapsed: boolean) => void;
    onToggleFavorite: (favorited: boolean) => void;
    onCreateChild: () => void;
    onDuplicate: () => void;
    moveTargets: NotesPageMoveTarget[];
    onRequestMoveTargets?: () => void | Promise<void>;
    onMove: (parent: NotesParent) => void;
    folderMoveTargets?: (NotesDestinationPickerTarget & { folderId: string | null })[];
    onMoveToFolder?: (folderId: string | null) => void;
    onArchive: () => void;
    onTrash: () => void;
    blockDropActive?: boolean;
    onBlockDragOver?: (pageId: string, event: DragEvent) => void;
    onBlockDragLeave?: (pageId: string, event: DragEvent) => void;
    onBlockDrop?: (pageId: string, event: DragEvent) => void;
    readOnly?: boolean;
    showDisclosure?: boolean;
    highlightRequestId?: number;
    navigationDragging?: boolean;
    navigationDropState?: "none" | "valid" | "invalid";
    onNavigationDragStart?: (event: DragEvent) => void;
    onNavigationDragEnd?: (event: DragEvent) => void;
    onNavigationDragOver?: (event: DragEvent) => void;
    onNavigationDragLeave?: (event: DragEvent) => void;
    onNavigationDrop?: (event: DragEvent) => void;
    displayTitle?: string;
  } = $props();

  const { t } = getLocalization();
  const explorerRowIconSize = COMPACT_IDENTITY_ICON_SIZE;
  const explorerRowIconStrokeWidth = COMPACT_IDENTITY_ICON_STROKE_WIDTH;
  const settledScrollTolerancePx = 0.5;
  const settledScrollFrameCount = 3;
  const maximumScrollObservationFrames = 90;
  let editing = $state(false);
  let menuOpen = $state(false);
  let menuStyle = $state("");
  let moveMenuOpen = $state(false);
  let folderMoveMenuOpen = $state(false);
  let titleDraft = $state("");
  let renameHistory = $state(createInlineRenameHistory(""));
  let pendingTitle = $state<string | null>(null);
  let highlightPulseActive = $state(false);
  let highlightRequestInitialized = false;
  let handledHighlightRequestId = 0;
  let scrollObservationFrame: number | null = null;
  let rowElement = $state<HTMLDivElement | null>(null);
  let renameInput = $state<HTMLInputElement | null>(null);
  let shouldSuppressNavigationSelect = false;
  let destinationPickerLoadState = $state<LazyComponentLoadState<
    "destination-picker",
    LoadedNotesOptionalComponent
  > | null>(null);

  function handleNavigationDragStart(event: DragEvent): void {
    shouldSuppressNavigationSelect = true;
    onNavigationDragStart?.(event);
  }

  function handleNavigationDragEnd(event: DragEvent): void {
    onNavigationDragEnd?.(event);
    window.setTimeout(() => {
      shouldSuppressNavigationSelect = false;
    }, 0);
  }

  function handleSelect(): void {
    if (shouldSuppressNavigationSelect) {
      shouldSuppressNavigationSelect = false;
      return;
    }
    onSelect();
  }
  const storedTitle = $derived(
    displayTitle === undefined
      ? notesPageTitle(page, t("notes.untitled"))
      : displayTitle.trim() || t("notes.untitled"),
  );
  const editableTitle = $derived(notesPageTitle(page, ""));
  const title = $derived(
    pendingTitle === null ? storedTitle : pendingTitle || t("notes.untitled"),
  );

  $effect(() => {
    if (pendingTitle !== null && editableTitle === pendingTitle) pendingTitle = null;
    if (!editing && pendingTitle === null) titleDraft = editableTitle;
  });

  $effect(() => {
    if (!menuOpen) {
      moveMenuOpen = false;
      folderMoveMenuOpen = false;
    }
  });

  $effect(() => {
    if (!editing) return;
    void tick().then(() => {
      renameInput?.focus();
      renameInput?.select();
    });
  });

  $effect(() => {
    const requestId = highlightRequestId;
    if (!highlightRequestInitialized) {
      highlightRequestInitialized = true;
      handledHighlightRequestId = requestId;
      return;
    }
    if (requestId <= 0 || requestId === handledHighlightRequestId) return;
    handledHighlightRequestId = requestId;
    highlightPulseActive = false;
    void tick().then(() => {
      revealAndHighlightCurrentRow(requestId);
    });
  });

  onDestroy(() => {
    if (scrollObservationFrame !== null) cancelAnimationFrame(scrollObservationFrame);
  });

  $effect(() => {
    if (moveMenuOpen || folderMoveMenuOpen) {
      void requestDestinationPicker();
      void onRequestMoveTargets?.();
    }
  });

  /** Loads the destination picker on first use. A retry rejects when the load fails again. */
  function requestDestinationPicker(retry = false): Promise<void> {
    if (!retry && destinationPickerLoadState?.key === "destination-picker") return Promise.resolve();
    const loadingState = beginLazyComponentLoad(destinationPickerLoadState, "destination-picker");
    destinationPickerLoadState = loadingState;
    const request = retry
      ? retryNotesOptionalComponent("destination-picker")
      : loadNotesOptionalComponent("destination-picker");
    return request.then((component) => {
      if (!destinationPickerLoadState) return;
      destinationPickerLoadState = resolveLazyComponentLoad(
        destinationPickerLoadState,
        "destination-picker",
        loadingState.requestId,
        component,
      );
    }).catch((error: unknown) => {
      if (!destinationPickerLoadState) return;
      destinationPickerLoadState = rejectLazyComponentLoad(
        destinationPickerLoadState,
        "destination-picker",
        loadingState.requestId,
        error,
      );
      console.error("load Notes page destination picker failed", error);
      if (retry) throw error;
    });
  }

  function revealAndHighlightCurrentRow(requestId: number): void {
    const row = rowElement;
    const scrollContainer = row?.closest<HTMLElement>("[data-notes-explorer-scroll]");
    if (!row || !scrollContainer) return;

    if (scrollObservationFrame !== null) cancelAnimationFrame(scrollObservationFrame);
    scrollObservationFrame = null;

    const rowBounds = row.getBoundingClientRect();
    const containerBounds = scrollContainer.getBoundingClientRect();
    const rowIsVisible = rowBounds.bottom > containerBounds.top
      && rowBounds.top < containerBounds.bottom;
    if (rowIsVisible) {
      highlightPulseActive = true;
      return;
    }

    let previousScrollTop = scrollContainer.scrollTop;
    let stableFrames = 0;
    let hasObservedMovement = false;
    let observedFrames = 0;
    const prefersReducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    row.scrollIntoView({
      behavior: prefersReducedMotion ? "auto" : "smooth",
      block: "center",
      inline: "nearest",
    });

    const observeScroll = (): void => {
      if (handledHighlightRequestId !== requestId) return;

      const currentScrollTop = scrollContainer.scrollTop;
      const scrollDifference = Math.abs(currentScrollTop - previousScrollTop);
      observedFrames += 1;
      if (scrollDifference <= settledScrollTolerancePx) {
        stableFrames += 1;
      } else {
        hasObservedMovement = true;
        stableFrames = 0;
      }
      previousScrollTop = currentScrollTop;

      if (
        (hasObservedMovement && stableFrames >= settledScrollFrameCount)
        || observedFrames >= maximumScrollObservationFrames
      ) {
        scrollObservationFrame = null;
        highlightPulseActive = true;
        return;
      }
      scrollObservationFrame = requestAnimationFrame(observeScroll);
    };

    scrollObservationFrame = requestAnimationFrame(observeScroll);
  }

  function moveToTarget(targetKey: string): void {
    const target = moveTargets.find((candidate) => candidate.key === targetKey);
    if (!target) return;
    menuOpen = false;
    moveMenuOpen = false;
    onMove(target.parent);
  }

  function moveToFolderTarget(targetKey: string): void {
    const target = folderMoveTargets?.find((candidate) => candidate.key === targetKey);
    if (!target || !onMoveToFolder) return;
    menuOpen = false;
    moveMenuOpen = false;
    folderMoveMenuOpen = false;
    onMoveToFolder(target.folderId);
  }

  function closeMenu(): void {
    menuOpen = false;
    moveMenuOpen = false;
    folderMoveMenuOpen = false;
  }

  /** Open the existing row menu from a keyboard or pointer action button. */
  function openActionMenu(event: MouseEvent): void {
    const trigger = event.currentTarget;
    if (!(trigger instanceof HTMLButtonElement)) return;
    if (menuOpen) {
      event.preventDefault();
      event.stopPropagation();
      closeMenu();
      return;
    }
    openContextMenu(event);
    const rect = trigger.getBoundingClientRect();
    menuStyle = notesRowContextMenuStyle(notesRowContextMenuGeometry({
      clientX: rect.left,
      clientY: rect.bottom,
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
    }));
    void tick().then(() => rowElement?.querySelector<HTMLButtonElement>('[role="menu"] button')?.focus());
  }

  function openContextMenu(event: MouseEvent): void {
    if (readOnly || editing) return;
    if (event.target instanceof Element && event.target.closest("[data-app-floating-surface]")) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    menuStyle = notesRowContextMenuStyle(notesRowContextMenuGeometry({
      clientX: event.clientX,
      clientY: event.clientY,
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
    }));
    menuOpen = true;
  }

  function saveRename(): void {
    if (!editing) return;
    const title = titleDraft.trim();
    const previousTitle = editableTitle;
    editing = false;
    menuOpen = false;
    titleDraft = title;
    if (title === editableTitle) return;
    pendingTitle = title;
    void Promise.resolve(onRename(title)).then((renamed) => {
      if (renamed !== false) return;
      pendingTitle = null;
      titleDraft = previousTitle;
    }).catch(() => {
      pendingTitle = null;
      titleDraft = previousTitle;
    });
  }

  function beginRename(): void {
    titleDraft = editableTitle;
    renameHistory = createInlineRenameHistory(titleDraft);
    editing = true;
  }

  function handleRenameInput(event: Event): void {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) return;
    titleDraft = input.value;
    const inputType = event instanceof InputEvent ? event.inputType : "";
    renameHistory = recordInlineRenameValue(
      renameHistory,
      titleDraft,
      inlineRenameInputKind(inputType),
      Date.now(),
    );
  }

  function handleRenameKeydown(event: KeyboardEvent): void {
    const historyAction = inlineRenameHistoryAction(event);
    if (historyAction) {
      event.preventDefault();
      event.stopPropagation();
      renameHistory = stepInlineRenameHistory(renameHistory, historyAction);
      titleDraft = inlineRenameHistoryValue(renameHistory);
      if (renameInput) {
        renameInput.value = titleDraft;
        renameInput.setSelectionRange(titleDraft.length, titleDraft.length);
      }
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      saveRename();
    }
    if (event.key === "Escape") {
      event.preventDefault();
      editing = false;
      titleDraft = editableTitle;
    }
  }

  function parentStatusLabel(status: NotesPageParentStatus): string {
    return status === "trashed" ? t("notes.parentInTrash") : t("notes.parentMissing");
  }
</script>

<div
  bind:this={rowElement}
  class="notes-page-row group relative"
  class:notes-row-context-menu-open={menuOpen}
  class:notes-page-block-drop-target={blockDropActive}
  class:notes-page-row-current-file-pulse={highlightPulseActive}
  class:notes-navigation-dragging={navigationDragging}
  class:notes-navigation-draggable={Boolean(onNavigationDragStart) && !editing && !readOnly}
  class:notes-navigation-drop-valid={navigationDropState === "valid"}
  class:notes-navigation-drop-invalid={navigationDropState === "invalid"}
  role="group"
  aria-label={title}
  style={`--notes-page-depth: ${Math.min(depth, 10)}`}
  data-app-tooltip={blockDropActive ? t("notes.dropBlockOnPage", title) : undefined}
  ondragover={(event) => {
    onNavigationDragOver?.(event);
    onBlockDragOver?.(page.id, event);
  }}
  ondragleave={(event) => {
    onNavigationDragLeave?.(event);
    onBlockDragLeave?.(page.id, event);
  }}
  ondrop={(event) => {
    onNavigationDrop?.(event);
    onBlockDrop?.(page.id, event);
  }}
  draggable={!editing && !readOnly && Boolean(onNavigationDragStart)}
  ondragstart={handleNavigationDragStart}
  ondragend={handleNavigationDragEnd}
  oncontextmenu={openContextMenu}
  onanimationend={() => {
    highlightPulseActive = false;
  }}
  use:dismissOnOutside={{ enabled: menuOpen, onDismiss: closeMenu }}
>
  {#if editing}
    <div class={`notes-page-row-content explorer-row flex min-w-0 items-center rounded-md bg-accent/50 py-1.5 pr-1 text-foreground ${showDisclosure ? "" : "pl-2"}`}>
      {#if showDisclosure}
        <span class="explorer-row-disclosure" aria-hidden="true"></span>
      {/if}
      <div class="flex min-w-0 flex-1 items-center gap-1.5">
        {#if page.icon}
          <NotesPageIcon
            icon={page.icon}
            size={explorerRowIconSize}
            strokeWidth={explorerRowIconStrokeWidth}
            emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
            class="explorer-row-icon"
          />
        {:else}
          <NotesPageIcon
            icon={null}
            size={explorerRowIconSize}
            strokeWidth={explorerRowIconStrokeWidth}
            emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
            class="explorer-row-icon"
          />
        {/if}
        <input
          bind:this={renameInput}
          class="min-w-0 flex-1 bg-transparent explorer-row-label text-[0.866667rem] text-inherit caret-primary outline-none placeholder:text-muted-foreground"
          data-app-shortcuts="ignore"
          aria-label={t("notes.renamePage")}
          value={titleDraft}
          placeholder={t("notes.titlePlaceholder")}
          oninput={handleRenameInput}
          onkeydown={handleRenameKeydown}
          onblur={saveRename}
        />
      </div>
    </div>
  {:else}
    <div
      class={`notes-page-row-content explorer-row flex min-w-0 items-center rounded-md text-foreground ${readOnly ? "pr-1" : ""} ${showDisclosure ? "" : "pl-2"}`}
      class:explorer-selected={selected}
    >
      {#if showDisclosure}
        <button
          class="explorer-row-disclosure flex items-center justify-center rounded text-muted-foreground hover:bg-background/80 hover:text-foreground disabled:pointer-events-none disabled:opacity-0"
          type="button"
          aria-label={collapsed ? t("notes.expandPage") : t("notes.collapsePage")}
          disabled={!hasChildren}
          onclick={(event) => {
            event.stopPropagation();
            onToggleCollapsed?.(!collapsed);
          }}
        >
          {#if hasChildren && collapsed}
            <ChevronRight class="explorer-row-icon" strokeWidth={explorerRowIconStrokeWidth} />
          {:else if hasChildren}
            <ChevronDown class="explorer-row-icon" strokeWidth={explorerRowIconStrokeWidth} />
          {/if}
        </button>
      {/if}
      <button
        class="flex min-w-0 flex-1 items-center gap-1.5 py-1.5 pr-1 text-left explorer-row-label text-[0.866667rem] text-inherit"
        type="button"
        aria-current={selected ? "page" : undefined}
        onclick={handleSelect}
      >
        {#if page.icon}
          <NotesPageIcon
            icon={page.icon}
            size={explorerRowIconSize}
            strokeWidth={explorerRowIconStrokeWidth}
            emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
            class="explorer-row-icon"
          />
        {:else}
          <NotesPageIcon
            icon={null}
            size={explorerRowIconSize}
            strokeWidth={explorerRowIconStrokeWidth}
            emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
            class="explorer-row-icon"
          />
        {/if}
        <span class="min-w-0 flex-1 truncate" use:overflowTooltip={title}>{title}</span>
        {#if parentStatus}
          <TriangleAlert
            class="size-3.5 shrink-0 text-destructive"
            aria-label={parentStatusLabel(parentStatus)}
            data-app-tooltip={parentStatusLabel(parentStatus)}
          />
        {/if}
      </button>
      {#if favorited || !readOnly}
        <span class="notes-page-row-trailing" class:notes-page-row-trailing-swap={favorited && !readOnly}>
          {#if favorited}
            <span class="notes-page-row-star" aria-hidden="true"><Star class="size-3.5 fill-current text-primary" /></span>
          {/if}
          {#if !readOnly}
            <button type="button" class="explorer-icon explorer-row-action" aria-label={`${t("notes.pageActions")}: ${title}`} data-app-tooltip-disabled="true" aria-haspopup="menu" aria-expanded={menuOpen} onclick={openActionMenu}><EllipsisVertical size={14} /></button>
          {/if}
        </span>
      {/if}
    </div>
  {/if}

  {#if menuOpen}
    <div
      class="notes-page-action-menu surface-floating fixed z-50 flex min-w-36 flex-col overflow-hidden"
      style={menuStyle}
      role="menu"
      tabindex="-1"
      onkeydown={(event) => {
        if (event.key !== "Escape") return;
        event.stopPropagation();
        closeMenu();
        rowElement?.querySelector<HTMLButtonElement>(".explorer-row-action")?.focus();
      }}
      data-app-floating-surface
    >
      <div class="surface-floating-body flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden" use:scrollEdgeFadeAction>
        <button
          class="menu-item"
          type="button"
          onclick={() => {
            menuOpen = false;
            onCreateChild();
          }}
        >
          <FilePlus2 />
          <span>{t("notes.newSubpage")}</span>
        </button>
        <button
          class="menu-item"
          type="button"
          onclick={() => {
            onToggleFavorite(!favorited);
            menuOpen = false;
          }}
        >
          <Star class={favorited ? "fill-current text-primary" : undefined} />
          <span>{favorited ? t("notes.removeFromFavorites") : t("notes.addToFavorites")}</span>
        </button>
        <button
          class="menu-item"
          type="button"
          onclick={() => {
            beginRename();
            menuOpen = false;
          }}
        >
          <Pencil />
          <span>{t("notes.renamePage")}</span>
        </button>
        <button
          class="menu-item"
          type="button"
          onclick={() => {
            menuOpen = false;
            onDuplicate();
          }}
        >
          <Copy />
          <span>{t("notes.duplicatePage")}</span>
        </button>
        <button
          class="menu-item"
          type="button"
          aria-expanded={moveMenuOpen}
          onclick={() => {
            moveMenuOpen = !moveMenuOpen;
            folderMoveMenuOpen = false;
          }}
        >
          <FolderInput />
          <span>{t("notes.movePageTo")}</span>
        </button>
        {#if moveMenuOpen}
          <div
            class="notes-page-move-menu border-y border-border bg-muted/25 py-1"
            use:scrollEdgeFadeAction
            aria-label={t("notes.movePageTo")}
          >
            {#if destinationPickerLoadState?.status === "ready" && destinationPickerLoadState.component.kind === "destination-picker"}
              {@const NotesDestinationPickerList = destinationPickerLoadState.component.component}
              <NotesDestinationPickerList
                targets={moveTargets}
                searchLabel={t("notes.moveDestinationSearch")}
                searchPlaceholder={t("notes.moveDestinationSearchPlaceholder")}
                recentLabel={t("notes.recentDestinations")}
                pagesLabel={t("notes.allPages")}
                emptyLabel={t("notes.noPageMoveTargets")}
                optionLabel={(target) => t("notes.movePageToTarget", target.title)}
                onSelect={moveToTarget}
                onClose={() => {
                  moveMenuOpen = false;
                }}
              />
            {:else if destinationPickerLoadState?.status === "failed"}
              <Notice class="p-2" message={t("common.viewLoadFailed", t("notes.movePageTo"))} copy={false} onRetry={() => requestDestinationPicker(true)} />
            {:else}
              <Loading layout="rows" rows={3} label={t("notes.movePageTo")} />
            {/if}
          </div>
        {/if}
        {#if folderMoveTargets && folderMoveTargets.length > 0 && onMoveToFolder}
          <button
            class="menu-item"
            type="button"
            aria-expanded={folderMoveMenuOpen}
            onclick={() => {
              folderMoveMenuOpen = !folderMoveMenuOpen;
              moveMenuOpen = false;
            }}
          >
            <FolderTree />
            <span>{t("notes.movePageToFolder")}</span>
          </button>
          {#if folderMoveMenuOpen}
            <div
              class="notes-page-move-menu border-y border-border bg-muted/25 py-1"
              use:scrollEdgeFadeAction
              aria-label={t("notes.movePageToFolder")}
            >
              {#if destinationPickerLoadState?.status === "ready" && destinationPickerLoadState.component.kind === "destination-picker"}
                {@const NotesDestinationPickerList = destinationPickerLoadState.component.component}
                <NotesDestinationPickerList
                  targets={folderMoveTargets}
                  searchLabel={t("notes.moveDestinationSearch")}
                  searchPlaceholder={t("notes.moveDestinationSearchPlaceholder")}
                  recentLabel={t("notes.recentDestinations")}
                  pagesLabel={t("notes.folders")}
                  emptyLabel={t("notes.noFolderMoveTargets")}
                  optionLabel={(target) => t("notes.movePageToFolderTarget", target.title)}
                  onSelect={moveToFolderTarget}
                  onClose={() => {
                    folderMoveMenuOpen = false;
                  }}
                />
              {:else if destinationPickerLoadState?.status === "failed"}
                <Notice class="p-2" message={t("common.viewLoadFailed", t("notes.movePageToFolder"))} copy={false} onRetry={() => requestDestinationPicker(true)} />
              {:else}
                <Loading layout="rows" rows={3} label={t("notes.movePageToFolder")} />
              {/if}
            </div>
          {/if}
        {/if}
        <button
          class="menu-item"
          type="button"
          onclick={() => {
            menuOpen = false;
            onArchive();
          }}
        >
          <Archive />
          <span>{t("notes.archivePage")}</span>
        </button>
        <button
          class="menu-item menu-item-destructive"
          type="button"
          onclick={() => {
            menuOpen = false;
            onTrash();
          }}
        >
          <Trash2 />
          <span>{t("notes.moveToTrash")}</span>
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .notes-page-row-content {
    margin-left: calc(var(--notes-page-depth) * 0.875rem);
  }

  /* The favorite star sits in the actions slot so it lines up with the actions button on every row. */
  .notes-page-row-trailing {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    margin-inline-end: var(--explorer-row-action-end-inset);
  }

  .notes-page-row-trailing > .explorer-row-action {
    margin-inline-end: 0;
  }

  .notes-page-row-star {
    display: inline-flex;
    width: var(--explorer-action-size);
    height: var(--explorer-action-size);
    align-items: center;
    justify-content: center;
    pointer-events: none;
  }

  /* With a hover pointer the actions button only shows on hover or focus, so the star takes its place otherwise. */
  @media (hover: hover) and (pointer: fine) {
    :global(.explorer-sidebar:not(.explorer-touch)) .notes-page-row-trailing-swap {
      display: grid;
    }

    :global(.explorer-sidebar:not(.explorer-touch)) .notes-page-row-trailing-swap > * {
      grid-area: 1 / 1;
    }

    :global(.explorer-sidebar:not(.explorer-touch)) :global(.explorer-row):is(:hover, :has(:focus-visible)) .notes-page-row-star,
    :global(.explorer-sidebar:not(.explorer-touch)) .notes-page-row-trailing:has([aria-expanded="true"]) .notes-page-row-star {
      visibility: hidden;
    }
  }

  .notes-row-context-menu-open .notes-page-row-content {
    background: var(--explorer-row-highlight);
  }

  .notes-page-block-drop-target .notes-page-row-content {
    background: hsl(var(--primary) / 0.12);
    box-shadow: inset 0 0 0 1px hsl(var(--primary) / 0.55);
  }

  .notes-navigation-dragging .notes-page-row-content {
    opacity: 0.42;
  }

  .notes-navigation-draggable .notes-page-row-content {
    cursor: grab;
  }

  .notes-navigation-dragging .notes-page-row-content {
    cursor: grabbing;
  }

  .notes-navigation-drop-valid .notes-page-row-content {
    background: color-mix(in oklab, var(--accent) 78%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--foreground) 18%, transparent);
  }

  .notes-navigation-drop-invalid .notes-page-row-content {
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--destructive) 42%, transparent);
    cursor: no-drop;
  }

  .notes-page-row-current-file-pulse .notes-page-row-content {
    position: relative;
    isolation: isolate;
  }

  .notes-page-row-current-file-pulse .notes-page-row-content::before {
    position: absolute;
    inset: 0;
    z-index: -1;
    border-radius: inherit;
    background: var(--selection-background);
    opacity: 0;
    pointer-events: none;
    content: "";
    animation: notes-current-file-highlight 720ms ease-in-out;
  }

  @keyframes notes-current-file-highlight {
    0%, 50%, 100% {
      opacity: 0;
    }

    25%, 75% {
      opacity: 1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .notes-page-row-current-file-pulse .notes-page-row-content::before {
      animation-duration: 1ms;
    }
  }

  .notes-page-move-menu {
    max-height: 12rem;
    overflow-y: auto;
  }
</style>
