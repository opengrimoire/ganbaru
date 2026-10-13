<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { shortcutModifierLabel } from "$lib/keyboard-shortcuts";
  import type { ProjectView } from "$lib/projects/types";
  import { PROJECT_VIEW_TYPE_ICONS, projectViewLabel } from "./project-view-presentation";

  /**
   * The views of the selected project in their saved order, including the hosted Notes and Chat views. Docked
   * explorers render it below their toolbar while they belong to the Projects tab, so Projects keeps one sidebar
   * whatever view is open.
   */
  let {
    variant,
    views,
    activeViewId,
    onSelectView,
  }: {
    /** `list` inside an expanded sidebar, `rail` inside a collapsed one. */
    variant: "list" | "rail";
    /** Views to list, in sidebar order. */
    views: readonly ProjectView[];
    activeViewId: string | null;
    onSelectView: (view: ProjectView) => void;
  } = $props();

  const { t } = getLocalization();
  const modifierLabel = shortcutModifierLabel();

  /** Names the view with the Ctrl and digit shortcut that opens it, written like the title bar tab shortcuts. */
  function viewTooltip(label: string, index: number): string {
    return index < 9 ? t("projects.sidebar.placeWithShortcut", label, `${modifierLabel}+${index + 1}`) : label;
  }
</script>

<nav class="project-places" class:project-places-rail={variant === "rail"} aria-label={t("projects.sidebar.places")} data-project-places>
  {#each views as view, index (view.id)}
    {@const Icon = PROJECT_VIEW_TYPE_ICONS[view.viewType]}
    {@const label = projectViewLabel(t, view)}
    {@const active = activeViewId === view.id}
    <button
      type="button"
      class={variant === "rail"
        ? "explorer-icon"
        : "explorer-row explorer-row-label flex w-full min-w-0 items-center gap-1.5 px-2 text-left text-inherit"}
      class:explorer-icon-active={variant === "rail" && active}
      class:explorer-selected={variant === "list" && active}
      aria-label={variant === "rail" ? label : undefined}
      aria-current={active ? "page" : undefined}
      data-app-tooltip={viewTooltip(label, index)}
      data-project-place={view.viewType}
      onclick={() => onSelectView(view)}
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
