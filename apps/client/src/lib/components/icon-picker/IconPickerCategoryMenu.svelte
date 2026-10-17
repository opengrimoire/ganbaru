<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Shapes from "@lucide/svelte/icons/shapes";
  import type {
    ProjectLucideCategory,
    ProjectLucideIconEntry,
  } from "$lib/projects/icons/lucide-catalog.generated";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import LucideNodeIcon from "./LucideNodeIcon.svelte";

  let {
    rootElement = $bindable<HTMLElement | undefined>(),
    style,
    portalTarget = "body",
    options,
    selectedCategory,
    ariaLabel,
    onSelect,
  }: {
    rootElement?: HTMLElement;
    style: string;
    /** Where the panel is portaled; a picker nested in another floating surface keeps its panels inside it. */
    portalTarget?: HTMLElement | string;
    options: readonly { category: ProjectLucideCategory; icon: ProjectLucideIconEntry | undefined }[];
    selectedCategory: ProjectLucideCategory | "all";
    ariaLabel: string;
    onSelect: (category: ProjectLucideCategory) => void;
  } = $props();
</script>

<div
  bind:this={rootElement}
  use:portal={portalTarget}
  class="surface-floating fixed z-100 flex min-h-0 flex-col overflow-hidden"
  {style}
  role="dialog"
  data-app-floating-surface
  aria-label={ariaLabel}
>
  <div use:scrollEdgeFadeAction class="surface-floating-body min-h-0 overflow-y-auto">
    {#each options as option (option.category)}
      <button
        type="button"
        class="menu-item"
        aria-checked={selectedCategory === option.category}
        role="menuitemradio"
        onclick={() => onSelect(option.category)}
      >
        <span class="flex size-5 shrink-0 items-center justify-center">
          {#if option.icon}
            <LucideNodeIcon iconNode={option.icon.iconNode} size={15} strokeWidth={1.75} class="block" />
          {:else}
            <Shapes size={15} strokeWidth={1.75} class="block" />
          {/if}
        </span>
        <span class="min-w-0 flex-1 truncate">{option.category}</span>
        {#if selectedCategory === option.category}
          <Check size={14} strokeWidth={1.75} class="shrink-0" aria-hidden="true" />
        {/if}
      </button>
    {/each}
  </div>
</div>
