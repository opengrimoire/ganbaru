<script lang="ts">
  import CollectionBadge from "$lib/components/collections/CollectionBadge.svelte";
  import {
    projectStatusBadgeClass,
    projectStatusBadgeDotStyle,
    projectStatusBadgeStyle,
  } from "$lib/projects/display";
  import type { ProjectStatus } from "$lib/projects/types";
  import type { Theme } from "$lib/themes";
  import { cn } from "$lib/utils";

  let {
    status,
    theme,
    label,
    class: className = "",
    textClass = "",
  }: {
    status: ProjectStatus | undefined;
    theme: Theme;
    label: string;
    class?: string;
    textClass?: string;
  } = $props();
</script>

<CollectionBadge {label} {textClass} class={cn(projectStatusBadgeClass(status), className)} style={projectStatusBadgeStyle(status, theme)}>
  {#snippet leading()}
    {#if status}<span class="size-2 shrink-0 rounded-full" style={projectStatusBadgeDotStyle(status, theme)} aria-hidden="true"></span>{/if}
  {/snippet}
</CollectionBadge>
