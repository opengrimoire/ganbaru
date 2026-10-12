<script lang="ts">
  import type { ProjectLucideIconNode } from "$lib/projects/icons/lucide-catalog.generated";

  let {
    iconNode,
    size = 16,
    strokeWidth = 1.75,
    class: className = "",
    style,
  }: {
    iconNode: readonly ProjectLucideIconNode[];
    size?: number;
    strokeWidth?: number;
    class?: string;
    style?: string;
  } = $props();
</script>

<svg
  xmlns="http://www.w3.org/2000/svg"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  class={className}
  {style}
  aria-hidden="true"
>
  {#each iconNode as [tag, attributes]}
    <svelte:element this={tag} {...attributes} />
  {/each}
</svg>
