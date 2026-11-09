<script lang="ts">
  import { cn } from "$lib/utils";

  /**
   * A phone welcome scene's touch: a soft ring where a finger presses, shown as far as `opacity` says. It sits at `left`
   * and `top` in its positioned parent, centered there, or at the parent's center by default.
   */
  let {
    opacity,
    left = "50%",
    top = "50%",
    class: className = "",
  }: {
    /** How far the touch shows, from 0 to 1; nothing is drawn at 0. */
    opacity: number;
    left?: string;
    top?: string;
    class?: string;
  } = $props();
</script>

{#if opacity > 0}
  <span
    class={cn("pointer-events-none absolute z-20 size-6 -translate-1/2 rounded-full bg-foreground/20 ring-2 ring-foreground/35", className)}
    style:left
    style:top
    style:opacity
  ></span>
{/if}
