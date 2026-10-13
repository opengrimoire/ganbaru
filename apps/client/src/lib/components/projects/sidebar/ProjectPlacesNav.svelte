<script lang="ts">
  import Book from "@lucide/svelte/icons/book";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import ChartGantt from "@lucide/svelte/icons/chart-gantt";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import LayoutDashboard from "@lucide/svelte/icons/layout-dashboard";
  import List from "@lucide/svelte/icons/list";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { shortcutModifierLabel } from "$lib/keyboard-shortcuts";
  import { PROJECT_PLACES, type ProjectPlace } from "$lib/projects/places";

  /**
   * The places of the selected project: its views, then the hosted Notes and Chat workspaces. Docked explorers render
   * it below their toolbar while they belong to the Projects tab, so Projects keeps one sidebar whatever place is open.
   */
  let {
    variant,
    places = PROJECT_PLACES,
    activePlace,
    onSelectPlace,
  }: {
    /** `list` inside an expanded sidebar, `rail` inside a collapsed one. */
    variant: "list" | "rail";
    /** Places to list, in sidebar order. */
    places?: readonly ProjectPlace[];
    activePlace: ProjectPlace;
    onSelectPlace: (place: ProjectPlace) => void;
  } = $props();

  const { t } = getLocalization();
  const modifierLabel = shortcutModifierLabel();

  /** Names the place with the Ctrl and digit shortcut that opens it, written like the title bar tab shortcuts. */
  function placeTooltip(label: string, index: number): string {
    return index < 9 ? t("projects.sidebar.placeWithShortcut", label, `${modifierLabel}+${index + 1}`) : label;
  }

  function placeLabel(place: ProjectPlace): string {
    switch (place) {
      case "dashboard": return t("projects.sidebar.overview");
      case "list": return t("projects.tabs.list");
      case "kanban": return t("projects.tabs.kanban");
      case "calendar": return t("projects.tabs.calendar");
      case "gantt": return t("projects.tabs.gantt");
      case "notes": return t("projects.sidebar.notes");
      case "chat": return t("projects.sidebar.chat");
    }
  }

  function placeIcon(place: ProjectPlace) {
    switch (place) {
      case "dashboard": return LayoutDashboard;
      case "list": return List;
      case "kanban": return Columns3;
      case "calendar": return CalendarDays;
      case "gantt": return ChartGantt;
      case "notes": return Book;
      case "chat": return MessageSquare;
    }
  }
</script>

<nav class="project-places" class:project-places-rail={variant === "rail"} aria-label={t("projects.sidebar.places")} data-project-places>
  {#each places as place, index (place)}
    {@const Icon = placeIcon(place)}
    {@const label = placeLabel(place)}
    {@const active = activePlace === place}
    <button
      type="button"
      class={variant === "rail"
        ? "explorer-icon"
        : "explorer-row explorer-row-label flex w-full min-w-0 items-center gap-1.5 px-2 text-left text-inherit"}
      class:explorer-icon-active={variant === "rail" && active}
      class:explorer-selected={variant === "list" && active}
      aria-label={variant === "rail" ? label : undefined}
      aria-current={active ? "page" : undefined}
      data-app-tooltip={placeTooltip(label, index)}
      data-project-place={place}
      onclick={() => onSelectPlace(place)}
    >
      <Icon class="explorer-row-icon" aria-hidden="true" />
      {#if variant === "list"}<span class="min-w-0 flex-1 truncate">{label}</span>{/if}
    </button>
  {/each}
</nav>

<style>
  .project-places {
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    padding: 0 0.5rem 0.5rem;
  }

  .project-places-rail {
    align-items: center;
    gap: 0.125rem;
    padding: 0.25rem 0;
  }
</style>
