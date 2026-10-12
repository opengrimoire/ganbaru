<script lang="ts">
  import type { Snippet } from "svelte";
  import { cn } from "$lib/utils";
  let { label, style, class: className, textClass, leading }: { label: string; style?: string; class?: string; textClass?: string; leading?: Snippet } = $props();
</script>

<span class={cn("inline-flex max-w-full min-w-0 items-center gap-1.5 rounded-full px-2 py-0.5 text-[0.733333rem] font-medium", className)} {style}>
  {@render leading?.()}
  <span class={cn("min-w-0 truncate", textClass)}>{label}</span>
</span>
