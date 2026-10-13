<script lang="ts">
  import { tick } from "svelte";
  import ChevronsLeft from "@lucide/svelte/icons/chevrons-left";
  import ChevronsRight from "@lucide/svelte/icons/chevrons-right";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectPlace } from "$lib/projects/places";
  import { getProjects } from "$lib/stores/projects.svelte";
  import ProjectPlacesNav from "./ProjectPlacesNav.svelte";

  /**
   * The docked Projects explorer with the places of the selected project. Collapsing it keeps a narrow rail with the
   * same places; the toolbar matches the hosted Notes and Chat explorers so the collapse control stays in one spot.
   */
  let {
    places,
    activePlace,
    onSelectPlace,
  }: {
    places: readonly ProjectPlace[];
    activePlace: ProjectPlace;
    onSelectPlace: (place: ProjectPlace) => void;
  } = $props();

  const projects = getProjects();
  const { t } = getLocalization();

  const collapsed = $derived(projects.sidebarCollapsed);

  let expandButton = $state<HTMLButtonElement | null>(null);
  let collapseButton = $state<HTMLButtonElement | null>(null);

  function setCollapsed(value: boolean): void {
    projects.sidebarCollapsed = value;
    void tick().then(() => (value ? expandButton : collapseButton)?.focus());
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
      <ProjectPlacesNav variant="rail" {places} {activePlace} {onSelectPlace} />
    {/if}
  </div>

  <div class="projects-sidebar-content flex h-full min-h-0 flex-col overflow-y-auto" inert={collapsed}>
    <div class="explorer-toolbar">
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
