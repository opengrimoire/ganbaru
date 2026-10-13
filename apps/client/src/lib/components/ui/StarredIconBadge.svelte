<script lang="ts">
  /**
   * Wraps a picker row's icon and marks a favorite with a small star on its bottom-right corner, so a list that puts
   * favorites first still reads as ordered without changing the row's text layout.
   */
  import type { Snippet } from "svelte";
  import Star from "@lucide/svelte/icons/star";
  import { cn } from "$lib/utils";

  let {
    starred = false,
    class: className,
    children,
  }: {
    starred?: boolean;
    class?: string;
    children: Snippet;
  } = $props();
</script>

<span class={cn("relative inline-flex shrink-0", className)}>
  {@render children()}
  {#if starred}
    <Star class="starred-icon-badge absolute -right-0.5 -bottom-0.5 size-2 fill-current text-primary" aria-hidden="true" />
  {/if}
</span>
