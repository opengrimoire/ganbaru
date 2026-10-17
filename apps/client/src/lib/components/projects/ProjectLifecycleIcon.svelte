<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { projectLifecycleLabel } from "$lib/projects/display";
  import type { ProjectLifecycleStatus } from "$lib/projects/types";
  import { cn } from "$lib/utils";

  /** Marks a hidden or archived project with a muted icon; active projects render nothing. */
  let {
    status,
    size = 13,
    strokeWidth = 1.6,
    class: className = "",
  }: {
    status: ProjectLifecycleStatus;
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();

  const { t } = getLocalization();
  const label = $derived(projectLifecycleLabel(status, t));
</script>

{#if status !== "active"}
  <span role="img" aria-label={label} title={label} class={cn("inline-flex shrink-0 text-muted-foreground", className)}>
    {#if status === "hidden"}
      <EyeOff {size} {strokeWidth} aria-hidden="true" />
    {:else}
      <Archive {size} {strokeWidth} aria-hidden="true" />
    {/if}
  </span>
{/if}
