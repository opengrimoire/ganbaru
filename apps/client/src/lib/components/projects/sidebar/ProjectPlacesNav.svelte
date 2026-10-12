<script lang="ts">
  import ChevronsUpDown from "@lucide/svelte/icons/chevrons-up-down";
  import LayoutDashboard from "@lucide/svelte/icons/layout-dashboard";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    COMPACT_IDENTITY_EMOJI_SCALE,
    COMPACT_IDENTITY_ICON_SIZE,
    COMPACT_IDENTITY_ICON_STROKE_WIDTH,
  } from "$lib/icon-sizing";
  import { PROJECT_PLACES, type ProjectPlace } from "$lib/projects/places";
  import { projectNavigatorPanelGeometry } from "$lib/projects/toolbar";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import ProjectIcon from "../ProjectIcon.svelte";
  import ProjectNavigator from "../ProjectNavigator.svelte";

  /**
   * The project switcher and the places of the selected project. Docked explorers render it below their toolbar while
   * they belong to the Projects tab, so Projects keeps one sidebar whatever place is open.
   */
  let {
    variant,
    activePlace,
    onSelectPlace,
    onProjectSelected = undefined,
  }: {
    /** `list` inside an expanded sidebar, `rail` inside a collapsed one. */
    variant: "list" | "rail";
    activePlace: ProjectPlace;
    onSelectPlace: (place: ProjectPlace) => void;
    onProjectSelected?: () => void;
  } = $props();

  const projects = getProjects();
  const viewport = getViewport();
  const { t } = getLocalization();

  const selectedProject = $derived(projects.selectedProject);
  const selectedGroup = $derived(projects.selectedGroup);

  let navigatorOpen = $state(false);
  let switcherElement = $state<HTMLButtonElement | null>(null);
  let navigatorPanelElement = $state<HTMLDivElement | null>(null);
  let navigatorPanelStyle = $state("");
  let navigatorPanelMaxHeight = $state(0);

  function placeLabel(place: ProjectPlace): string {
    return place === "overview" ? t("projects.sidebar.overview") : t("projects.sidebar.tasks");
  }

  function placeIcon(place: ProjectPlace) {
    return place === "overview" ? LayoutDashboard : ListChecks;
  }

  function refreshNavigatorGeometry(): void {
    if (!navigatorOpen || !switcherElement) return;
    const rect = switcherElement.getBoundingClientRect();
    const geometry = projectNavigatorPanelGeometry({
      anchorLeft: variant === "rail" ? rect.right : rect.left,
      anchorBottom: variant === "rail" ? rect.top : rect.bottom,
      viewportWidth: viewport.width,
      viewportHeight: viewport.height,
    });
    navigatorPanelStyle = [
      `left: ${Math.round(geometry.left)}px`,
      `top: ${Math.round(geometry.top)}px`,
      `width: ${Math.round(geometry.width)}px`,
    ].join("; ");
    navigatorPanelMaxHeight = geometry.height;
  }

  function toggleNavigator(): void {
    navigatorOpen = !navigatorOpen;
    refreshNavigatorGeometry();
  }

  function closeNavigator(): void {
    navigatorOpen = false;
  }

  function handleWindowPointerDown(event: PointerEvent): void {
    if (!navigatorOpen) return;
    const target = event.target;
    if (!(target instanceof Node)) return;
    if (switcherElement?.contains(target) || navigatorPanelElement?.contains(target)) return;
    navigatorOpen = false;
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (!navigatorOpen || event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    navigatorOpen = false;
    switcherElement?.focus();
  }

  $effect(() => {
    if (!navigatorOpen) return;
    void viewport.width;
    void viewport.height;
    requestAnimationFrame(refreshNavigatorGeometry);
  });
</script>

<svelte:window onpointerdown={handleWindowPointerDown} onkeydown={handleWindowKeydown} />

{#if selectedProject && selectedGroup}
  <nav class="project-places" class:project-places-rail={variant === "rail"} aria-label={t("projects.sidebar.places")} data-project-places>
    <button
      bind:this={switcherElement}
      type="button"
      class={variant === "rail" ? "explorer-icon" : "explorer-row explorer-row-label project-places-row"}
      class:explorer-icon-active={variant === "rail" && navigatorOpen}
      class:explorer-selected={variant === "list" && navigatorOpen}
      aria-label={`${t("projects.sidebar.switchProject")}: ${selectedGroup.name} / ${selectedProject.name}`}
      aria-haspopup="dialog"
      aria-expanded={navigatorOpen}
      data-app-tooltip={variant === "rail" ? selectedProject.name : undefined}
      data-project-switcher
      onclick={toggleNavigator}
    >
      <ProjectIcon
        name={selectedProject.icon}
        size={COMPACT_IDENTITY_ICON_SIZE}
        strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH}
        emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
        class="shrink-0"
      />
      {#if variant === "list"}
        <span class="min-w-0 flex-1 truncate font-medium">{selectedProject.name}</span>
        <ChevronsUpDown class="explorer-row-icon text-muted-foreground" aria-hidden="true" />
      {/if}
    </button>
    {#each PROJECT_PLACES as place (place)}
      {@const Icon = placeIcon(place)}
      {@const label = placeLabel(place)}
      {@const active = activePlace === place}
      <button
        type="button"
        class={variant === "rail" ? "explorer-icon" : "explorer-row explorer-row-label project-places-row"}
        class:explorer-icon-active={variant === "rail" && active}
        class:explorer-selected={variant === "list" && active}
        aria-label={variant === "rail" ? label : undefined}
        aria-current={active ? "page" : undefined}
        data-app-tooltip={variant === "rail" ? label : undefined}
        data-project-place={place}
        onclick={() => onSelectPlace(place)}
      >
        <Icon class="explorer-row-icon" aria-hidden="true" />
        {#if variant === "list"}<span class="min-w-0 flex-1 truncate">{label}</span>{/if}
      </button>
    {/each}
  </nav>

  {#if navigatorOpen}
    <div
      bind:this={navigatorPanelElement}
      class="fixed z-80"
      style={navigatorPanelStyle}
      role="dialog"
      tabindex="-1"
      aria-label={t("projects.navigator.pickerLabel")}
    >
      <ProjectNavigator
        selectedProjectId={selectedProject.id}
        selectedGroupId={selectedGroup.id}
        showInactiveProjects={projects.showInactiveProjects}
        panelMode="groups"
        panelMaxHeight={navigatorPanelMaxHeight}
        onShowInactiveProjectsChange={(value) => { projects.showInactiveProjects = value; }}
        onProjectSelected={() => {
          closeNavigator();
          onProjectSelected?.();
        }}
      />
    </div>
  {/if}
{/if}

<style>
  .project-places {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 0 0.5rem 0.5rem;
  }

  .project-places-rail {
    align-items: center;
    gap: 0.125rem;
    padding: 0.25rem 0;
  }

  .project-places-row {
    display: flex;
    width: 100%;
    min-width: 0;
    align-items: center;
    gap: 0.5rem;
    padding-inline: 0.5rem;
    color: var(--foreground);
    text-align: start;
  }
</style>
