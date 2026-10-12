<script lang="ts">
  import { tick } from "svelte";
  import Bookmark from "@lucide/svelte/icons/bookmark";
  import ChevronsLeft from "@lucide/svelte/icons/chevrons-left";
  import ChevronsRight from "@lucide/svelte/icons/chevrons-right";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import SquarePen from "@lucide/svelte/icons/square-pen";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectPlace } from "$lib/projects/places";
  import type { ProjectSavedTaskView } from "$lib/projects/types";
  import { getProjects } from "$lib/stores/projects.svelte";
  import type { ProjectTaskQueryController } from "../task-query-controller.svelte";
  import ProjectPlacesNav from "./ProjectPlacesNav.svelte";

  /**
   * The docked Projects explorer: task actions, the project places, and the saved views of the selected project.
   * Collapsing it keeps a narrow rail with the same places.
   */
  let {
    taskQuery,
    places,
    activePlace,
    onSelectPlace,
    onCreateTask,
    onOpenFinder,
    onProjectSelected,
    onSavedViewApplied,
  }: {
    taskQuery: ProjectTaskQueryController;
    places: readonly ProjectPlace[];
    activePlace: ProjectPlace;
    onSelectPlace: (place: ProjectPlace) => void;
    onCreateTask: () => void;
    onOpenFinder: () => void;
    onProjectSelected: () => void;
    onSavedViewApplied: () => void;
  } = $props();

  const projects = getProjects();
  const { t } = getLocalization();

  const projectId = $derived(projects.selectedProject?.id ?? null);
  const collapsed = $derived(projects.sidebarCollapsed);
  const savedViews = $derived(taskQuery.savedViews);
  const savedViewsLoaded = $derived(
    Boolean(projectId && projects.isProjectOptionalDataLoaded("saved_views", projectId)),
  );
  const savedViewBusy = $derived(taskQuery.savedViewSaving || taskQuery.listColumnsSaving);

  let creatingView = $state(false);
  let viewNameInput = $state<HTMLInputElement | null>(null);
  let expandButton = $state<HTMLButtonElement | null>(null);
  let collapseButton = $state<HTMLButtonElement | null>(null);

  $effect(() => {
    const id = projectId;
    if (!id) return;
    void projects.ensureProjectToolbarData(id).catch((error) => {
      console.error("load project saved views failed", error);
    });
  });

  $effect(() => {
    void projectId;
    creatingView = false;
  });

  function setCollapsed(value: boolean): void {
    projects.sidebarCollapsed = value;
    void tick().then(() => (value ? expandButton : collapseButton)?.focus());
  }

  function toggleViewDraft(): void {
    creatingView = !creatingView;
    taskQuery.savedViewError = null;
    if (creatingView) void tick().then(() => viewNameInput?.focus());
  }

  async function saveView(): Promise<void> {
    await taskQuery.saveCurrentView();
    if (!taskQuery.savedViewError) creatingView = false;
  }

  function applyView(view: ProjectSavedTaskView): void {
    void taskQuery.applySavedView(view).finally(onSavedViewApplied);
  }
</script>

<aside
  class="explorer-sidebar projects-sidebar relative h-full min-h-0 shrink-0 overflow-hidden"
  class:projects-sidebar-collapsed={collapsed}
  aria-label={t("projects.sidebar.label")}
  data-projects-sidebar
>
  <div class="projects-sidebar-rail absolute inset-y-0 left-0 z-10 flex w-11 flex-col" aria-hidden={!collapsed} inert={!collapsed}>
    <div class="explorer-toolbar justify-center">
      <button
        bind:this={expandButton}
        type="button"
        class="explorer-icon"
        aria-label={t("projects.sidebar.expand")}
        aria-expanded="false"
        data-app-tooltip={t("projects.sidebar.expand")}
        onclick={() => setCollapsed(false)}
      >
        <ChevronsRight />
      </button>
    </div>
    {#if collapsed}
      <ProjectPlacesNav variant="rail" {places} {activePlace} {onSelectPlace} {onProjectSelected} />
    {/if}
  </div>

  <div class="projects-sidebar-content flex h-full min-h-0 flex-col" inert={collapsed}>
    <div class="explorer-toolbar">
      <button
        type="button"
        class="explorer-icon"
        aria-label={t("projects.header.addTask")}
        data-app-tooltip={t("projects.header.addTask")}
        disabled={!projectId}
        onclick={onCreateTask}
      >
        <SquarePen />
      </button>
      <button
        type="button"
        class="explorer-icon"
        aria-label={t("projects.finder.open")}
        data-app-tooltip={`${t("projects.finder.open")} (${t("projects.finder.shortcut")})`}
        disabled={!projectId}
        onclick={onOpenFinder}
      >
        <Search />
      </button>
      <div class="flex-1"></div>
      <button
        bind:this={collapseButton}
        type="button"
        class="explorer-icon"
        aria-label={t("projects.sidebar.collapse")}
        aria-expanded="true"
        data-app-tooltip={t("projects.sidebar.collapse")}
        onclick={() => setCollapsed(true)}
      >
        <ChevronsLeft />
      </button>
    </div>

    {#if !collapsed}
      <ProjectPlacesNav variant="list" {places} {activePlace} {onSelectPlace} {onProjectSelected} />
    {/if}

    {#if projectId}
      <section class="min-h-0 flex-1 overflow-y-auto px-2 pb-2" aria-label={t("projects.savedViews.title")} data-project-saved-views>
        <div class="section-heading projects-sidebar-heading">
          <span class="min-w-0 flex-1 truncate">{t("projects.savedViews.title")}</span>
          <button
            type="button"
            class="explorer-icon"
            class:explorer-icon-active={creatingView}
            aria-label={t("projects.sidebar.saveView")}
            aria-expanded={creatingView}
            data-app-tooltip={t("projects.sidebar.saveView")}
            disabled={!savedViewsLoaded}
            onclick={toggleViewDraft}
          >
            <Plus />
          </button>
        </div>

        {#if creatingView}
          <form class="mb-1 flex gap-1" onsubmit={(event) => { event.preventDefault(); void saveView(); }}>
            <input
              bind:this={viewNameInput}
              bind:value={taskQuery.savedViewNameDraft}
              class="field min-w-0 flex-1"
              maxlength="80"
              placeholder={t("projects.savedViews.namePlaceholder")}
              aria-label={t("projects.savedViews.namePlaceholder")}
              onkeydown={(event) => {
                if (event.key !== "Escape") return;
                event.stopPropagation();
                toggleViewDraft();
              }}
            />
            <button
              type="submit"
              class="explorer-icon"
              disabled={savedViewBusy || !taskQuery.savedViewNameDraft.trim()}
              aria-label={t("projects.savedViews.save")}
              data-app-tooltip={t("projects.savedViews.save")}
            >
              <Plus />
            </button>
          </form>
        {/if}

        {#if taskQuery.savedViewError}
          <Notice class="mb-1 px-2 py-1 text-xs" message={taskQuery.savedViewError} />
        {/if}

        {#each savedViews as view (view.id)}
          <div class="explorer-row projects-sidebar-view">
            <button
              type="button"
              class="explorer-row-label projects-sidebar-view-label"
              title={view.name}
              disabled={savedViewBusy}
              onclick={() => applyView(view)}
            >
              <Bookmark class="explorer-row-icon text-muted-foreground" aria-hidden="true" />
              <span class="min-w-0 flex-1 truncate">{view.name}</span>
            </button>
            <button
              type="button"
              class="explorer-icon explorer-row-action"
              disabled={taskQuery.savedViewSaving}
              aria-label={t("projects.savedViews.delete", view.name)}
              data-app-tooltip={t("projects.savedViews.delete", view.name)}
              onclick={() => { void taskQuery.deleteSavedView(view); }}
            >
              <Trash2 />
            </button>
          </div>
        {:else}
          {#if savedViewsLoaded && !creatingView}
            <p class="projects-sidebar-hint">{t("projects.sidebar.savedViewsEmpty")}</p>
          {/if}
        {/each}
      </section>
    {/if}
  </div>
</aside>

<style>
  .projects-sidebar {
    width: var(--explorer-width);
    transition: width 180ms cubic-bezier(0.2, 0, 0, 1);
  }

  .projects-sidebar-content {
    width: var(--explorer-width);
    min-width: var(--explorer-width);
  }

  .projects-sidebar-collapsed {
    width: 2.75rem;
  }

  .projects-sidebar-rail {
    background-color: var(--explorer-background);
    opacity: 0;
    pointer-events: none;
    transition: opacity 60ms linear;
  }

  .projects-sidebar-collapsed .projects-sidebar-rail {
    opacity: 1;
    pointer-events: auto;
    transition-delay: 100ms;
  }

  .projects-sidebar-heading {
    display: flex;
    align-items: center;
    gap: 0.2rem;
    padding-inline: 0.5rem 0;
    color: var(--muted-foreground);
  }

  .projects-sidebar-view {
    display: flex;
    min-width: 0;
    align-items: center;
  }

  .projects-sidebar-view-label {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: 0.5rem;
    padding: var(--explorer-row-padding) 0.5rem;
    color: var(--foreground);
    text-align: start;
  }

  .projects-sidebar-hint {
    padding: 0.25rem 0.5rem 0.375rem;
    color: var(--muted-foreground);
    font-size: var(--panel-detail-font-size);
    line-height: 1.35;
  }

  @media (prefers-reduced-motion: reduce) {
    .projects-sidebar,
    .projects-sidebar-rail {
      transition: none;
    }
  }
</style>
