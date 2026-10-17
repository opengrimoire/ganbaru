<script lang="ts">
  import type { Snippet } from "svelte";
  import { cn } from "$lib/utils";
  let { title, onOpen, leading, actions, cover, children, selected = false, muted = false, compact = false }: {
    title: string;
    onOpen: () => void;
    leading?: Snippet;
    actions?: Snippet;
    cover?: Snippet;
    children?: Snippet;
    selected?: boolean;
    muted?: boolean;
    compact?: boolean;
  } = $props();
</script>

<article class={cn("collection-card group/card relative min-w-0 rounded-lg border bg-card text-card-foreground shadow-sm transition-colors hover:border-foreground/25", selected ? "border-primary/50 bg-accent/30" : "border-border", muted && "opacity-60")}>
  {#if cover}<div class="overflow-hidden rounded-t-lg">{@render cover()}</div>{/if}
  <div class={compact ? "p-1.5" : "p-3"}>
    <div class="flex min-w-0 items-start gap-1.5">
      {@render leading?.()}
      <button type="button" class={cn("min-w-0 flex-1 rounded-sm text-left text-[0.866667rem] font-medium outline-none hover:text-primary focus-visible:bg-accent", compact ? "min-h-6" : "min-h-8")} onclick={onOpen}><span class="line-clamp-3 wrap-anywhere">{title}</span></button>
      {#if actions}<div class="shrink-0">{@render actions()}</div>{/if}
    </div>
    {#if children}<div class="mt-2 min-w-0 text-[0.8rem]">{@render children()}</div>{/if}
  </div>
</article>
