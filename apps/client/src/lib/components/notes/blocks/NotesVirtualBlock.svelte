<script lang="ts">
  import type { Snippet } from "svelte";
  import type { NotesBlockTreeItem } from "$lib/notes/types";
  import type { NotesCalloutLayer } from "$lib/notes/block-types/callout-layout";
  import NotesCalloutLayers from "./NotesCalloutLayers.svelte";

  let {
    item,
    blockId,
    retainedHeight,
    measure,
    children,
    calloutLayers = [],
  }: {
    item: NotesBlockTreeItem | undefined;
    blockId: string;
    retainedHeight: number;
    measure: (
      node: HTMLElement,
      blockId: string,
    ) => { update(nextBlockId: string): void; destroy(): void };
    children: Snippet<[NotesBlockTreeItem]>;
    calloutLayers?: readonly NotesCalloutLayer[];
  } = $props();

  const isFirstCallout = $derived(calloutLayers[0]?.first ?? false);
  const isLastCallout = $derived(calloutLayers[0]?.last ?? false);
</script>

{#snippet body()}
  {#if item}
    {@render children(item)}
  {:else}
    <div class="rounded-sm bg-muted/20" style:height={`${retainedHeight}px`} data-notes-block-placeholder={blockId} aria-hidden="true"></div>
  {/if}
{/snippet}

{#if item}
  <div use:measure={item.block.id} data-notes-virtual-block={item.block.id} class:notes-callout-first={isFirstCallout} class:notes-callout-last={isLastCallout} class:notes-ordinary-block={!calloutLayers.length}>
    <NotesCalloutLayers layers={calloutLayers} {body} />
  </div>
{:else}
  <div class:notes-callout-first={isFirstCallout} class:notes-callout-last={isLastCallout} class:notes-ordinary-block={!calloutLayers.length}>
    <NotesCalloutLayers layers={calloutLayers} {body} />
  </div>
{/if}

<style>
  .notes-ordinary-block { margin-bottom: 0.125rem; }
  .notes-callout-first { margin-top: 0.4rem; }
  .notes-callout-last { margin-bottom: 0.4rem; }
</style>
