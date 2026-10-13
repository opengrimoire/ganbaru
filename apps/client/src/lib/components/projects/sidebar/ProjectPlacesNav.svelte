<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { shortcutModifierLabel } from "$lib/keyboard-shortcuts";
  import type { ProjectView } from "$lib/projects/types";
  import { PROJECT_VIEW_TYPE_ICONS, projectViewLabel } from "./project-view-presentation";

  /**
   * The views of the selected project as icons in the collapsed sidebar rail, in their saved order, including the
   * hosted Notes and Chat views.
   */
  let {
    views,
    activeViewId,
    onSelectView,
  }: {
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

<nav class="project-places" aria-label={t("projects.sidebar.places")} data-project-places>
  {#each views as view, index (view.id)}
    {@const Icon = PROJECT_VIEW_TYPE_ICONS[view.viewType]}
    {@const label = projectViewLabel(t, view)}
    {@const active = activeViewId === view.id}
    <button
      type="button"
      class="explorer-icon"
      class:explorer-icon-active={active}
      aria-label={label}
      aria-current={active ? "page" : undefined}
      data-app-tooltip={viewTooltip(label, index)}
      data-project-place={view.viewType}
      onclick={() => onSelectView(view)}
    >
      <Icon class="explorer-row-icon" aria-hidden="true" />
    </button>
  {/each}
</nav>

<style>
  .project-places {
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    align-items: center;
    gap: 0.125rem;
    padding: 0.25rem 0;
  }
</style>
