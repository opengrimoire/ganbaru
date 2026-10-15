<script lang="ts">
  import { tick } from "svelte";
  import Archive from "@lucide/svelte/icons/archive";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import EllipsisVertical from "@lucide/svelte/icons/ellipsis-vertical";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import Star from "@lucide/svelte/icons/star";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import MoveStepMenuItems from "$lib/components/ui/MoveStepMenuItems.svelte";
  import StarredIconBadge from "$lib/components/ui/StarredIconBadge.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { COMPACT_IDENTITY_EMOJI_SCALE, COMPACT_IDENTITY_ICON_SIZE, COMPACT_IDENTITY_ICON_STROKE_WIDTH } from "$lib/icon-sizing";
  import { shortcutModifierLabel } from "$lib/keyboard-shortcuts";
  import { normalizeProjectName } from "$lib/projects/text";
  import type { ProjectSidebarActions } from "$lib/projects/sidebar";
  import type { Project, ProjectGroup, ProjectLifecycleStatus, ProjectView, ProjectViewType } from "$lib/projects/types";
  import {
    isProjectTaskViewType,
    projectViewAddOptions,
    projectViewIdsMovedByStep,
    projectViewsFor,
    reorderedProjectViewIds,
  } from "$lib/projects/views";
  import { PROJECT_HOSTED_VIEW_TYPES } from "$lib/projects/types";
  import { PROJECT_VIEW_DRAG_SETTLE_MS } from "$lib/projects/view-drag";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { anchoredPopover } from "$lib/utils/anchored-popover";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import type { OpenInNewTabIntent } from "$lib/workspace-tabs/gestures";
  import ProjectIcon from "../ProjectIcon.svelte";
  import ProjectLifecycleIcon from "../ProjectLifecycleIcon.svelte";
  import ProjectViewDragCard from "./ProjectViewDragCard.svelte";
  import ProjectViewRow from "./ProjectViewRow.svelte";
  import { PROJECT_VIEW_TYPE_ICONS, projectViewLabel, projectViewTypeLabel } from "./project-view-presentation";
  import { createProjectViewDragController } from "./view-drag-controller.svelte";

  /**
   * One project in the Projects sidebar, shown like a Chat section: its heading collapses the section, adds views from
   * the plus menu, and opens the project menu to rename it, move it to another group, hide or archive it, and open its
   * settings. Its views follow as rows. Built-in projects show rename and move as unavailable.
   */
  let {
    project,
    selected,
    groupName = null,
    actions,
    moveGroups,
    views,
    activeViewId,
    collapsed,
    onCollapsedChange,
    onSelectView,
    onOpenViewInNewTab = undefined,
    onRename,
    onMoveToGroup,
    onSetStatus,
    onOpenSettings = undefined,
    onHeadingPointerDown = undefined,
    onMove = undefined,
    canMoveUp = false,
    canMoveDown = false,
  }: {
    project: Project;
    selected: boolean;
    /** Group shown after the name, such as in search results. */
    groupName?: string | null;
    actions: ProjectSidebarActions;
    /** Groups listed under Move to group, in sidebar order. */
    moveGroups: readonly ProjectGroup[];
    /** Views this window can open, in sidebar order. */
    views: readonly ProjectView[];
    /** Open view of the selected project. */
    activeViewId: string | null;
    collapsed: boolean;
    onCollapsedChange: (collapsed: boolean) => void;
    /** Opens a view, selecting its project first when needed. */
    onSelectView: (view: ProjectView) => void;
    /** Opens a view in a new workspace tab; rows leave out the gesture and menu entry without it. */
    onOpenViewInNewTab?: (view: ProjectView, intent: OpenInNewTabIntent) => void;
    onRename: (name: string) => Promise<void>;
    onMoveToGroup: (groupId: string) => Promise<void>;
    onSetStatus: (status: ProjectLifecycleStatus) => Promise<void>;
    /** Opens the project's settings; the item is left out where no settings panel exists. */
    onOpenSettings?: () => void;
    /** Starts dragging the section from its heading, while the custom sidebar order is in effect. */
    onHeadingPointerDown?: (event: PointerEvent) => void;
    /** Moves the project one place in the custom sidebar order; the menu leaves the move items out without it. */
    onMove?: (step: -1 | 1) => void;
    canMoveUp?: boolean;
    canMoveDown?: boolean;
  } = $props();

  const projects = getProjects();
  const { t } = getLocalization();
  const modifierLabel = shortcutModifierLabel();

  let menuOpen = $state(false);
  let moveOpen = $state(false);
  let addMenuOpen = $state(false);
  let editing = $state(false);
  let nameDraft = $state("");
  let pendingName = $state<string | null>(null);
  let removeCandidate = $state<ProjectView | null>(null);
  let actionButton = $state<HTMLButtonElement | null>(null);
  let addButton = $state<HTMLButtonElement | null>(null);
  let menuElement = $state<HTMLDivElement | null>(null);
  let addMenuElement = $state<HTMLDivElement | null>(null);
  let renameInput = $state<HTMLInputElement | null>(null);

  const displayName = $derived(pendingName ?? project.name);
  const lockedReason = $derived(actions.identityLocked ? t("projects.sidebar.identityLocked") : undefined);
  const moveUnavailableReason = $derived(lockedReason ?? (actions.canMoveToGroup ? undefined : t("projects.sidebar.noOtherGroup")));
  /** Every view of the project, including hosted views this window hides, so a hosted view already present stays unavailable. */
  const allProjectViews = $derived(projectViewsFor(projects.views, project.id, PROJECT_HOSTED_VIEW_TYPES));
  const addOptions = $derived(projectViewAddOptions(allProjectViews, projects.hostableViewTypes));
  const starred = $derived(projects.isProjectStarred(project.id));
  let sectionElement = $state<HTMLElement | null>(null);

  const viewDrag = createProjectViewDragController({
    getViews: () => views,
    isStarred: (viewId) => projects.isViewStarred(viewId),
    getContainer: () => sectionElement,
    move: (viewId, targetId, placement) =>
      reorder(reorderedProjectViewIds(projects.views, project.id, viewId, targetId, placement)),
  });

  $effect(() => viewDrag.destroy);

  /** Starred views stay ahead of the rest, so a view moves only past a neighbor with the same starred state. */
  function canMoveBy(index: number, step: -1 | 1): boolean {
    const view = views[index];
    const neighbor = views[index + step];
    if (!view || !neighbor) return false;
    return projects.isViewStarred(view.id) === projects.isViewStarred(neighbor.id);
  }

  function openMenu(): void {
    if (editing) return;
    addMenuOpen = false;
    menuOpen = true;
    moveOpen = false;
    void tick().then(() => menuElement?.querySelector<HTMLButtonElement>("button")?.focus());
  }

  function closeMenu(): void {
    menuOpen = false;
    moveOpen = false;
  }

  function openContextMenu(event: MouseEvent): void {
    event.preventDefault();
    openMenu();
  }

  function toggleAddMenu(): void {
    if (addMenuOpen) {
      addMenuOpen = false;
      return;
    }
    closeMenu();
    addMenuOpen = true;
    void tick().then(() => addMenuElement?.querySelector<HTMLButtonElement>("button:not([aria-disabled])")?.focus());
  }

  /** Runs a menu action after closing the menu, and reports a failed write with its context. */
  function runAction(description: string, action: () => Promise<void>): void {
    closeMenu();
    void action().catch((error: unknown) => {
      console.error(`Failed to ${description} from the Projects sidebar`, error);
    });
  }

  function beginRename(): void {
    if (!actions.canRename) return;
    closeMenu();
    nameDraft = project.name;
    editing = true;
    void tick().then(() => {
      renameInput?.focus();
      renameInput?.select();
    });
  }

  function saveRename(): void {
    if (!editing) return;
    editing = false;
    const name = normalizeProjectName(nameDraft);
    if (!name || name === project.name) return;
    pendingName = name;
    void onRename(name)
      .catch((error: unknown) => {
        console.error("Failed to rename a project from the Projects sidebar", error);
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

  /** Adds a view of the chosen type with its type label as name, then opens it. */
  async function addView(viewType: ProjectViewType): Promise<void> {
    addMenuOpen = false;
    try {
      const viewId = await projects.addView(project.id, viewType);
      const view = projects.viewsForProject(project.id).find((entry) => entry.id === viewId);
      if (!view) return;
      onCollapsedChange(false);
      onSelectView(view);
    } catch (error: unknown) {
      console.error("Failed to add a project view from the Projects sidebar", error);
    }
  }

  /** Saves a new view order and reports whether there was one to save. */
  function reorder(viewIds: string[] | null): boolean {
    if (!viewIds) return false;
    void projects.reorderViews(project.id, viewIds).catch((error: unknown) => {
      console.error("Failed to reorder project views from the Projects sidebar", error);
    });
    return true;
  }

  function confirmRemove(): void {
    const view = removeCandidate;
    removeCandidate = null;
    if (!view) return;
    void projects.removeView(view.id).catch((error: unknown) => {
      console.error("Failed to remove a project view from the Projects sidebar", error);
    });
  }

  /** Names a view of the selected project with the Ctrl and digit shortcut that opens it. */
  function viewTooltip(view: ProjectView, index: number): string | undefined {
    if (!selected || index >= 9) return undefined;
    return t("projects.sidebar.placeWithShortcut", projectViewLabel(t, view), `${modifierLabel}+${index + 1}`);
  }
</script>

<section
  bind:this={sectionElement}
  class="project-section"
  class:project-section-inactive={project.status !== "active"}
  aria-label={displayName}
  data-project-id={project.id}
>
  <div class="section-heading" class:section-renaming={editing}>
    {#if editing}
      <ProjectIcon
        name={project.icon}
        size={COMPACT_IDENTITY_ICON_SIZE}
        strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH}
        emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
      />
      <input
        bind:this={renameInput}
        bind:value={nameDraft}
        class="section-rename-input"
        data-app-shortcuts="ignore"
        aria-label={t("projects.sidebar.name")}
        placeholder={t("projects.sidebar.name")}
        onkeydown={handleRenameKeydown}
        onblur={saveRename}
      />
    {:else}
      <button
        type="button"
        class="section-toggle"
        aria-label={collapsed ? t("projects.sidebar.expandProject", displayName) : t("projects.sidebar.collapseProject", displayName)}
        aria-expanded={!collapsed}
        onclick={() => onCollapsedChange(!collapsed)}
        oncontextmenu={openContextMenu}
        onpointerdown={onHeadingPointerDown}
      >
        <StarredIconBadge {starred}>
          <ProjectIcon
            name={project.icon}
            size={COMPACT_IDENTITY_ICON_SIZE}
            strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH}
            emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
          />
        </StarredIconBadge>
        <span class="section-name">{displayName}</span>
        {#if groupName}
          <span class="section-group">{groupName}</span>
        {/if}
        <ProjectLifecycleIcon status={project.status} size={12} class="text-current opacity-70" />
        {#if collapsed}<ChevronRight class="section-chevron" />{:else}<ChevronDown class="section-chevron" />{/if}
      </button>
      <button
        bind:this={addButton}
        type="button"
        class="section-create-action"
        aria-label={t("projects.sidebar.addViewTo", displayName)}
        data-app-tooltip={t("projects.sidebar.addView")}
        aria-haspopup="menu"
        aria-expanded={addMenuOpen}
        onclick={toggleAddMenu}
      >
        <Plus />
      </button>
      <button
        bind:this={actionButton}
        type="button"
        class="explorer-icon explorer-row-action"
        aria-label={t("projects.sidebar.projectActions", displayName)}
        data-app-tooltip-disabled="true"
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        onclick={() => (menuOpen ? closeMenu() : openMenu())}
      >
        <EllipsisVertical size={14} />
      </button>
    {/if}
  </div>

  {#if !collapsed}
    {#each views as view, index (view.id)}
      <ProjectViewRow
        {view}
        active={selected && activeViewId === view.id}
        tooltip={viewTooltip(view, index)}
        starred={projects.isViewStarred(view.id)}
        canMoveUp={canMoveBy(index, -1)}
        canMoveDown={canMoveBy(index, 1)}
        onSelect={() => onSelectView(view)}
        onOpenInNewTab={onOpenViewInNewTab ? (intent) => onOpenViewInNewTab(view, intent) : undefined}
        onToggleStar={(next) => projects.setViewStarred(view.id, next)}
        onRename={(name) => projects.renameView(view.id, name)}
        onMove={(step) => reorder(projectViewIdsMovedByStep(projects.views, views, view.id, step))}
        onRemove={() => {
          removeCandidate = view;
        }}
        shift={viewDrag.shiftFor(view.id)}
        shiftAnimated={viewDrag.shiftAnimated}
        lifted={viewDrag.draggedId === view.id}
        onPointerDown={(event, row) => viewDrag.pointerDown(event, view, row)}
      />
    {:else}
      <p class="explorer-row-label px-2 py-1 text-muted-foreground">{t("projects.sidebar.noViews")}</p>
    {/each}
  {/if}
</section>

{#if viewDrag.card}
  <ProjectViewDragCard
    view={viewDrag.card.view}
    active={selected && activeViewId === viewDrag.card.view.id}
    starred={projects.isViewStarred(viewDrag.card.view.id)}
    frame={viewDrag.card.frame}
    phase={viewDrag.card.phase}
    durationMs={PROJECT_VIEW_DRAG_SETTLE_MS}
  />
{/if}

{#if addMenuOpen}
  <div
    bind:this={addMenuElement}
    use:portal
    use:anchoredPopover={{ anchor: addButton ?? undefined, onOutside: () => (addMenuOpen = false) }}
    class="surface-floating fixed z-90 flex w-floating-sm flex-col overflow-hidden"
    role="menu"
    tabindex="-1"
    aria-label={t("projects.sidebar.addViewTo", displayName)}
    data-app-floating-surface
    onkeydown={(event) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      addMenuOpen = false;
      addButton?.focus();
    }}
  >
    <div class="surface-floating-body flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden" use:scrollEdgeFadeAction>
      {#each addOptions as option (option.viewType)}
        {@const Icon = PROJECT_VIEW_TYPE_ICONS[option.viewType]}
        <button
          type="button"
          role="menuitem"
          class="menu-item"
          aria-disabled={option.available ? undefined : "true"}
          data-app-tooltip={option.available ? undefined : t("projects.sidebar.viewAlreadyAdded")}
          data-project-add-view={option.viewType}
          onclick={() => {
            if (option.available) void addView(option.viewType);
          }}
        >
          <Icon />
          <span>{projectViewTypeLabel(t, option.viewType)}</span>
        </button>
      {/each}
    </div>
  </div>
{/if}

{#if menuOpen}
  <div
    bind:this={menuElement}
    use:portal
    use:anchoredPopover={{ anchor: actionButton ?? undefined, onOutside: closeMenu }}
    class="surface-floating fixed z-90 flex w-floating-sm flex-col overflow-hidden"
    role="menu"
    tabindex="-1"
    aria-label={t("projects.sidebar.projectActions", displayName)}
    data-app-floating-surface
    onkeydown={(event) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      closeMenu();
      actionButton?.focus();
    }}
  >
    <div class="surface-floating-body flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden" use:scrollEdgeFadeAction>
      <button
        type="button"
        role="menuitem"
        class="menu-item"
        aria-disabled={actions.canRename ? undefined : "true"}
        data-app-tooltip={lockedReason}
        onclick={beginRename}
      >
        <Pencil />
        <span>{t("projects.sidebar.rename")}</span>
      </button>
      <button
        type="button"
        role="menuitem"
        class="menu-item"
        aria-disabled={actions.canMoveToGroup ? undefined : "true"}
        aria-expanded={actions.canMoveToGroup ? moveOpen : undefined}
        data-app-tooltip={moveUnavailableReason}
        onclick={() => {
          if (actions.canMoveToGroup) moveOpen = !moveOpen;
        }}
      >
        <FolderInput />
        <span class="min-w-0 flex-1 truncate">{t("projects.sidebar.moveToGroup")}</span>
        {#if actions.canMoveToGroup}
          {#if moveOpen}<ChevronDown />{:else}<ChevronRight />{/if}
        {/if}
      </button>
      {#if moveOpen}
        <div class="flex max-h-48 flex-col overflow-y-auto border-y border-border bg-muted/25 py-1" role="group" aria-label={t("projects.sidebar.moveToGroup")}>
          {#each moveGroups as group (group.id)}
            <button
              type="button"
              role="menuitem"
              class="menu-item"
              onclick={() => runAction("move a project to another group", () => onMoveToGroup(group.id))}
            >
              <ProjectIcon
                name={group.icon}
                size={COMPACT_IDENTITY_ICON_SIZE}
                strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH}
                emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
              />
              <span class="min-w-0 flex-1 truncate">{group.name}</span>
            </button>
          {/each}
        </div>
      {/if}
      {#if onMove}
        <MoveStepMenuItems {canMoveUp} {canMoveDown} onMove={(step) => onMove?.(step)} onClose={closeMenu} />
      {/if}
      <button
        type="button"
        role="menuitem"
        class="menu-item"
        onclick={() => {
          closeMenu();
          projects.setProjectStarred(project.id, !starred);
        }}
      >
        <Star class={starred ? "fill-current text-primary" : undefined} />
        <span>{starred ? t("common.removeFromFavorites") : t("common.addToFavorites")}</span>
      </button>
      {#if onOpenSettings}
        <button
          type="button"
          role="menuitem"
          class="menu-item"
          onclick={() => {
            closeMenu();
            onOpenSettings?.();
          }}
        >
          <Settings2 />
          <span>{t("projects.sidebar.settings")}</span>
        </button>
      {/if}
      <div class="menu-separator" aria-hidden="true"></div>
      <button
        type="button"
        role="menuitem"
        class="menu-item"
        onclick={() => runAction("change a project's visibility", () => onSetStatus(actions.visibility.status))}
      >
        {#if actions.visibility.label === "show"}<Eye />{:else}<EyeOff />{/if}
        <span>{actions.visibility.label === "show" ? t("projects.sidebar.show") : t("projects.sidebar.hide")}</span>
      </button>
      <button
        type="button"
        role="menuitem"
        class="menu-item"
        onclick={() => runAction("archive or restore a project", () => onSetStatus(actions.archive.status))}
      >
        {#if actions.archive.label === "restore"}<ArchiveRestore />{:else}<Archive />{/if}
        <span>{actions.archive.label === "restore" ? t("projects.sidebar.restore") : t("projects.sidebar.archive")}</span>
      </button>
    </div>
  </div>
{/if}

{#if removeCandidate}
  <ConfirmDialog
    title={t("projects.sidebar.removeViewTitle", projectViewLabel(t, removeCandidate))}
    message={isProjectTaskViewType(removeCandidate.viewType)
      ? t("projects.sidebar.removeTaskViewMessage")
      : t("projects.sidebar.removeHostedViewMessage")}
    confirmLabel={t("projects.sidebar.removeViewConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirmRemove}
    onCancel={() => {
      removeCandidate = null;
    }}
  />
{/if}

<style>
  .section-heading {
    --section-heading-gap: 0.2rem;
    --section-heading-inset: 0.5rem;
    display: flex;
    align-items: center;
    gap: var(--section-heading-gap);
    padding-inline: var(--section-heading-inset) 0;
    color: var(--muted-foreground);
  }

  .section-heading.section-renaming {
    padding-inline-end: var(--section-heading-inset);
    border-radius: 0.375rem;
    background: var(--explorer-row-hover, color-mix(in oklab, var(--accent) 50%, transparent));
    color: var(--foreground);
  }

  .section-rename-input {
    width: 0;
    min-width: 0;
    min-height: var(--explorer-action-size);
    flex: 1;
    border: 0;
    background: transparent;
    color: var(--foreground);
    caret-color: var(--primary);
    font: inherit;
    outline: none;
  }

  .section-heading > button {
    display: flex;
    min-width: var(--explorer-action-size);
    min-height: var(--explorer-action-size);
    align-items: center;
    justify-content: center;
    gap: 0.375rem;
    border-radius: 0.3rem;
  }

  .section-heading > .section-toggle {
    min-width: 0;
    flex: 1;
    justify-content: flex-start;
    margin-inline-start: calc(-1 * var(--section-heading-inset));
    padding-inline: var(--section-heading-inset);
    border-radius: 0.375rem;
  }

  .section-heading > .section-toggle:active {
    background: var(--explorer-icon-hover, var(--accent));
    color: var(--foreground);
  }

  @media (hover: hover) {
    .section-heading > .section-toggle:hover {
      background: var(--explorer-icon-hover, var(--accent));
      color: var(--foreground);
    }
  }

  .section-heading > .section-create-action {
    margin-inline-end: calc(-1 * var(--section-heading-gap));
  }

  .section-heading > .section-create-action:is(:hover, :focus-visible, [aria-expanded="true"]) {
    background: transparent;
    color: var(--foreground);
  }

  .section-heading > .explorer-row-action {
    margin-inline-end: 0;
    opacity: 1;
  }

  /* Touch headings take the row action size and end inset, so their actions line up with the view rows below. */
  :global(.explorer-touch) .section-heading > button:not(.section-toggle) {
    min-width: var(--touch-target-min);
    min-height: var(--touch-target-min);
  }

  :global(.explorer-touch) .section-heading > .explorer-row-action {
    margin-inline-end: var(--explorer-row-action-end-inset);
  }

  .section-name,
  .section-group {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .section-group {
    max-width: 50%;
    flex-shrink: 0;
    opacity: 0.75;
  }

  .section-toggle :global(.section-chevron) {
    flex: 0 0 auto;
    margin-inline-start: -0.2rem;
  }

  .project-section-inactive > .section-heading .section-name {
    opacity: 0.7;
  }
</style>
