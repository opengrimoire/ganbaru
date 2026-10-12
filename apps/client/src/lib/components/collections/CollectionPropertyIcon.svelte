<script lang="ts">
  import Shapes from "@lucide/svelte/icons/shapes";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import { COLLECTION_PROPERTY_ICONS, type CollectionPropertyKind } from "./property-icons";

  /** Shows a property's chosen Lucide icon, or its type icon when none was chosen. A draft without a type shows a neutral icon. */
  let { icon = null, kind, size = 14, strokeWidth = 1.75, class: className = "" }: {
    /** A `lucide:` picker value; null shows the type icon. */
    icon?: string | null;
    kind: CollectionPropertyKind | null;
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();

  const TypeIcon = $derived(kind ? COLLECTION_PROPERTY_ICONS[kind] : Shapes);
</script>

{#if icon}
  <ProjectIcon name={icon} {size} {strokeWidth} class={className} />
{:else}
  <TypeIcon {size} {strokeWidth} class={className} aria-hidden="true" />
{/if}
