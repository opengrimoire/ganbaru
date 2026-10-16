<script lang="ts">
  import { tick } from "svelte";
  import AppWindow from "@lucide/svelte/icons/app-window";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import EllipsisVertical from "@lucide/svelte/icons/ellipsis-vertical";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Star from "@lucide/svelte/icons/star";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import ExplorerRowStar from "$lib/components/ui/ExplorerRowStar.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectView } from "$lib/projects/types";
  import { PROJECT_VIEW_DRAG_SETTLE_MS } from "$lib/projects/view-drag";
  import { anchoredPopover } from "$lib/utils/anchored-popover";
  import { portal } from "$lib/utils/portal";
  import { openInNewTabIntent, preventMiddleClickAutoscroll, type OpenInNewTabIntent } from "$lib/workspace-tabs/gestures";
  import { PROJECT_VIEW_TYPE_ICONS, projectViewLabel, projectViewTypeLabel } from "./project-view-presentation";

  /**
   * One view inside a project's sidebar section, like a channel inside a Chat section. Its menu renames, moves, stars, and
   * removes the view; the icon always follows the view type. Pressing a row and dragging it reorders it within its project.
   */
  let {
    view,
    active,
    tooltip = undefined,
    starred,
    canMoveUp,
    canMoveDown,
    onSelect,
    onRename,
    onToggleStar,
    onMove,
    onRemove,
    shift = 0,
    shiftAnimated = true,
    lifted = false,
    onPointerDown,
    onOpenInNewTab = undefined,
  }: {
    view: ProjectView;
    active: boolean;
    /** Hover text, such as the view name with its keyboard shortcut. */
    tooltip?: string;
    /** Whether the view is starred, which lists it first and shows the star mark. */
    starred: boolean;
    canMoveUp: boolean;
    canMoveDown: boolean;
    onSelect: () => void;
    /** Saves a new name; a blank name restores the type label. */
    onRename: (name: string) => Promise<void>;
    onToggleStar: (starred: boolean) => void;
    onMove: (step: -1 | 1) => void;
    onRemove: () => void;
    /** Vertical offset in pixels while another row is dragged past this one. */
    shift?: number;
    /** Whether the offset slides rather than jumps. */
    shiftAnimated?: boolean;
    /** Whether this row is lifted out as a drag card, leaving its slot empty. */
    lifted?: boolean;
    /** Starts tracking a press that may become a drag. */
    onPointerDown: (event: PointerEvent, row: HTMLElement) => void;
    /** Opens the view in a new workspace tab; leaving it out hides the gesture and the menu entry. */
    onOpenInNewTab?: (intent: OpenInNewTabIntent) => void;
  } = $props();

  const { t } = getLocalization();

  let menuOpen = $state(false);
  let editing = $state(false);
  let nameDraft = $state("");
  let pendingName = $state<string | null>(null);
  let actionButton = $state<HTMLButtonElement | null>(null);
  let menuElement = $state<HTMLDivElement | null>(null);
  let renameInput = $state<HTMLInputElement | null>(null);

  const Icon = $derived(PROJECT_VIEW_TYPE_ICONS[view.viewType]);
  const label = $derived(pendingName ?? projectViewLabel(t, view));

  function openMenu(): void {
    if (editing) return;
    menuOpen = true;
    void tick().then(() => menuElement?.querySelector<HTMLButtonElement>("button:not([aria-disabled])")?.focus());
  }

  function closeMenu(): void {
    menuOpen = false;
  }

  function openContextMenu(event: MouseEvent): void {
    event.preventDefault();
    openMenu();
  }

  function runMenuAction(action: () => void): void {
    closeMenu();
    action();
  }

  /** Route a click to a new tab when the gesture asks for one, else select the view. */
  function handleLabelClick(event: MouseEvent): void {
    const intent = onOpenInNewTab ? openInNewTabIntent(event) : null;
    if (intent) {
      event.preventDefault();
      onOpenInNewTab?.(intent);
      return;
    }
    if (event.type === "click") onSelect();
  }

  function beginRename(): void {
    closeMenu();
    nameDraft = projectViewLabel(t, view);
    editing = true;
    void tick().then(() => {
      renameInput?.focus();
      renameInput?.select();
    });
  }

  /** Stores the draft unless it repeats the current name, so an unnamed view keeps following its localized type label. */
  function saveRename(): void {
    if (!editing) return;
    editing = false;
    const name = nameDraft.trim();
    const unchanged = view.name === undefined
      ? name.length === 0 || name === projectViewTypeLabel(t, view.viewType)
      : name === view.name;
    if (unchanged) return;
    pendingName = name || projectViewTypeLabel(t, view.viewType);
    void onRename(name)
      .catch((error: unknown) => {
        console.error("Failed to rename a project view from the Projects sidebar", error);
      })
      .finally(() => {
        pendingName = null;
      });
  }

  function handleRenameKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter") {
      event.preventDefault();
      saveRename();
      void tick().then(() => actionButton?.focus());
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      editing = false;
      void tick().then(() => actionButton?.focus());
    }
  }
</script>

<div
  class="project-view-row explorer-row flex min-w-0 items-center"
  class:explorer-selected={active}
  class:shift-animated={shiftAnimated}
  class:invisible={lifted}
  style:translate={shift ? `0 ${shift}px` : undefined}
  style:--project-view-shift-duration={`${PROJECT_VIEW_DRAG_SETTLE_MS}ms`}
  role="group"
  aria-label={label}
  data-project-view-id={view.id}
  data-project-place={view.viewType}
  onpointerdown={(event) => {
    if (!editing) onPointerDown(event, event.currentTarget);
  }}
>
  {#if editing}
    <div class="flex min-w-0 flex-1 items-center gap-1.5 px-2">
      <Icon class="explorer-row-icon" aria-hidden="true" />
      <input
        bind:this={renameInput}
        bind:value={nameDraft}
        class="explorer-row-label min-w-0 flex-1 bg-transparent text-inherit caret-primary outline-none placeholder:text-muted-foreground"
        data-app-shortcuts="ignore"
        aria-label={t("projects.sidebar.viewName")}
        placeholder={projectViewTypeLabel(t, view.viewType)}
        onkeydown={handleRenameKeydown}
        onblur={saveRename}
      />
    </div>
  {:else}
    <button
      type="button"
      class="explorer-row-label flex min-w-0 flex-1 items-center gap-1.5 self-stretch px-2 text-left text-inherit"
      aria-current={active ? "page" : undefined}
      data-app-tooltip={tooltip}
      onmousedown={preventMiddleClickAutoscroll}
      onclick={handleLabelClick}
      onauxclick={handleLabelClick}
      oncontextmenu={openContextMenu}
    >
      <Icon class="explorer-row-icon" aria-hidden="true" />
      <span class="min-w-0 flex-1 truncate">{label}</span>
    </button>
    <span class="explorer-row-trailing">
      {#if starred}
        <ExplorerRowStar />
      {/if}
      <button
        bind:this={actionButton}
        type="button"
        class="explorer-icon explorer-row-action"
        aria-label={t("projects.sidebar.viewActions", label)}
        data-app-tooltip-disabled="true"
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        onclick={() => (menuOpen ? closeMenu() : openMenu())}
      >
        <EllipsisVertical size={14} />
      </button>
    </span>
  {/if}
</div>

{#if menuOpen}
  <div
    bind:this={menuElement}
    use:portal
    use:anchoredPopover={{ anchor: actionButton ?? undefined, onOutside: closeMenu }}
    class="surface-floating fixed z-90 flex w-floating-sm flex-col overflow-hidden"
    role="menu"
    tabindex="-1"
    aria-label={t("projects.sidebar.viewActions", label)}
    data-app-floating-surface
    onkeydown={(event) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      closeMenu();
      actionButton?.focus();
    }}
  >
    <div class="surface-floating-body flex flex-col">
      {#if onOpenInNewTab}
        <button type="button" role="menuitem" class="menu-item" onclick={() => runMenuAction(() => onOpenInNewTab?.({ background: false }))}>
          <AppWindow />
          <span>{t("titleBar.tab.openInNewTab")}</span>
        </button>
      {/if}
      <button type="button" role="menuitem" class="menu-item" onclick={beginRename}>
        <Pencil />
        <span>{t("projects.sidebar.renameView")}</span>
      </button>
      <button
        type="button"
        role="menuitem"
        class="menu-item"
        aria-disabled={canMoveUp ? undefined : "true"}
        onclick={() => {
          if (canMoveUp) runMenuAction(() => onMove(-1));
        }}
      >
        <ArrowUp />
        <span>{t("projects.sidebar.moveViewUp")}</span>
      </button>
      <button
        type="button"
        role="menuitem"
        class="menu-item"
        aria-disabled={canMoveDown ? undefined : "true"}
        onclick={() => {
          if (canMoveDown) runMenuAction(() => onMove(1));
        }}
      >
        <ArrowDown />
        <span>{t("projects.sidebar.moveViewDown")}</span>
      </button>
      <button type="button" role="menuitem" class="menu-item" onclick={() => runMenuAction(() => onToggleStar(!starred))}>
        <Star class={starred ? "fill-current text-primary" : undefined} />
        <span>{starred ? t("common.removeFromFavorites") : t("common.addToFavorites")}</span>
      </button>
      <div class="menu-separator" aria-hidden="true"></div>
      <button type="button" role="menuitem" class="menu-item menu-item-destructive" onclick={() => runMenuAction(onRemove)}>
        <Trash2 />
        <span>{t("projects.sidebar.removeView")}</span>
      </button>
    </div>
  </div>
{/if}

<style>
  .project-view-row.shift-animated {
    transition: translate var(--project-view-shift-duration) cubic-bezier(0.2, 0, 0, 1);
  }

  :global(html.project-view-dragging) .project-view-row:hover:not(.explorer-selected) {
    background: transparent;
  }

  :global(html.project-view-dragging),
  :global(html.project-view-dragging *) {
    cursor: grabbing !important;
    user-select: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .project-view-row.shift-animated {
      transition: none;
    }
  }
</style>
