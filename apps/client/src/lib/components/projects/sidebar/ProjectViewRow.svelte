<script lang="ts">
  import { tick } from "svelte";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import EllipsisVertical from "@lucide/svelte/icons/ellipsis-vertical";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Star from "@lucide/svelte/icons/star";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import ExplorerRowStar from "$lib/components/ui/ExplorerRowStar.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectView } from "$lib/projects/types";
  import { anchoredPopover } from "$lib/utils/anchored-popover";
  import { portal } from "$lib/utils/portal";
  import { PROJECT_VIEW_TYPE_ICONS, projectViewLabel, projectViewTypeLabel } from "./project-view-presentation";

  /**
   * One view inside a project's sidebar section, like a channel inside a Chat section. Its menu renames, moves, stars, and
   * removes the view; the icon always follows the view type. Rows of the same project reorder by dragging.
   */
  let {
    view,
    active,
    tooltip = undefined,
    starred,
    canMoveUp,
    canMoveDown,
    dropEnabled,
    onSelect,
    onRename,
    onToggleStar,
    onMove,
    onRemove,
    onDragStart,
    onDragEnd,
    onDrop,
  }: {
    view: ProjectView;
    active: boolean;
    /** Hover text, such as the view name with its keyboard shortcut. */
    tooltip?: string;
    /** Whether the view is starred, which lists it first and shows the star mark. */
    starred: boolean;
    canMoveUp: boolean;
    canMoveDown: boolean;
    /** Whether the view being dragged may land on this row. */
    dropEnabled: boolean;
    onSelect: () => void;
    /** Saves a new name; a blank name restores the type label. */
    onRename: (name: string) => Promise<void>;
    onToggleStar: (starred: boolean) => void;
    onMove: (step: -1 | 1) => void;
    onRemove: () => void;
    onDragStart: () => void;
    onDragEnd: () => void;
    onDrop: (placement: "before" | "after") => void;
  } = $props();

  const { t } = getLocalization();

  let menuOpen = $state(false);
  let editing = $state(false);
  let nameDraft = $state("");
  let pendingName = $state<string | null>(null);
  let dropPlacement = $state<"before" | "after" | null>(null);
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

  function handleDragStart(event: DragEvent): void {
    if (editing) {
      event.preventDefault();
      return;
    }
    event.dataTransfer?.setData("text/plain", label);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
    onDragStart();
  }

  function handleDragOver(event: DragEvent): void {
    if (!dropEnabled) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
    dropPlacement = event.clientY < bounds.top + bounds.height / 2 ? "before" : "after";
  }

  function handleDrop(event: DragEvent): void {
    const placement = dropPlacement;
    dropPlacement = null;
    if (!dropEnabled || !placement) return;
    event.preventDefault();
    onDrop(placement);
  }
</script>

<div
  class="project-view-row explorer-row flex min-w-0 items-center"
  class:explorer-selected={active}
  class:drop-before={dropPlacement === "before"}
  class:drop-after={dropPlacement === "after"}
  role="group"
  aria-label={label}
  draggable={!editing}
  data-project-view-id={view.id}
  data-project-place={view.viewType}
  ondragstart={handleDragStart}
  ondragend={() => {
    dropPlacement = null;
    onDragEnd();
  }}
  ondragover={handleDragOver}
  ondragleave={() => {
    dropPlacement = null;
  }}
  ondrop={handleDrop}
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
      onclick={onSelect}
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
  .project-view-row {
    position: relative;
  }

  .project-view-row.drop-before::before,
  .project-view-row.drop-after::after {
    position: absolute;
    inset-inline: 0.5rem;
    height: 2px;
    border-radius: 1px;
    background: var(--primary);
    content: "";
    pointer-events: none;
  }

  .project-view-row.drop-before::before {
    top: -1px;
  }

  .project-view-row.drop-after::after {
    bottom: -1px;
  }
</style>
