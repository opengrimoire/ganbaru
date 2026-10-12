<script lang="ts">
  import type { Snippet } from "svelte";
  import { notesCalloutLayerStyle, type NotesCalloutLayer } from "$lib/notes/block-types/callout-layout";

  let { layers, body }: { layers: readonly NotesCalloutLayer[]; body: Snippet } = $props();
</script>

{#snippet content(index: number)}
  {#if index < layers.length}
    {@const layer = layers[index]}
    <div
      class="notes-callout-layer"
      class:notes-nested-callout-layer={index > 0}
      class:notes-callout-layer-first={layer.first}
      class:notes-callout-layer-last={layer.last}
      style={notesCalloutLayerStyle(layer.color)}
      data-notes-callout-id={layer.id}
    >
      {@render content(index + 1)}
    </div>
  {:else}
    {@render body()}
  {/if}
{/snippet}

{@render content(0)}

<style>
  .notes-callout-layer {
    --notes-callout-inset: 0.75rem;
    background: var(--notes-callout-background);
    padding-inline: var(--notes-callout-inset);
  }
  .notes-nested-callout-layer { margin-left: 1.25rem; }
  .notes-callout-layer-first { padding-top: 0.65rem; border-radius: 0.75rem 0.75rem 0 0; }
  .notes-callout-layer-last { padding-bottom: 0.65rem; border-radius: 0 0 0.75rem 0.75rem; }
  .notes-callout-layer-first.notes-callout-layer-last { border-radius: 0.75rem; }
</style>
