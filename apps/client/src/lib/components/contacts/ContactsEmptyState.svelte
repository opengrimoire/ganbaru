<script lang="ts">
  import type { Snippet } from "svelte";

  /** Dashed empty state for Contacts lists: an icon, a short title, and the one sentence that says how the list fills. */
  let {
    title,
    description = null,
    icon,
    action,
  }: {
    title: string;
    description?: string | null;
    icon: Snippet;
    /** An optional call to action rendered under the description. */
    action?: Snippet;
  } = $props();
</script>

<div class="flex flex-col items-center gap-2 rounded-lg border border-dashed border-border px-4 py-6 text-center">
  <span class="text-muted-foreground">{@render icon()}</span>
  <p class="text-sm font-medium text-foreground">{title}</p>
  {#if description}<p class="max-w-sm text-[0.8rem] text-muted-foreground">{description}</p>{/if}
  {#if action}<div class="mt-1">{@render action()}</div>{/if}
</div>
