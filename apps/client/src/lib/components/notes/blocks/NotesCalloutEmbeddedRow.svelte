<script lang="ts">
  import type { Snippet } from "svelte";
  import type { NotesCalloutLayer } from "$lib/notes/block-types/callout-layout";
  import NotesCalloutLayers from "./NotesCalloutLayers.svelte";

  let { layers, body }: { layers: readonly NotesCalloutLayer[]; body: Snippet } = $props();
  const isFirstCallout = $derived(layers[0]?.first ?? false);
  const isLastCallout = $derived(layers[0]?.last ?? false);
</script>

<div class:notes-callout-first={isFirstCallout} class:notes-callout-last={isLastCallout} class:notes-ordinary-block={!layers.length}>
  <NotesCalloutLayers {layers} {body} />
</div>

<style>
  .notes-ordinary-block { margin-bottom: 0.125rem; }
  .notes-callout-first { margin-top: 0.4rem; }
  .notes-callout-last { margin-bottom: 0.4rem; }
</style>
