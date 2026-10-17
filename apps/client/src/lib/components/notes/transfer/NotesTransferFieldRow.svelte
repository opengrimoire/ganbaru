<script lang="ts">
  import type { Snippet } from "svelte";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { cn } from "$lib/utils";

  let {
    label,
    description,
    forId,
    labelUrl = null,
    wide = false,
    children,
  }: {
    label: string;
    description?: string;
    forId?: string;
    labelUrl?: string | null;
    wide?: boolean;
    children: Snippet;
  } = $props();

  async function openExternalUrl(url: string): Promise<void> {
    try {
      await openUrl(url);
    } catch (error: unknown) {
      console.warn("Failed to open external URL:", error);
    }
  }
</script>

<div
  class={cn(
    wide
      ? "grid gap-2 px-1 py-1"
      : "flex items-center justify-between gap-4 px-1 py-1 max-[560px]:flex-col max-[560px]:items-stretch max-[560px]:gap-2",
  )}
>
  <div class={cn("min-w-0", wide ? "" : "flex-1")}>
    {#if labelUrl}
      <button
        type="button"
        class="inline-flex min-w-0 max-w-full items-center gap-1 text-left text-[0.866667rem] text-primary underline-offset-2 transition-colors hover:underline focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        data-app-tooltip-disabled="true"
        onclick={() => {
          void openExternalUrl(labelUrl);
        }}
      >
        <span class="truncate">{label}</span>
        <ExternalLink size={12} strokeWidth={2.25} class="shrink-0" />
      </button>
    {:else if forId}
      <label for={forId} class="text-[0.866667rem] text-foreground">{label}</label>
    {:else}
      <div class="text-[0.866667rem] text-foreground">{label}</div>
    {/if}
    {#if description}
      <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">{description}</div>
    {/if}
  </div>
  <div class={cn(wide ? "min-w-0" : "flex shrink-0 items-center justify-end max-[560px]:w-full max-[560px]:justify-start")}>
    {@render children()}
  </div>
</div>
