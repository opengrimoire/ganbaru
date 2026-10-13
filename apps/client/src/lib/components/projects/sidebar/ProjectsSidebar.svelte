<script lang="ts">
  import { tick } from "svelte";
  import ChevronsLeft from "@lucide/svelte/icons/chevrons-left";
  import ChevronsRight from "@lucide/svelte/icons/chevrons-right";
  import SearchField from "$lib/components/ui/SearchField.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { COMPACT_IDENTITY_EMOJI_SCALE, COMPACT_IDENTITY_ICON_SIZE, COMPACT_IDENTITY_ICON_STROKE_WIDTH } from "$lib/icon-sizing";
  import { resolveProjectSidebarGroupId, searchProjectSidebar } from "$lib/projects/sidebar";
  import type { ProjectPlace } from "$lib/projects/places";
  import type { Project } from "$lib/projects/types";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { suppressHoverOnToggle } from "$lib/utils/hover-suppression";
  import ProjectIcon from "../ProjectIcon.svelte";
  import ProjectPlacesNav from "./ProjectPlacesNav.svelte";

  /**
   * The docked Projects explorer: a project search, the projects of the selected project's group, and the places of the
   * selected project. A search lists matching projects from every visible group; picking a project selects it.
   * Collapsing the sidebar keeps a narrow rail with the places alone. Hosted Notes and Chat render it in place of their
   * own explorer.
   */
  let {
    places,
    activePlace,
    onSelectPlace,
    onProjectSelected,
  }: {
    /** Places to list, in sidebar order. */
    places: readonly ProjectPlace[];
    activePlace: ProjectPlace;
    onSelectPlace: (place: ProjectPlace) => void;
    /** Runs after a project row selects its project. */
    onProjectSelected: () => void;
  } = $props();

  const projects = getProjects();
  const { t } = getLocalization();

  const collapsed = $derived(projects.sidebarCollapsed);
  const observeCollapsedForHover = suppressHoverOnToggle();
  $effect(() => observeCollapsedForHover(collapsed));

  let query = $state("");

  const groups = $derived(projects.visibleGroups());
  const listedGroupId = $derived(resolveProjectSidebarGroupId(
    projects.selectedGroup?.id ?? null,
    groups.map((group) => group.id),
  ));
  const searching = $derived(query.trim().length > 0);
  const searchResults = $derived(searching
    ? searchProjectSidebar(query, groups.map((group) => ({ name: group.name, projects: groupProjects(group.id) })))
    : []);
  const listedProjects = $derived(searching
    ? searchResults.map((result) => result.project)
    : listedGroupId ? groupProjects(listedGroupId) : []);
  /** Search results name their group only when more than one group could have matched. */
  const resultGroupNames = $derived(searching && groups.length > 1
    ? new Map(searchResults.map((result) => [result.project.id, result.groupName]))
    : null);

  function groupProjects(groupId: string): Project[] {
    return projects.showInactiveProjects
      ? projects.projectsForGroupIncludingInactive(groupId)
      : projects.projectsForGroup(groupId);
  }

  let expandButton = $state<HTMLButtonElement | null>(null);
  let collapseButton = $state<HTMLButtonElement | null>(null);

  function setCollapsed(value: boolean): void {
    projects.sidebarCollapsed = value;
    void tick().then(() => (value ? expandButton : collapseButton)?.focus());
  }

  async function selectProject(project: Project): Promise<void> {
    if (project.id === projects.selectedProjectId) return;
    try {
      await projects.selectProject(project.id);
      onProjectSelected();
    } catch (error: unknown) {
      console.error("Failed to select a project from the sidebar", error);
    }
  }
</script>

<aside
  class="explorer-sidebar explorer-dock h-full min-h-0"
  class:explorer-dock-collapsed={collapsed}
  aria-label={t("projects.sidebar.label")}
  data-projects-sidebar
>
  <div class="explorer-dock-rail flex flex-col" aria-hidden={!collapsed} inert={!collapsed}>
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
      <ProjectPlacesNav variant="rail" {places} {activePlace} {onSelectPlace} />
    {/if}
  </div>

  <div class="explorer-dock-content flex h-full min-h-0 flex-col" inert={collapsed}>
    <div class="explorer-toolbar">
      <SearchField
        bind:value={query}
        label={t("projects.sidebar.search")}
        clearLabel={t("projects.sidebar.clearSearch")}
        inline
        focusOnMount={false}
      />
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
      <nav class="explorer-list flex min-h-0 flex-col pb-2" aria-label={t("projects.sidebar.projects")} data-project-list>
        {#each listedProjects as project (project.id)}
          {@const selected = project.id === projects.selectedProjectId}
          <button
            type="button"
            class="explorer-row explorer-row-label flex w-full min-w-0 items-center gap-1.5 px-2 text-left text-inherit"
            class:explorer-selected={selected}
            class:text-muted-foreground={project.status !== "active"}
            aria-current={selected ? "true" : undefined}
            data-project-id={project.id}
            onclick={() => void selectProject(project)}
          >
            <ProjectIcon
              name={project.icon}
              size={COMPACT_IDENTITY_ICON_SIZE}
              strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH}
              emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
              class="explorer-row-icon"
            />
            <span class="min-w-0 flex-1 truncate">{project.name}</span>
            {#if resultGroupNames?.has(project.id)}
              <span class="max-w-1/2 shrink-0 truncate text-muted-foreground">{resultGroupNames.get(project.id)}</span>
            {/if}
          </button>
        {:else}
          <p class="explorer-row-label px-2 py-1 text-muted-foreground">
            {searching ? t("projects.sidebar.noResults") : t("projects.navigator.empty")}
          </p>
        {/each}
      </nav>
      <ProjectPlacesNav variant="list" {places} {activePlace} {onSelectPlace} />
    {/if}
  </div>
</aside>
