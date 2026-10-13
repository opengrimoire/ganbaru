<script lang="ts">
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import ChartGantt from "@lucide/svelte/icons/chart-gantt";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import LayoutDashboard from "@lucide/svelte/icons/layout-dashboard";
  import List from "@lucide/svelte/icons/list";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { PROJECT_VIEW_IDS, type ProjectViewId } from "$lib/projects/types";

  /** The views of the selected project, as sidebar rows or as rail icons. */
  let {
    variant,
    activeView,
    onSelectView,
  }: {
    /** `list` inside an expanded sidebar, `rail` inside a collapsed one. */
    variant: "list" | "rail";
    activeView: ProjectViewId;
    onSelectView: (view: ProjectViewId) => void;
  } = $props();

  const { t } = getLocalization();

  function viewLabel(view: ProjectViewId): string {
    switch (view) {
      case "dashboard": return t("projects.sidebar.overview");
      case "list": return t("projects.tabs.list");
      case "kanban": return t("projects.tabs.kanban");
      case "calendar": return t("projects.tabs.calendar");
      case "gantt": return t("projects.tabs.gantt");
    }
  }

  function viewIcon(view: ProjectViewId) {
    switch (view) {
      case "dashboard": return LayoutDashboard;
      case "list": return List;
      case "kanban": return Columns3;
      case "calendar": return CalendarDays;
      case "gantt": return ChartGantt;
    }
  }
</script>

<nav class="project-views" class:project-views-rail={variant === "rail"} aria-label={t("projects.toolbar.views")} data-project-views>
  {#each PROJECT_VIEW_IDS as view (view)}
    {@const Icon = viewIcon(view)}
    {@const label = viewLabel(view)}
    {@const active = activeView === view}
    <button
      type="button"
      class={variant === "rail"
        ? "explorer-icon"
        : "explorer-row explorer-row-label flex w-full min-w-0 items-center gap-1.5 px-2 text-left text-inherit"}
      class:explorer-icon-active={variant === "rail" && active}
      class:explorer-selected={variant === "list" && active}
      aria-label={variant === "rail" ? label : undefined}
      aria-current={active ? "page" : undefined}
      data-app-tooltip={variant === "rail" ? label : undefined}
      data-project-view={view}
      onclick={() => onSelectView(view)}
    >
      <Icon class="explorer-row-icon" aria-hidden="true" />
      {#if variant === "list"}<span class="min-w-0 flex-1 truncate">{label}</span>{/if}
    </button>
  {/each}
</nav>

<style>
  .project-views {
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    padding: 0 0.5rem 0.5rem;
  }

  .project-views-rail {
    align-items: center;
    gap: 0.125rem;
    padding: 0.25rem 0;
  }
</style>
