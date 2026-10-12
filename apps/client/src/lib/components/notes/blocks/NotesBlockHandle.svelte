<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import {
    beginLazyComponentLoad,
    rejectLazyComponentLoad,
    resolveLazyComponentLoad,
    type LazyComponentLoadState,
  } from "$lib/lazy-component-loader";
  import { dismissOnOutside } from "$lib/utils/dismiss-on-outside";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    NOTES_BACKGROUND_COLORS,
    NOTES_TEXT_COLORS,
    notesBlockColorSwatchStyle,
  } from "$lib/notes/blocks/color";
  import {
    CLOSED_NOTES_BLOCK_HANDLE_MENUS,
    notesBlockHandleActionMenuStyle,
    notesBlockHandleMenuStateAfterAction,
    notesBlockHandleMenuStateAfterMoveToggle,
  } from "$lib/notes/blocks/handle";
  import type {
    NotesBlockHandleAction,
    NotesBlockHandleMenuState,
  } from "$lib/notes/blocks/handle";
  import type { NotesBlockInsertMenuRect } from "$lib/notes/blocks/insertion";
  import type { NotesMoveToPageTarget } from "$lib/notes/blocks/move";
  import type { NotesColor } from "$lib/notes/types";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import LinkIcon from "@lucide/svelte/icons/link";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import Pilcrow from "@lucide/svelte/icons/pilcrow";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { onDestroy } from "svelte";
  import {
    loadNotesEditorPanel,
    retryNotesEditorPanel,
    type LoadedNotesEditorPanel,
  } from "$lib/components/notes/editor-component-registry";

  let {
    onTurnInto,
    canTurnInto = true,
    canSetColor,
    currentColor,
    currentBackgroundColor = currentColor,
    backgroundOnly = false,
    onColorSelect,
    onCopyLink,
    onDuplicate,
    onComment,
    onOpenComments,
    commentCount = 0,
    unreadCommentCount = 0,
    onMoveUp,
    onMoveDown,
    moveTargets,
    onMoveToPage,
    onDelete,
    contextMenuRequest = null,
    onMenuOpenChange,
  }: {
    onTurnInto: () => void;
    canTurnInto?: boolean;
    canSetColor: boolean;
    currentColor: NotesColor;
    currentBackgroundColor?: NotesColor;
    backgroundOnly?: boolean;
    onColorSelect: (color: NotesColor) => void;
    onCopyLink: () => Promise<void> | void;
    onDuplicate: () => void;
    onComment: () => void;
    /** Opens the block's existing discussions from the comment count, starting a comment when omitted. */
    onOpenComments?: () => void;
    commentCount?: number;
    unreadCommentCount?: number;
    onMoveUp: () => void;
    onMoveDown: () => void;
    moveTargets: NotesMoveToPageTarget[];
    onMoveToPage: (pageId: string) => void;
    onDelete: () => void;
    contextMenuRequest?: { x: number; y: number; id: number } | null;
    onMenuOpenChange?: (open: boolean) => void;
  } = $props();

  const { t } = getLocalization();
  let menuOpen = $state(false);
  let moveMenuOpen = $state(false);
  let actionMenuTriggerRect = $state<NotesBlockInsertMenuRect | null>(null);
  let copyLinkStatus = $state<"idle" | "copied" | "failed">("idle");
  let copyLinkTimer: ReturnType<typeof setTimeout> | null = null;
  let destinationPickerLoadState = $state<LazyComponentLoadState<
    "destination-picker",
    LoadedNotesEditorPanel
  > | null>(null);
  const visibleCommentCount = $derived(unreadCommentCount > 0 ? unreadCommentCount : commentCount);
  const visibleCommentLabel = $derived(
    unreadCommentCount > 0
      ? t("notes.blockUnreadCommentsCount", unreadCommentCount, commentCount)
      : t("notes.blockCommentsCount", commentCount),
  );
  const actionMenuStyle = $derived(
    actionMenuTriggerRect && typeof window !== "undefined"
      ? notesBlockHandleActionMenuStyle({
        triggerRect: actionMenuTriggerRect,
        viewportWidth: window.innerWidth,
        viewportHeight: window.innerHeight,
      })
      : "",
  );
  const anyMenuOpen = $derived(menuOpen || moveMenuOpen);

  onDestroy(() => {
    if (copyLinkTimer) clearTimeout(copyLinkTimer);
  });

  $effect(() => {
    onMenuOpenChange?.(anyMenuOpen);
  });

  $effect(() => {
    if (!contextMenuRequest) return;
    const { x, y } = contextMenuRequest;
    actionMenuTriggerRect = { top: y, right: x, bottom: y, left: x };
    menuOpen = true;
    moveMenuOpen = false;
  });

  function currentMenuState(): NotesBlockHandleMenuState {
    return {
      menuOpen,
      moveMenuOpen,
    };
  }

  function applyMenuState(state: NotesBlockHandleMenuState): void {
    menuOpen = state.menuOpen;
    moveMenuOpen = state.moveMenuOpen;
  }

  function closeMenus(): void {
    applyMenuState(CLOSED_NOTES_BLOCK_HANDLE_MENUS);
  }

  function runAction(actionType: NotesBlockHandleAction, action: () => void): void {
    applyMenuState(notesBlockHandleMenuStateAfterAction(currentMenuState(), actionType));
    action();
  }

  function toggleMoveMenu(): void {
    applyMenuState(notesBlockHandleMenuStateAfterMoveToggle(currentMenuState()));
  }

  function moveToPage(pageId: string): void {
    runAction("move_to_page", () => onMoveToPage(pageId));
  }

  function moveToPageTarget(targetKey: string): void {
    const target = moveTargets.find((candidate) => candidate.key === targetKey);
    if (!target) return;
    moveToPage(target.id);
  }

  function selectColor(color: NotesColor): void {
    applyMenuState(notesBlockHandleMenuStateAfterAction(currentMenuState(), "color"));
    onColorSelect(color);
  }

  function resetCopyLinkStatusLater(): void {
    if (copyLinkTimer) clearTimeout(copyLinkTimer);
    copyLinkTimer = setTimeout(() => {
      copyLinkStatus = "idle";
      copyLinkTimer = null;
    }, 2_000);
  }

  async function copyLinkToBlock(): Promise<void> {
    applyMenuState(notesBlockHandleMenuStateAfterAction(currentMenuState(), "copy_link"));
    try {
      await onCopyLink();
      copyLinkStatus = "copied";
    } catch (error) {
      console.warn("copy notes block link failed", error);
      copyLinkStatus = "failed";
    }
    resetCopyLinkStatusLater();
  }

  function colorLabel(color: NotesColor): string {
    switch (color) {
      case "default":
        return t("notes.blockColor.default");
      case "gray":
        return t("notes.blockColor.gray");
      case "brown":
        return t("notes.blockColor.brown");
      case "orange":
        return t("notes.blockColor.orange");
      case "yellow":
        return t("notes.blockColor.yellow");
      case "green":
        return t("notes.blockColor.green");
      case "blue":
        return t("notes.blockColor.blue");
      case "purple":
        return t("notes.blockColor.purple");
      case "pink":
        return t("notes.blockColor.pink");
      case "red":
        return t("notes.blockColor.red");
      case "gray_background":
        return t("notes.blockColor.grayBackground");
      case "brown_background":
        return t("notes.blockColor.brownBackground");
      case "orange_background":
        return t("notes.blockColor.orangeBackground");
      case "yellow_background":
        return t("notes.blockColor.yellowBackground");
      case "green_background":
        return t("notes.blockColor.greenBackground");
      case "blue_background":
        return t("notes.blockColor.blueBackground");
      case "purple_background":
        return t("notes.blockColor.purpleBackground");
      case "pink_background":
        return t("notes.blockColor.pinkBackground");
      case "red_background":
        return t("notes.blockColor.redBackground");
    }
  }
  /** Loads the destination picker on first use. A retry rejects when the load fails again. */
  function requestDestinationPicker(retry = false): Promise<void> {
    if (!retry && destinationPickerLoadState) return Promise.resolve();
    const loadingState = beginLazyComponentLoad(destinationPickerLoadState, "destination-picker");
    destinationPickerLoadState = loadingState;
    const request = retry
      ? retryNotesEditorPanel("destination-picker")
      : loadNotesEditorPanel("destination-picker");
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
      console.error("load Notes block destination picker failed", error);
      if (retry) throw error;
    });
  }

  $effect(() => {
    if (moveMenuOpen) void requestDestinationPicker();
  });
</script>

<div
  class="notes-block-handle absolute z-10"
  use:dismissOnOutside={{ enabled: anyMenuOpen, onDismiss: closeMenus }}
>
  {#if commentCount > 0}
    <button
      class={`notes-block-handle-comment-button flex items-center justify-center gap-0.5 rounded text-muted-foreground hover:bg-accent hover:text-accent-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring ${
        unreadCommentCount > 0 ? "bg-primary/10 text-primary" : ""
      }`}
      type="button"
      aria-label={visibleCommentLabel}
      data-app-tooltip={visibleCommentLabel}
      onclick={() => runAction("comment", onOpenComments ?? onComment)}
    >
      <MessageSquare class="size-3.5 shrink-0" />
      <span class="min-w-0 text-[0.65rem] font-medium leading-none">{visibleCommentCount}</span>
    </button>
  {/if}

  {#if menuOpen}
    <div
      class="surface-floating z-50 flex flex-col overflow-hidden"
      style={actionMenuStyle}
      role="menu"
      tabindex="-1"
      data-app-floating-surface
      onmousedown={(event) => {
        const target = event.target;
        if (!(target instanceof HTMLInputElement)) event.preventDefault();
      }}
    >
      <div class="surface-floating-body min-h-0 flex-1 overflow-auto" use:scrollEdgeFadeAction>
      {#if canTurnInto}
      <button
        class="menu-item"
        type="button"
        role="menuitem"
        onclick={() => runAction("turn_into", onTurnInto)}
      >
        <Pilcrow class="size-4 shrink-0" />
        <span class="min-w-0 truncate">{t("notes.turnInto")}</span>
      </button>
      {/if}
      {#if canSetColor}
        <div class="menu-separator" role="separator"></div>
        <div class="menu-label">
          {t("notes.color")}
        </div>
        {#each backgroundOnly ? NOTES_TEXT_COLORS.slice(0, 1) : NOTES_TEXT_COLORS as color}
          <button
            class="menu-item"
            type="button"
            role="menuitemradio"
            aria-checked={currentColor === color}
            onclick={() => selectColor(color)}
          >
            <span
              class="notes-color-swatch"
              style={notesBlockColorSwatchStyle(color)}
              aria-hidden="true"
            >
              A
            </span>
            <span class="min-w-0 flex-1 truncate">{colorLabel(color)}</span>
            {#if currentColor === color}
              <Check class="size-3.5 shrink-0" />
            {/if}
          </button>
        {/each}
        <div class="menu-label">
          {t("notes.backgroundColor")}
        </div>
        {#each NOTES_BACKGROUND_COLORS as color}
          <button
            class="menu-item"
            type="button"
            role="menuitemradio"
            aria-checked={currentBackgroundColor === color}
            onclick={() => selectColor(color)}
          >
            <span
              class="notes-color-swatch"
              style={notesBlockColorSwatchStyle(color)}
              aria-hidden="true"
            >
              A
            </span>
            <span class="min-w-0 flex-1 truncate">{colorLabel(color)}</span>
            {#if currentBackgroundColor === color}
              <Check class="size-3.5 shrink-0" />
            {/if}
          </button>
        {/each}
        <div class="menu-separator" role="separator"></div>
      {/if}
      <button
        class="menu-item"
        type="button"
        role="menuitem"
        onclick={() => {
          void copyLinkToBlock();
        }}
      >
        {#if copyLinkStatus === "copied"}
          <Check class="size-4 shrink-0" />
        {:else}
          <LinkIcon class="size-4 shrink-0" />
        {/if}
        <span class="min-w-0 truncate">
          {#if copyLinkStatus === "copied"}
            {t("notes.blockLinkCopied")}
          {:else if copyLinkStatus === "failed"}
            {t("notes.copyBlockLinkFailed")}
          {:else}
            {t("notes.copyBlockLink")}
          {/if}
        </span>
      </button>
      <button
        class="menu-item"
        type="button"
        role="menuitem"
        onclick={() => runAction("duplicate", onDuplicate)}
      >
        <Copy class="size-4 shrink-0" />
        <span class="min-w-0 truncate">{t("notes.duplicateBlock")}</span>
      </button>
      <button
        class="menu-item"
        type="button"
        role="menuitem"
        onclick={() => runAction("comment", onComment)}
      >
        <MessageSquare class="size-4 shrink-0" />
        <span class="min-w-0 flex-1 truncate">{t("notes.commentBlock")}</span>
        {#if commentCount > 0}
          <span class="shrink-0 text-panel-detail text-muted-foreground">
            {#if unreadCommentCount > 0}
              {t("notes.unreadCommentShortCount", unreadCommentCount)}
            {:else}
              {commentCount}
            {/if}
          </span>
        {/if}
      </button>
      <button
        class="menu-item"
        type="button"
        role="menuitem"
        onclick={() => runAction("move_up", onMoveUp)}
      >
        <ArrowUp class="size-4 shrink-0" />
        <span class="min-w-0 truncate">{t("notes.moveBlockUp")}</span>
      </button>
      <button
        class="menu-item"
        type="button"
        role="menuitem"
        onclick={() => runAction("move_down", onMoveDown)}
      >
        <ArrowDown class="size-4 shrink-0" />
        <span class="min-w-0 truncate">{t("notes.moveBlockDown")}</span>
      </button>
      <button
        class="menu-item"
        type="button"
        role="menuitem"
        aria-expanded={moveMenuOpen}
        onclick={toggleMoveMenu}
      >
        <FolderInput class="size-4 shrink-0" />
        <span class="min-w-0 truncate">{t("notes.moveBlockToPage")}</span>
      </button>
      {#if moveMenuOpen}
        <div class="border-y border-border bg-muted/25 py-1" role="group" aria-label={t("notes.moveBlockToPage")}>
          {#if destinationPickerLoadState?.status === "ready" && destinationPickerLoadState.component.kind === "destination-picker"}
            {@const NotesDestinationPickerList = destinationPickerLoadState.component.component}
            <NotesDestinationPickerList
              targets={moveTargets}
              searchLabel={t("notes.moveDestinationSearch")}
              searchPlaceholder={t("notes.moveDestinationSearchPlaceholder")}
              recentLabel={t("notes.recentDestinations")}
              pagesLabel={t("notes.allPages")}
              emptyLabel={t("notes.noMoveTargets")}
              optionLabel={(target) => t("notes.moveBlockToPageTarget", target.title)}
              onSelect={moveToPageTarget}
              onClose={() => {
                moveMenuOpen = false;
              }}
            />
          {:else if destinationPickerLoadState?.status === "failed"}
            <Notice class="p-2" message={t("common.viewLoadFailed", t("notes.moveBlockToPage"))} copy={false} onRetry={() => requestDestinationPicker(true)} />
          {:else}
            <Loading layout="rows" rows={3} />
          {/if}
        </div>
      {/if}
      <button
        class="menu-item menu-item-destructive"
        type="button"
        role="menuitem"
        onclick={() => runAction("delete", onDelete)}
      >
        <Trash2 class="size-4 shrink-0" />
        <span class="min-w-0 truncate">{t("notes.deleteBlock")}</span>
      </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .notes-block-handle {
    inset-inline-start: calc(var(--notes-depth) * 1.25rem - 2.25rem);
    top: 0.625rem;
  }

  .notes-block-handle-comment-button {
    block-size: 1.25rem;
    min-inline-size: 1.75rem;
    padding-inline: 0.2rem;
  }

  .notes-color-swatch {
    display: inline-flex;
    width: 1rem;
    height: 1rem;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--notes-color-swatch-border);
    border-radius: 0.25rem;
    background: var(--notes-color-swatch-bg);
    color: var(--notes-color-swatch-fg);
    font-size: calc(0.65rem * var(--type-scale));
    font-weight: 600;
    line-height: 1;
  }

  @media (any-pointer: coarse), (max-width: 420px) {
    .notes-block-handle-comment-button {
      block-size: 1.5rem;
      min-inline-size: 2rem;
    }
  }
</style>
