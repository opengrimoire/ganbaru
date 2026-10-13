<script lang="ts">
  import { tick } from "svelte";
  import ChevronsLeft from "@lucide/svelte/icons/chevrons-left";
  import ChevronsRight from "@lucide/svelte/icons/chevrons-right";
  import SearchField from "$lib/components/ui/SearchField.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { projectSidebarActions, resolveProjectSidebarGroupId, searchProjectSidebar } from "$lib/projects/sidebar";
  import type { ProjectPlace } from "$lib/projects/places";
  import type { Project } from "$lib/projects/types";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { suppressHoverOnToggle } from "$lib/utils/hover-suppression";
  import ProjectSidebarRow from "./ProjectSidebarRow.svelte";
  import ProjectPlacesNav from "./ProjectPlacesNav.svelte";

  /**
   * The docked Projects explorer: a project search, the projects of the selected project's group, and the places of the
   * selected project pinned to the bottom, so they keep their position while the project list scrolls above them. A search lists matching projects from every visible group; picking a project selects it.
   * Collapsing the sidebar keeps a narrow rail with the places alone. Hosted Notes and Chat render it in place of their
   * own explorer.
   */
  let {
    places,
    activePlace,
    onSelectPlace,
    onProjectSelected,
    onOpenProjectSettings = undefined,
  }: {
    /** Places to list, in sidebar order. */
    places: readonly ProjectPlace[];
    activePlace: ProjectPlace;
    onSelectPlace: (place: ProjectPlace) => void;
    /** Runs after a project row selects its project. */
    onProjectSelected: () => void;
    /** Selects a project and opens its settings; hosted places without a settings panel leave it out. */
    onOpenProjectSettings?: (project: Project) => void;
  } = $props();

  const projects = getProjects();
  const { t } = getLocalization();

  const collapsed = $derived(projects.sidebarCollapsed);
  const observeCollapsedForHover = suppressHoverOnToggle();
  $effect(() => observeCollapsedForHover(collapsed));

  let query = $state("");

  const groups = $derived(projects.visibleGroups());
  const groupIds = $derived(groups.map((group) => group.id));
  const listedGroupId = $derived(resolveProjectSidebarGroupId(
    projects.selectedGroup?.id ?? null,
    groupIds,
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

  /** Selects a listed project and ends any search, so the list returns to the selected project's group. */
  async function selectProject(project: Project): Promise<void> {
    if (project.id === projects.selectedProjectId) {
      query = "";
      return;
    }
    try {
      await projects.selectProject(project.id);
      query = "";
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
      <nav class="explorer-list flex min-h-0 flex-1 flex-col pb-2" aria-label={t("projects.sidebar.projects")} data-project-list>
        {#each listedProjects as project (project.id)}
          {@const actions = projectSidebarActions(project, groupIds)}
          <ProjectSidebarRow
            {project}
            selected={project.id === projects.selectedProjectId}
            groupName={resultGroupNames?.get(project.id) ?? null}
            {actions}
            moveGroups={actions.moveGroupIds.flatMap((groupId) => groups.find((group) => group.id === groupId) ?? [])}
            onSelect={() => void selectProject(project)}
            onRename={(name) => projects.renameProject(project, name)}
            onMoveToGroup={(groupId) => projects.moveProjectToGroup(project, groupId)}
            onSetStatus={(status) => projects.setProjectStatus(project, status)}
            onOpenSettings={onOpenProjectSettings ? () => onOpenProjectSettings?.(project) : undefined}
          />
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
