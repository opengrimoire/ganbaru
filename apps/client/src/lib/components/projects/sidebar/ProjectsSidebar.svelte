<script lang="ts">
  import { tick } from "svelte";
  import ChevronsLeft from "@lucide/svelte/icons/chevrons-left";
  import ChevronsRight from "@lucide/svelte/icons/chevrons-right";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { COMPACT_IDENTITY_EMOJI_SCALE, COMPACT_IDENTITY_ICON_SIZE, COMPACT_IDENTITY_ICON_STROKE_WIDTH } from "$lib/icon-sizing";
  import { resolveProjectSidebarGroupId } from "$lib/projects/sidebar";
  import type { ProjectPlace } from "$lib/projects/places";
  import type { Project } from "$lib/projects/types";
  import { getProjects } from "$lib/stores/projects.svelte";
  import ProjectIcon from "../ProjectIcon.svelte";
  import ProjectPlacesNav from "./ProjectPlacesNav.svelte";

  /**
   * The docked Projects explorer: a group selector, the projects of that group, and the places of the selected project.
   * Browsing another group only changes the listed projects; picking a project selects it. Collapsing the sidebar keeps
   * a narrow rail with the places alone. Hosted Notes and Chat render it in place of their own explorer.
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

  /** A browsed group lasts until the selected project changes, so any project switch lists the new project's group. */
  let browsed = $state<{ groupId: string; fromProjectId: string | null } | null>(null);

  const groups = $derived(projects.visibleGroups());
  const groupOptions = $derived(groups.map((group) => ({ value: group.id, label: group.name })));
  const listedGroupId = $derived(resolveProjectSidebarGroupId(
    browsed && browsed.fromProjectId === projects.selectedProjectId ? browsed.groupId : null,
    projects.selectedGroup?.id ?? null,
    groups.map((group) => group.id),
  ));
  const listedProjects = $derived.by((): Project[] => {
    if (!listedGroupId) return [];
    return projects.showInactiveProjects
      ? projects.projectsForGroupIncludingInactive(listedGroupId)
      : projects.projectsForGroup(listedGroupId);
  });

  let expandButton = $state<HTMLButtonElement | null>(null);
  let collapseButton = $state<HTMLButtonElement | null>(null);

  function setCollapsed(value: boolean): void {
    projects.sidebarCollapsed = value;
    void tick().then(() => (value ? expandButton : collapseButton)?.focus());
  }

  function browseGroup(groupId: string): void {
    browsed = { groupId, fromProjectId: projects.selectedProjectId };
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

{#snippet groupIcon(groupId: string)}
  <ProjectIcon
    name={projects.groupById(groupId)?.icon}
    size={COMPACT_IDENTITY_ICON_SIZE}
    strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH}
    emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
  />
{/snippet}

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
      <ProjectPlacesNav variant="rail" {places} {activePlace} {onSelectPlace} />
    {/if}
  </div>

  <div class="projects-sidebar-content flex h-full min-h-0 flex-col" inert={collapsed}>
    <div class="explorer-toolbar gap-1">
      {#if listedGroupId}
        <Select
          value={listedGroupId}
          options={groupOptions}
          onChange={browseGroup}
          ariaLabel={t("projects.sidebar.group")}
          appearance="quiet"
          contentAlign="start"
          leading={groupIcon}
          class="w-auto flex-1"
        />
      {/if}
      <button
        bind:this={collapseButton}
        type="button"
        class="explorer-icon ml-auto"
        aria-label={t("projects.sidebar.collapse")}
        aria-expanded="true"
        data-app-tooltip={t("projects.sidebar.collapse")}
        onclick={() => setCollapsed(true)}
      >
        <ChevronsLeft />
      </button>
    </div>

    {#if !collapsed}
      <nav class="flex min-h-0 flex-col overflow-y-auto px-2 pb-2" aria-label={t("projects.sidebar.projects")} data-project-list>
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
          </button>
        {:else}
          <p class="explorer-row-label px-2 py-1 text-muted-foreground">{t("projects.navigator.empty")}</p>
        {/each}
      </nav>
      <ProjectPlacesNav variant="list" {places} {activePlace} {onSelectPlace} />
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

  @media (prefers-reduced-motion: reduce) {
    .projects-sidebar,
    .projects-sidebar-rail {
      transition: none;
    }
  }
</style>
