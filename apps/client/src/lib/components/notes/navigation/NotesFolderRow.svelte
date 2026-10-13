<script lang="ts">
  import EllipsisVertical from "@lucide/svelte/icons/ellipsis-vertical";
  import { tick } from "svelte";
  import {
    beginLazyComponentLoad,
    rejectLazyComponentLoad,
    resolveLazyComponentLoad,
    type LazyComponentLoadState,
  } from "$lib/lazy-component-loader";
  import FilePlus2 from "@lucide/svelte/icons/file-plus-2";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Star from "@lucide/svelte/icons/star";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ExplorerRowStar from "$lib/components/ui/ExplorerRowStar.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import {
    COMPACT_IDENTITY_ICON_SIZE,
    COMPACT_IDENTITY_ICON_STROKE_WIDTH,
  } from "$lib/icon-sizing";
  import type { NotesDestinationPickerTarget } from "$lib/notes/pages/destination-picker";
  import {
    createInlineRenameHistory,
    inlineRenameHistoryAction,
    inlineRenameInputKind,
    inlineRenameHistoryValue,
    recordInlineRenameValue,
    stepInlineRenameHistory,
  } from "$lib/notes/history/inline-rename-history";
  import {
    notesRowContextMenuGeometry,
    notesRowContextMenuStyle,
  } from "$lib/notes/database/row-context-menu";
  import type { NotesFolder } from "$lib/notes/types";
  import { dismissOnOutside } from "$lib/utils/dismiss-on-outside";
  import { overflowTooltip } from "$lib/utils/overflow-tooltip";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    loadNotesOptionalComponent,
    retryNotesOptionalComponent,
    type LoadedNotesOptionalComponent,
  } from "$lib/components/notes/component-registry";

  type NotesFolderRowMoveTarget = NotesDestinationPickerTarget & {
    folderId: string | null;
  };

  let {
    folder,
    depth,
    collapsed,
    favorited = false,
    renameRequestId = 0,
    moveTargets,
    onActivate,
    onToggleCollapsed,
    onToggleFavorite,
    onCreatePage,
    onCreateFolder,
    onRename,
    onMove,
    onDelete,
    navigationDragging = false,
    navigationDropState = "none",
    onNavigationDragStart = undefined,
    onNavigationDragEnd = undefined,
    onNavigationDragOver = undefined,
    onNavigationDragLeave = undefined,
    onNavigationDrop = undefined,
  }: {
    folder: NotesFolder;
    depth: number;
    collapsed: boolean;
    favorited?: boolean;
    renameRequestId?: number;
    moveTargets: NotesFolderRowMoveTarget[];
    onActivate: () => void;
    onToggleCollapsed: (collapsed: boolean) => void;
    onToggleFavorite: (favorited: boolean) => void;
    onCreatePage: () => void;
    onCreateFolder: () => void;
    onRename: (name: string) => boolean | Promise<boolean>;
    onMove: (parentFolderId: string | null) => void;
    onDelete: () => void;
    navigationDragging?: boolean;
    navigationDropState?: "none" | "valid" | "invalid";
    onNavigationDragStart?: (event: DragEvent) => void;
    onNavigationDragEnd?: (event: DragEvent) => void;
    onNavigationDragOver?: (event: DragEvent) => void;
    onNavigationDragLeave?: (event: DragEvent) => void;
    onNavigationDrop?: (event: DragEvent) => void;
  } = $props();

  const { t } = getLocalization();
  const explorerRowIconSize = COMPACT_IDENTITY_ICON_SIZE;
  const explorerRowIconStrokeWidth = COMPACT_IDENTITY_ICON_STROKE_WIDTH;
  let editing = $state(false);
  let rowElement: HTMLDivElement;
  let menuOpen = $state(false);
  let menuStyle = $state("");
  let moveMenuOpen = $state(false);
  let nameDraft = $state("");
  let renameHistory = $state(createInlineRenameHistory(""));
  let pendingName = $state<string | null>(null);
  let renameInput = $state<HTMLInputElement | null>(null);
  let handledRenameRequestId = 0;
  let shouldSuppressFolderActivation = false;
  let destinationPickerLoadState = $state<LazyComponentLoadState<
    "destination-picker",
    LoadedNotesOptionalComponent
  > | null>(null);

  function handleNavigationDragStart(event: DragEvent): void {
    shouldSuppressFolderActivation = true;
    onNavigationDragStart?.(event);
  }

  function handleNavigationDragEnd(event: DragEvent): void {
    onNavigationDragEnd?.(event);
    window.setTimeout(() => {
      shouldSuppressFolderActivation = false;
    }, 0);
  }

  function activateFolder(): void {
    if (shouldSuppressFolderActivation) {
      shouldSuppressFolderActivation = false;
      return;
    }
    onActivate();
    onToggleCollapsed(!collapsed);
  }

  const visibleName = $derived(pendingName ?? folder.name);

  $effect(() => {
    if (pendingName !== null && folder.name === pendingName) pendingName = null;
    if (!editing && pendingName === null) nameDraft = folder.name;
  });

  $effect(() => {
    if (!menuOpen) moveMenuOpen = false;
  });

  $effect(() => {
    if (renameRequestId <= 0 || renameRequestId === handledRenameRequestId) return;
    handledRenameRequestId = renameRequestId;
    beginRename();
  });

  $effect(() => {
    if (!editing) return;
    void tick().then(() => {
      renameInput?.focus();
      renameInput?.select();
    });
  });

  $effect(() => {
    if (moveMenuOpen) void requestDestinationPicker();
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
      console.error("load Notes folder destination picker failed", error);
      if (retry) throw error;
    });
  }

  function closeMenu(): void {
    menuOpen = false;
    moveMenuOpen = false;
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
    if (editing) return;
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
    const name = nameDraft.trim();
    const previousName = folder.name;
    editing = false;
    menuOpen = false;
    nameDraft = name || folder.name;
    if (!name || name === folder.name) return;
    pendingName = name;
    void Promise.resolve(onRename(name)).then((renamed) => {
      if (renamed) return;
      pendingName = null;
      nameDraft = previousName;
    }).catch(() => {
      pendingName = null;
      nameDraft = previousName;
    });
  }

  function beginRename(): void {
    nameDraft = visibleName;
    renameHistory = createInlineRenameHistory(nameDraft);
    editing = true;
  }

  function handleRenameInput(event: Event): void {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) return;
    nameDraft = input.value;
    const inputType = event instanceof InputEvent ? event.inputType : "";
    renameHistory = recordInlineRenameValue(
      renameHistory,
      nameDraft,
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
      nameDraft = inlineRenameHistoryValue(renameHistory);
      if (renameInput) {
        renameInput.value = nameDraft;
        renameInput.setSelectionRange(nameDraft.length, nameDraft.length);
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
      nameDraft = folder.name;
    }
  }

  function moveToTarget(targetKey: string): void {
    const target = moveTargets.find((candidate) => candidate.key === targetKey);
    if (!target) return;
    closeMenu();
    onMove(target.folderId);
  }
</script>

<div
  bind:this={rowElement}
  class="notes-folder-row group relative"
  class:notes-row-context-menu-open={menuOpen}
  class:notes-navigation-dragging={navigationDragging}
  class:notes-navigation-draggable={Boolean(onNavigationDragStart) && !editing}
  class:notes-navigation-drop-valid={navigationDropState === "valid"}
  class:notes-navigation-drop-invalid={navigationDropState === "invalid"}
  role="group"
  aria-label={visibleName}
  style={`--notes-folder-depth: ${Math.min(depth, 10)}`}
  oncontextmenu={openContextMenu}
  draggable={!editing && Boolean(onNavigationDragStart)}
  ondragstart={handleNavigationDragStart}
  ondragend={handleNavigationDragEnd}
  ondragover={onNavigationDragOver}
  ondragleave={onNavigationDragLeave}
  ondrop={onNavigationDrop}
  use:dismissOnOutside={{ enabled: menuOpen, onDismiss: closeMenu }}
>
  {#if editing}
    <div class="notes-folder-row-content explorer-row flex min-w-0 items-center gap-1.5 rounded-md bg-accent/50 px-2 py-1.5 text-foreground">
      {#if collapsed}
        <Folder size={explorerRowIconSize} class="explorer-row-icon" strokeWidth={explorerRowIconStrokeWidth} />
      {:else}
        <FolderOpen size={explorerRowIconSize} class="explorer-row-icon" strokeWidth={explorerRowIconStrokeWidth} />
      {/if}
      <input
        bind:this={renameInput}
        class="min-w-0 flex-1 bg-transparent explorer-row-label text-[0.866667rem] text-inherit caret-primary outline-none placeholder:text-muted-foreground"
        data-app-shortcuts="ignore"
        aria-label={t("notes.renameFolder")}
        value={nameDraft}
        placeholder={t("notes.folderNamePlaceholder")}
        oninput={handleRenameInput}
        onkeydown={handleRenameKeydown}
        onblur={saveRename}
      />
    </div>
  {:else}
    <div class="notes-folder-row-content explorer-row flex min-w-0 items-center rounded-md text-foreground">
      <button
        class="flex min-w-0 flex-1 items-center gap-1.5 px-2 py-1.5 text-left explorer-row-label text-[0.866667rem] text-inherit"
        type="button"
        aria-expanded={!collapsed}
        aria-label={collapsed ? t("notes.expandFolder") : t("notes.collapseFolder")}
        onclick={activateFolder}
      >
        {#if collapsed}
          <Folder size={explorerRowIconSize} class="explorer-row-icon" strokeWidth={explorerRowIconStrokeWidth} />
        {:else}
          <FolderOpen size={explorerRowIconSize} class="explorer-row-icon" strokeWidth={explorerRowIconStrokeWidth} />
        {/if}
        <span class="min-w-0 flex-1 truncate" use:overflowTooltip={visibleName}>{visibleName}</span>
      </button>
      <span class="explorer-row-trailing">
        {#if favorited}
          <ExplorerRowStar />
        {/if}
        <button type="button" class="explorer-icon explorer-row-action" aria-label={`${t("notes.folderActions")}: ${visibleName}`} data-app-tooltip-disabled="true" aria-haspopup="menu" aria-expanded={menuOpen} onclick={openActionMenu}><EllipsisVertical size={14} /></button>
      </span>
    </div>
  {/if}

  {#if menuOpen}
    <div
      class="notes-folder-action-menu surface-floating fixed z-50 flex min-w-40 flex-col overflow-hidden"
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
            closeMenu();
            onCreatePage();
          }}
        >
          <FilePlus2 />
          <span>{t("notes.newNoteInFolder")}</span>
        </button>
        <button
          class="menu-item"
          type="button"
          onclick={() => {
            closeMenu();
            onCreateFolder();
          }}
        >
          <FolderPlus />
          <span>{t("notes.newSubfolder")}</span>
        </button>
        <button
          class="menu-item"
          type="button"
          onclick={() => {
            closeMenu();
            onToggleFavorite(!favorited);
          }}
        >
          <Star class={favorited ? "fill-current text-primary" : undefined} />
          <span>{favorited ? t("common.removeFromFavorites") : t("common.addToFavorites")}</span>
        </button>
        <button
          class="menu-item"
          type="button"
          onclick={() => {
            beginRename();
            closeMenu();
          }}
        >
          <Pencil />
          <span>{t("notes.renameFolder")}</span>
        </button>
        {#if moveTargets.length > 0}
          <button
            class="menu-item"
            type="button"
            aria-expanded={moveMenuOpen}
            onclick={() => {
              moveMenuOpen = !moveMenuOpen;
            }}
          >
            <FolderInput />
            <span>{t("notes.moveFolderTo")}</span>
          </button>
          {#if moveMenuOpen}
            <div class="notes-folder-move-menu border-y border-border bg-muted/25 py-1" use:scrollEdgeFadeAction>
              {#if destinationPickerLoadState?.status === "ready" && destinationPickerLoadState.component.kind === "destination-picker"}
                {@const NotesDestinationPickerList = destinationPickerLoadState.component.component}
                <NotesDestinationPickerList
                  targets={moveTargets}
                  searchLabel={t("notes.moveDestinationSearch")}
                  searchPlaceholder={t("notes.moveDestinationSearchPlaceholder")}
                  recentLabel={t("notes.recentDestinations")}
                  pagesLabel={t("notes.folders")}
                  emptyLabel={t("notes.noFolderMoveTargets")}
                  optionLabel={(target) => t("notes.moveFolderToTarget", target.title)}
                  onSelect={moveToTarget}
                  onClose={() => {
                    moveMenuOpen = false;
                  }}
                />
              {:else if destinationPickerLoadState?.status === "failed"}
                <Notice class="p-2" message={t("common.viewLoadFailed", t("notes.moveFolderTo"))} copy={false} onRetry={() => requestDestinationPicker(true)} />
              {:else}
                <Loading layout="rows" rows={3} label={t("notes.moveFolderTo")} />
              {/if}
            </div>
          {/if}
        {/if}
        <button
          class="menu-item menu-item-destructive"
          type="button"
          onclick={() => {
            closeMenu();
            onDelete();
          }}
        >
          <Trash2 />
          <span>{t("notes.deleteFolder")}</span>
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .notes-folder-row {
    --notes-folder-indent: calc(var(--notes-folder-depth) * 0.875rem);
  }

  .notes-folder-row-content {
    margin-left: var(--notes-folder-indent);
  }

  .notes-row-context-menu-open .notes-folder-row-content {
    background: var(--explorer-row-highlight);
  }

  .notes-navigation-dragging .notes-folder-row-content {
    opacity: 0.42;
  }

  .notes-navigation-draggable .notes-folder-row-content {
    cursor: grab;
  }

  .notes-navigation-dragging .notes-folder-row-content {
    cursor: grabbing;
  }

  .notes-navigation-drop-valid .notes-folder-row-content {
    background: color-mix(in oklab, var(--accent) 78%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--foreground) 18%, transparent);
  }

  .notes-navigation-drop-invalid .notes-folder-row-content {
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--destructive) 42%, transparent);
    cursor: no-drop;
  }

  .notes-folder-move-menu {
    max-height: 12rem;
    overflow-y: auto;
  }
</style>
