<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import { cn } from "$lib/utils";

  /**
   * The chevron that opens a collection row's nested rows. It stays visible on a row with children; on any other row it
   * appears on hover, so a first child can be added by opening the row.
   */
  let { expanded, hasChildren, label, disabled = false, onToggle }: {
    expanded: boolean;
    hasChildren: boolean;
    label: string;
    disabled?: boolean;
    onToggle: () => void;
  } = $props();
</script>

<button type="button"
  class={cn(
    "flex size-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50",
    !hasChildren && !expanded && "[@media(hover:hover)]:opacity-0 [@media(hover:hover)]:group-hover/row:opacity-100 [@media(hover:hover)]:group-has-focus-visible/row:opacity-100",
  )}
  aria-label={label} aria-expanded={expanded} {disabled} onclick={onToggle}>
  <ChevronRight class={cn("size-3.5 transition-transform", expanded && "rotate-90")} strokeWidth={1.75} aria-hidden="true" />
</button>
