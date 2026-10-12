<script lang="ts">
  import type { Snippet } from "svelte";
  import { cn } from "$lib/utils";
  let { label, title = label, active, compact = false, disabled = false, class: className, children, onclick }: {
    label: string;
    title?: string;
    active: boolean;
    compact?: boolean;
    disabled?: boolean;
    class?: string;
    children?: Snippet;
    onclick: () => void;
  } = $props();
</script>

<button type="button" class={cn("inline-flex h-8 shrink-0 items-center justify-center gap-1.5 rounded-md px-2 text-[0.866667rem] font-medium transition-colors hover:bg-accent/60 focus-visible:bg-accent focus-visible:outline-none disabled:cursor-not-allowed disabled:hover:bg-transparent", active ? "bg-accent text-foreground" : "text-muted-foreground", compact && "w-8 px-0", className)} aria-label={label} aria-pressed={active} {disabled} {title} {onclick}>
  {@render children?.()}
  {#if !compact}<span class="max-w-36 truncate">{label}</span>{/if}
</button>
