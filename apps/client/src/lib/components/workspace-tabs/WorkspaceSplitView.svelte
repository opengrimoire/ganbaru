<script lang="ts" module>
  import type { View } from "$lib/navigation";

  /** A tab shown in the content area and the view that shows it. */
  export interface WorkspaceShownPane {
    tabId: string;
    view: View;
  }
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { DEFAULT_SPLIT_RATIO, MIN_SPLIT_RATIO, type WorkspaceTabSplit } from "$lib/workspace-tabs/model";
  import {
    SPLIT_DIVIDER_SIZE,
    SPLIT_KEYBOARD_STEP,
    fitSplitRatio,
    minSplitPaneLength,
    workspaceSplitLayout,
    type WorkspaceSplitArea,
    type WorkspaceSplitLayout,
  } from "$lib/workspace-tabs/split-layout";
  import WorkspacePane from "./WorkspacePane.svelte";

  let {
    panes,
    split,
    focusedTabId,
    onFocusPane,
    onResize,
    content,
    area = $bindable({ width: 0, height: 0 }),
  }: {
    /** The shown tabs in split order, first on the left or top; one pane when there is no split. */
    panes: readonly WorkspaceShownPane[];
    split: WorkspaceTabSplit | null;
    focusedTabId: string;
    onFocusPane: (tabId: string) => void;
    onResize: (ratio: number) => void;
    content: Snippet<[WorkspaceShownPane]>;
    /** The size of the area the panes share, which decides how a split can show. */
    area?: WorkspaceSplitArea;
  } = $props();

  const { t } = getLocalization();

  let width = $state(0);
  let height = $state(0);
  let root: HTMLDivElement | undefined = $state();
  /** The first pane's share while the divider is dragged, committed when the drag ends. */
  let dragRatio = $state<number | null>(null);

  const layout = $derived<WorkspaceSplitLayout>(
    split && panes.length === 2
      ? workspaceSplitLayout(split.orientation, width, height, SPLIT_DIVIDER_SIZE)
      : "single",
  );
  $effect(() => {
    area = { width, height };
  });
  const extent = $derived((layout === "stacked" ? height : width) - SPLIT_DIVIDER_SIZE);
  const ratio = $derived(
    layout === "single"
      ? DEFAULT_SPLIT_RATIO
      : fitSplitRatio(dragRatio ?? split?.ratio ?? DEFAULT_SPLIT_RATIO, extent, minSplitPaneLength(layout)),
  );
  /** Panes in a fixed DOM order, so swapping sides or changing the split never moves or remounts a view. */
  const stablePanes = $derived([...panes].sort((a, b) => a.view.localeCompare(b.view)));

  function paneIndex(pane: WorkspaceShownPane): number {
    return panes.findIndex((candidate) => candidate.tabId === pane.tabId);
  }

  function paneBasis(pane: WorkspaceShownPane): string | null {
    if (layout === "single" || paneIndex(pane) !== 0) return null;
    return `calc((100% - ${SPLIT_DIVIDER_SIZE}px) * ${ratio})`;
  }

  function ratioAt(event: PointerEvent): number | null {
    const rect = root?.getBoundingClientRect();
    if (!rect || layout === "single") return null;
    const offset = layout === "stacked" ? event.clientY - rect.top : event.clientX - rect.left;
    const length = (layout === "stacked" ? rect.height : rect.width) - SPLIT_DIVIDER_SIZE;
    return length > 0 ? (offset - SPLIT_DIVIDER_SIZE / 2) / length : null;
  }

  function commit(next: number): void {
    if (layout === "single") return;
    onResize(fitSplitRatio(next, extent, minSplitPaneLength(layout)));
  }

  function handlePointerDown(event: PointerEvent): void {
    if (event.button !== 0) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    dragRatio = ratioAt(event);
  }

  function handlePointerMove(event: PointerEvent): void {
    if (dragRatio === null) return;
    dragRatio = ratioAt(event) ?? dragRatio;
  }

  function endDrag(): void {
    if (dragRatio === null) return;
    const next = dragRatio;
    dragRatio = null;
    commit(next);
  }

  function handleKeydown(event: KeyboardEvent): void {
    const backKey = layout === "stacked" ? "ArrowUp" : "ArrowLeft";
    const forwardKey = layout === "stacked" ? "ArrowDown" : "ArrowRight";
    let next: number | null = null;
    if (event.key === backKey) next = ratio - SPLIT_KEYBOARD_STEP;
    else if (event.key === forwardKey) next = ratio + SPLIT_KEYBOARD_STEP;
    else if (event.key === "Home") next = MIN_SPLIT_RATIO;
    else if (event.key === "End") next = 1 - MIN_SPLIT_RATIO;
    else if (event.key === "Enter") next = DEFAULT_SPLIT_RATIO;
    if (next === null) return;
    event.preventDefault();
    commit(next);
  }
</script>

<div
  bind:this={root}
  bind:clientWidth={width}
  bind:clientHeight={height}
  class="workspace-split flex size-full min-h-0 min-w-0"
  class:flex-col={layout === "stacked"}
  class:workspace-split-resizing={dragRatio !== null}
  data-split-layout={layout}
>
  {#each stablePanes as pane (pane.view)}
    {@const index = paneIndex(pane)}
    {@const focused = pane.tabId === focusedTabId}
    <WorkspacePane
      {focused}
      outlined={focused && layout !== "single"}
      hidden={layout === "single" && panes.length > 1 && !focused}
      painted={pane.view === "calendar"}
      order={index * 2}
      basis={paneBasis(pane)}
      onFocus={() => onFocusPane(pane.tabId)}
    >
      {@render content(pane)}
    </WorkspacePane>
  {/each}
  {#if layout !== "single"}
    <!-- A focusable separator with a value is the ARIA window splitter, an interactive widget. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="workspace-split-divider group flex shrink-0 items-center justify-center"
      class:cursor-col-resize={layout === "side-by-side"}
      class:cursor-row-resize={layout === "stacked"}
      style:order={1}
      style:flex-basis="{SPLIT_DIVIDER_SIZE}px"
      role="separator"
      tabindex="0"
      aria-label={t("titleBar.split.resize")}
      aria-orientation={layout === "side-by-side" ? "vertical" : "horizontal"}
      aria-valuemin={Math.round(MIN_SPLIT_RATIO * 100)}
      aria-valuemax={Math.round((1 - MIN_SPLIT_RATIO) * 100)}
      aria-valuenow={Math.round(ratio * 100)}
      onpointerdown={handlePointerDown}
      onpointermove={handlePointerMove}
      onpointerup={endDrag}
      onpointercancel={endDrag}
      onlostpointercapture={endDrag}
      ondblclick={() => commit(DEFAULT_SPLIT_RATIO)}
      onkeydown={handleKeydown}
    >
      <span
        class={[
          "workspace-split-grip rounded-full bg-sidebar-foreground transition-opacity",
          dragRatio === null ? "opacity-0 group-hover:opacity-30 group-focus-visible:opacity-50" : "opacity-50",
        ]}
      ></span>
    </div>
  {/if}
</div>

<style>
  .workspace-split[data-split-layout="side-by-side"] .workspace-split-grip {
    width: 2px;
    height: 2rem;
  }

  .workspace-split[data-split-layout="stacked"] .workspace-split-grip {
    width: 2rem;
    height: 2px;
  }

  /* Views under the pointer must not take hover or text selection while the divider is dragged. */
  .workspace-split-resizing :global(.workspace-pane) {
    pointer-events: none;
    user-select: none;
  }
</style>
