<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLAttributes } from "svelte/elements";
  import { cn } from "$lib/utils";
  let { label, element = $bindable<HTMLDivElement | null>(null), class: className, children, ...attributes }: HTMLAttributes<HTMLDivElement> & {
    label: string;
    element?: HTMLDivElement | null;
    children: Snippet;
  } = $props();
</script>

<div {...attributes} bind:this={element} role="dialog" aria-label={label} tabindex="-1" data-floating-root data-app-floating-surface data-app-shortcuts="ignore" class={cn("collection-panel surface-floating flex min-h-0 flex-col overflow-hidden", className)}>
  {@render children()}
</div>

<style>
  .collection-panel :global(:disabled) { opacity: 1; }
  .collection-panel :global([data-collection-menu-body] .grid) {
    align-content: start;
    grid-auto-rows: max-content;
  }
  .collection-panel :global(:is([data-collection-menu-body] button:not([role="switch"]), [data-collection-menu-body] label.flex, [data-collection-settings-page] button:not([role="switch"]), [data-collection-settings-page] label.flex, .collection-menu-trigger, [role="option"])) {
    box-sizing: border-box;
    height: var(--panel-row-height);
    min-height: var(--panel-row-height);
    flex-shrink: 0;
    padding: 0.25rem 0.5rem;
    gap: 0.375rem;
    font-size: var(--panel-font-size);
    line-height: 1.5;
    white-space: nowrap;
  }
  .collection-panel :global(:is([data-collection-menu-body], [data-collection-settings-page]) button[data-collection-row-two-line]) {
    height: auto;
  }
  .collection-panel :global(:is([data-collection-menu-body], [data-collection-settings-page]) button:is(.collection-menu-control, .collection-settings-control)) {
    justify-content: center;
    padding: 0;
  }
  .collection-panel :global([role="separator"] + [role="separator"]) {
    display: none;
  }
  .collection-panel :global(.collection-menu-row) {
    height: var(--panel-row-height);
    min-height: var(--panel-row-height);
    align-items: center;
  }
  .collection-panel :global(:is(.collection-menu-control, .collection-settings-control)) {
    height: var(--panel-row-height);
    min-height: var(--panel-row-height);
    min-width: var(--panel-row-height);
  }
  .collection-panel :global([data-collection-menu-body] button > svg),
  .collection-panel :global([data-collection-settings-page] button > svg),
  .collection-panel :global(.collection-menu-trigger > svg) {
    width: 1rem;
    height: 1rem;
    flex-shrink: 0;
  }
  .collection-panel :global(:is(input, select, textarea)) {
    font-size: var(--panel-font-size);
  }
  @media (any-pointer: coarse) and (any-hover: none) {
    .collection-panel :global(input:not([type="checkbox"]):not([type="radio"])),
    .collection-panel :global(select),
    .collection-panel :global(textarea) { min-height: var(--panel-row-height); }
  }
</style>
