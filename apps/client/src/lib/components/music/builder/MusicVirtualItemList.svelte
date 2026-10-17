<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { LocalRootBinding, MusicItemListEntry, MusicWeight } from "$lib/music/library/contracts";
  import { musicVirtualWindow, revealMusicVirtualIndex } from "$lib/music/builder/virtual-window";
  import type { MusicSnoozePreset } from "$lib/music/session/snooze";
  import MusicBuilderItemRow from "./MusicBuilderItemRow.svelte";

  let {
    items,
    initialScrollTop = 0,
    playingItemId = null,
    startingItemId = null,
    playbackActive = false,
    bindings,
    playlistName,
    showLocationAction = true,
    onTogglePlayback,
    onShowLocation,
    onSnooze,
    onRemoveSnooze,
    onWeight,
    onRemove,
    onScrollTop = () => undefined,
    hasMore = false,
    loadingMore = false,
    onLoadMore = () => undefined,
  }: {
    items: MusicItemListEntry[];
    initialScrollTop?: number;
    playingItemId?: string | null;
    startingItemId?: string | null;
    playbackActive?: boolean;
    bindings: readonly LocalRootBinding[];
    playlistName: string;
    showLocationAction?: boolean;
    onTogglePlayback: (item: MusicItemListEntry) => void;
    onShowLocation: (item: MusicItemListEntry) => Promise<void>;
    onSnooze: (item: MusicItemListEntry, duration: MusicSnoozePreset, everywhere: boolean) => Promise<void>;
    onRemoveSnooze: (item: MusicItemListEntry) => Promise<void>;
    onWeight: (item: MusicItemListEntry, weight: MusicWeight) => Promise<void>;
    onRemove: (item: MusicItemListEntry) => Promise<void>;
    onScrollTop?: (scrollTop: number) => void;
    hasMore?: boolean;
    loadingMore?: boolean;
    onLoadMore?: () => void;
  } = $props();

  const { t } = getLocalization();
  const rowHeight = 64;
  let viewport = $state<HTMLElement | null>(null);
  let scrollTop = $state(0);
  let viewportHeight = $state(0);
  let revealedPlayingItemId = $state<string | null>(null);
  const windowed = $derived(musicVirtualWindow({ count: items.length, scrollTop, viewportHeight, rowHeight, overscan: 6 }));
  const visibleItems = $derived(items.slice(windowed.startIndex, windowed.endIndex));

  $effect(() => {
    if (!playingItemId || playingItemId === revealedPlayingItemId || !viewport) return;
    const index = items.findIndex((item) => item.id === playingItemId);
    if (index < 0) return;
    revealedPlayingItemId = playingItemId;
    viewport.scrollTop = revealMusicVirtualIndex(index, viewport.scrollTop, viewport.clientHeight, rowHeight);
  });

  function viewportAction(node: HTMLElement): { destroy: () => void } {
    viewport = node;
    node.scrollTop = initialScrollTop;
    const update = () => {
      scrollTop = node.scrollTop;
      viewportHeight = node.clientHeight;
      onScrollTop(scrollTop);
      if (hasMore && !loadingMore && node.scrollTop + node.clientHeight >= node.scrollHeight - rowHeight * 4) onLoadMore();
    };
    const observer = new ResizeObserver(update);
    observer.observe(node);
    node.addEventListener("scroll", update, { passive: true });
    update();
    return { destroy: () => { observer.disconnect(); node.removeEventListener("scroll", update); viewport = null; } };
  }

</script>

<div
  class="music-builder-list h-full min-h-0 overflow-y-auto px-2 py-1 outline-none"
  use:viewportAction
  role="list"
  aria-busy={loadingMore}
>
  <div style={`height: ${windowed.topSpacer}px`} aria-hidden="true"></div>
  {#each visibleItems as item, visibleIndex (item.id)}
    <MusicBuilderItemRow
      {item}
      {bindings}
      {playlistName}
      {showLocationAction}
      position={windowed.startIndex + visibleIndex + 1}
      setSize={items.length}
      playing={playbackActive && item.id === playingItemId}
      starting={item.sourceKind === "youtube-video" && item.id === startingItemId}
      {onTogglePlayback}
      {onShowLocation}
      {onSnooze}
      {onRemoveSnooze}
      {onWeight}
      {onRemove}
    />
  {/each}
  <div style={`height: ${windowed.bottomSpacer}px`} aria-hidden="true"></div>
  {#if loadingMore}<div class="mx-2 my-1 h-10 animate-pulse rounded-lg bg-secondary motion-reduce:animate-none" aria-hidden="true"></div>{/if}
  <span class="sr-only" role="status" aria-live="polite">{loadingMore ? t("music.builder.loadingMore") : ""}</span>
</div>

<style>
  .music-builder-list { scrollbar-width: thin; scrollbar-color: color-mix(in srgb, var(--foreground) 18%, transparent) transparent; }
</style>
