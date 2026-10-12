<script lang="ts">
  import type { CollectionIcon } from "./property-icons";
  import { cn } from "$lib/utils";

  /** One segment of a collection selection toolbar; an icon-only segment keeps its label for tooltips and assistive technology. */
  let { icon: Icon, label, iconOnly = false, disabled = false, onclick }: {
    icon: CollectionIcon;
    label: string;
    iconOnly?: boolean;
    disabled?: boolean;
    onclick: () => void;
  } = $props();
</script>

<button type="button"
  class={cn("collection-selection-segment flex h-8 shrink-0 items-center gap-1.5 text-collection text-muted-foreground transition-colors hover:bg-accent/60 hover:text-foreground focus-visible:bg-accent focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50",
    iconOnly ? "w-8 justify-center" : "px-2.5")}
  aria-label={iconOnly ? label : undefined} {disabled} {onclick}>
  <Icon class="size-3.5 shrink-0" strokeWidth={1.75} aria-hidden="true" />
  {#if !iconOnly}<span class="whitespace-nowrap">{label}</span>{/if}
</button>

<style>
  @media (any-pointer: coarse) and (any-hover: none) {
    .collection-selection-segment {
      min-height: var(--panel-row-height);
      min-width: 2.75rem;
    }
  }
</style>
