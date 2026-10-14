<script lang="ts">
  /**
   * The view row a Projects sidebar drag lifts: a copy that follows the pointer up and down the section and settles into
   * the slot the view lands in, or back into its own slot when the drag is canceled.
   */
  import ExplorerRowStar from "$lib/components/ui/ExplorerRowStar.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectView } from "$lib/projects/types";
  import type { ProjectViewDragCardPhase, ProjectViewDragFrame } from "$lib/projects/view-drag";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";
  import { PROJECT_VIEW_TYPE_ICONS, projectViewLabel } from "./project-view-presentation";

  let {
    view,
    active,
    starred,
    frame,
    phase,
    durationMs,
  }: {
    view: ProjectView;
    /** Whether the lifted view is the open one, which keeps its selected highlight. */
    active: boolean;
    starred: boolean;
    frame: ProjectViewDragFrame;
    phase: ProjectViewDragCardPhase;
    durationMs: number;
  } = $props();

  const { t } = getLocalization();
  const Icon = $derived(PROJECT_VIEW_TYPE_ICONS[view.viewType]);
</script>

<div
  use:portal
  class={cn(
    "project-view-drag-card explorer-sidebar pointer-events-none fixed top-0 left-0 z-70 rounded-md",
    phase === "lifted"
      ? "scale-[1.02] shadow-xl ring-1 ring-border starting:scale-100 starting:shadow-none"
      : "scale-100 shadow-none",
  )}
  data-phase={phase}
  style:translate={`${frame.left}px ${frame.top}px`}
  style:width={`${frame.width}px`}
  style:height={`${frame.height}px`}
  style:--project-view-drag-card-duration={`${durationMs}ms`}
  aria-hidden="true"
>
  <div class="explorer-row flex h-full min-w-0 items-center" class:explorer-selected={active}>
    <span class="explorer-row-label flex min-w-0 flex-1 items-center gap-1.5 px-2">
      <Icon class="explorer-row-icon" aria-hidden="true" />
      <span class="min-w-0 flex-1 truncate">{projectViewLabel(t, view)}</span>
    </span>
    {#if starred}
      <span class="explorer-row-trailing">
        <ExplorerRowStar />
      </span>
    {/if}
  </div>
</div>

<style>
  .project-view-drag-card {
    background: var(--popover);
    color: var(--popover-foreground);
    transition-property: scale, box-shadow;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
  }

  .project-view-drag-card[data-phase="settling"] {
    background: transparent;
    color: inherit;
    transition-property: translate, scale, box-shadow, background-color;
    transition-duration: var(--project-view-drag-card-duration);
    transition-timing-function: cubic-bezier(0.2, 0, 0, 1);
  }

  @media (prefers-reduced-motion: reduce) {
    .project-view-drag-card {
      transition: none;
    }
  }
</style>
