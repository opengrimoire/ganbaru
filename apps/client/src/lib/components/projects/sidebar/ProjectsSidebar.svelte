<script lang="ts">
  import { tick } from "svelte";
  import ChevronsLeft from "@lucide/svelte/icons/chevrons-left";
  import ChevronsRight from "@lucide/svelte/icons/chevrons-right";
  import MobileSidebarPanel from "$lib/components/mobile/MobileSidebarPanel.svelte";
  import SearchField from "$lib/components/ui/SearchField.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { projectSidebarActions, resolveProjectSidebarGroupId, searchProjectSidebar } from "$lib/projects/sidebar";
  import type { Project, ProjectView } from "$lib/projects/types";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { getWorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { createSidebarOrderDrag } from "$lib/utils/sidebar-order-drag.svelte";
  import { suppressHoverOnToggle } from "$lib/utils/hover-suppression";
  import { scrollOverflowAction } from "$lib/utils/scroll-overflow";
  import type { OpenInNewTabIntent } from "$lib/workspace-tabs/gestures";
  import ProjectSidebarSection from "./ProjectSidebarSection.svelte";
  import ProjectPlacesNav from "./ProjectPlacesNav.svelte";

  /**
   * The docked Projects explorer, laid out like the Chat channel rail: a project search, then one collapsible section
   * per project of the selected project's group, each listing that project's views the way Chat sections list
   * channels. A search lists matching projects from every visible group. Opening a view of another project selects that
   * project. Collapsing the sidebar keeps a narrow rail with the selected project's views. Hosted Notes and Chat render
   * it in place of their own explorer. The phone shell opens it as a floating panel that closes instead of collapsing.
   */
  let {
    onSelectView,
    onProjectSelected,
    onOpenProjectSettings = undefined,
    presentation = "dock",
    onClose = undefined,
  }: {
    /** Opens a view of the selected project. */
    onSelectView: (view: ProjectView) => void;
    /** Runs after opening a view of another project selects that project. */
    onProjectSelected: () => void;
    /** Selects a project and opens its settings; hosted views without a settings panel leave it out. */
    onOpenProjectSettings?: (project: Project) => void;
    /** A dock collapses to a rail; a panel floats over the phone destination and closes through `onClose`. */
    presentation?: "dock" | "panel";
    /** Closes the panel presentation. */
    onClose?: () => void;
  } = $props();

  const panel = $derived(presentation === "panel");

  const projects = useProjects();
  const preferences = getPreferences();
  const tabs = getWorkspaceTabs();
  const { t } = getLocalization();

  const collapsed = $derived(!panel && projects.sidebarCollapsed);
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
  const selectedProjectViews = $derived(projects.viewsForProject(projects.selectedProjectId));

  /** Sections drag from their heading in the custom order, except in search results, which span groups. */
  const sectionDrag = createSidebarOrderDrag({
    getIds: () => listedProjects.map((project) => project.id),
    enabled: () => preferences.projectsSidebarOrder === "custom" && !searching && listedGroupId !== null,
    isStarred: (projectId) => projects.isProjectStarred(projectId),
    save: async (orderedIds) => {
      if (listedGroupId) await projects.reorderProjectsInGroup(listedGroupId, orderedIds);
    },
    description: "projects from the Projects sidebar",
  });

  $effect(() => sectionDrag.reset);

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

  /**
   * Opens a view and ends any search, so the list returns to the selected project's group. A view of another project
   * is remembered first, so selecting that project lands on it directly.
   */
  async function openView(view: ProjectView): Promise<void> {
    projects.setSidebarProjectCollapsed(view.projectId, false);
    if (view.projectId !== projects.selectedProjectId) {
      projects.selectView(view);
      try {
        await projects.selectProject(view.projectId);
      } catch (error: unknown) {
        console.error("Failed to select a project from the sidebar", error);
        return;
      }
      onProjectSelected();
    }
    query = "";
    onSelectView(view);
  }

  function openViewInNewTab(view: ProjectView, intent: OpenInNewTabIntent): void {
    tabs.openLocation("item", {
      kind: "projects",
      projectId: view.projectId,
      item: { type: "project-view", id: view.id },
    }, intent);
  }
</script>

{#snippet projectList(listClass: string)}
  <nav use:scrollOverflowAction class={listClass} aria-label={t("projects.sidebar.projects")} data-project-list>
    {#each listedProjects as project (project.id)}
      {@const actions = projectSidebarActions(project, groupIds)}
      <div {@attach sectionDrag.item(project.id)}>
        <ProjectSidebarSection
          {project}
          selected={project.id === projects.selectedProjectId}
          groupName={resultGroupNames?.get(project.id) ?? null}
          {actions}
          moveGroups={actions.moveGroupIds.flatMap((groupId) => groups.find((group) => group.id === groupId) ?? [])}
          views={projects.viewsForProject(project.id)}
          activeViewId={projects.activeViewId}
          collapsed={projects.isSidebarProjectCollapsed(project.id)}
          onCollapsedChange={(value) => projects.setSidebarProjectCollapsed(project.id, value)}
          onSelectView={(view) => void openView(view)}
          onOpenViewInNewTab={tabs.canOpenTabs && !panel ? openViewInNewTab : undefined}
          onRename={(name) => projects.renameProject(project, name)}
          onMoveToGroup={(groupId) => projects.moveProjectToGroup(project, groupId)}
          onSetStatus={(status) => projects.setProjectStatus(project, status)}
          onOpenSettings={onOpenProjectSettings ? () => onOpenProjectSettings?.(project) : undefined}
          onHeadingPointerDown={(event) => sectionDrag.pointerDown(event, project.id)}
          onMove={sectionDrag.enabled ? (step) => sectionDrag.move(project.id, step) : undefined}
          canMoveUp={sectionDrag.canMove(project.id, -1)}
          canMoveDown={sectionDrag.canMove(project.id, 1)}
        />
      </div>
    {:else}
      <p class="explorer-row-label px-2 py-1 text-muted-foreground">
        {searching ? t("projects.sidebar.noResults") : t("projects.navigator.empty")}
      </p>
    {/each}
  </nav>
{/snippet}

{#if panel}
  <MobileSidebarPanel
    title={t("projects.sidebar.projects")}
    searchLabel={t("projects.sidebar.search")}
    clearSearchLabel={t("projects.sidebar.clearSearch")}
    bind:query
    onClose={() => onClose?.()}
  >
    <div class="explorer-sidebar explorer-touch explorer-panel flex min-h-0 flex-1 flex-col" data-projects-sidebar>
      {@render projectList("hide-scrollbar flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto overscroll-contain px-2 py-2")}
    </div>
  </MobileSidebarPanel>
{:else}
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
        <ProjectPlacesNav views={selectedProjectViews} activeViewId={projects.activeViewId} {onSelectView} />
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
        {@render projectList("explorer-list flex min-h-0 flex-1 flex-col gap-2 pb-2")}
      {/if}
    </div>
  </aside>
{/if}
