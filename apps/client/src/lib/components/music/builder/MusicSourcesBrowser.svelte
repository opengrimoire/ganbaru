<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { LocalRootBinding, MusicItemListEntry } from "$lib/music/library/contracts";
  import type { MusicSourceBrowserNode } from "$lib/music/sources/browser";
  import type { MusicSnoozePreset } from "$lib/music/session/snooze";
  import MusicSourceContentList from "./MusicSourceContentList.svelte";

  let {
    nodes,
    selectedNode,
    selectedPath,
    bindings,
    playingItemId = null,
    startingItemId = null,
    playbackActive = false,
    showLocationAction = true,
    onSelect,
    onTogglePlayback,
    onShowLocation,
    onSnooze,
    onRemoveSnooze,
  }: {
    nodes: MusicSourceBrowserNode[];
    selectedNode: MusicSourceBrowserNode | null;
    selectedPath: MusicSourceBrowserNode[];
    bindings: readonly LocalRootBinding[];
    playingItemId?: string | null;
    startingItemId?: string | null;
    playbackActive?: boolean;
    showLocationAction?: boolean;
    onSelect: (nodeId: string | null) => void;
    onTogglePlayback: (item: MusicItemListEntry) => void;
    onShowLocation: (item: MusicItemListEntry) => Promise<void>;
    onSnooze: (item: MusicItemListEntry, duration: MusicSnoozePreset, everywhere: boolean) => Promise<void>;
    onRemoveSnooze: (item: MusicItemListEntry) => Promise<void>;
  } = $props();

  const { t } = getLocalization();
  const visibleNodes = $derived(selectedNode?.children ?? nodes);
  const visibleItems = $derived(selectedNode?.directItems ?? []);

</script>

<section class="source-browser flex min-h-0 flex-1 flex-col" data-music-scrollable="true">
  <header class="border-b border-border/45 px-4 py-3">
    <nav class="mb-1.5 flex min-w-0 items-center gap-1 text-[0.62rem] text-muted-foreground" aria-label={t("music.builder.sourceLocation")}>
      <button type="button" onclick={() => onSelect(null)} class="truncate hover:text-foreground" aria-current={selectedNode === null ? "page" : undefined}>{t("music.builder.allSources")}</button>
      {#each selectedPath as node (node.id)}
        <ChevronRight size={10} class="shrink-0" />
        <button type="button" onclick={() => onSelect(node.id)} class="truncate hover:text-foreground" aria-current={selectedNode?.id === node.id ? "page" : undefined}>{node.name}</button>
      {/each}
    </nav>
    <div class="flex min-w-0 items-baseline gap-2">
      <h1 class="truncate text-sm font-semibold">{selectedNode?.name ?? t("music.builder.allSources")}</h1>
      <span class="shrink-0 text-[0.65rem] tabular-nums text-muted-foreground">{t("music.tracks", selectedNode?.itemIds.length ?? nodes.reduce((total, node) => total + node.itemIds.length, 0))}</span>
    </div>
  </header>

  {#if visibleNodes.length === 0 && visibleItems.length === 0}
    <div class="grid min-h-44 place-items-center px-5 text-center">
      <div>
        <h2 class="text-sm font-medium">{t("music.builder.noSourceItems")}</h2>
        <p class="mt-1 text-xs text-muted-foreground">{selectedNode?.sourceType === "youtube" ? t("music.builder.addYouTubeLinkHint") : t("music.builder.addLocalFolderHint")}</p>
      </div>
    </div>
  {:else}
    <div class="min-h-0 flex-1">
      <MusicSourceContentList
        folders={visibleNodes}
        items={visibleItems}
        {bindings}
        {playingItemId}
        {startingItemId}
        {playbackActive}
        {showLocationAction}
        onOpenFolder={onSelect}
        {onTogglePlayback}
        {onShowLocation}
        {onSnooze}
        {onRemoveSnooze}
      />
    </div>
  {/if}
</section>

<style>
  .source-browser { min-height: 0; }
</style>
