<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Folder from "@lucide/svelte/icons/folder";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { LocalRootBinding, MusicItemListEntry } from "$lib/music/library/contracts";
  import type { MusicSourceBrowserNode } from "$lib/music/sources/browser";
  import type { MusicSnoozePreset } from "$lib/music/session/snooze";
  import { musicVirtualWindow, revealMusicVirtualIndex } from "$lib/music/builder/virtual-window";
  import MusicBuilderItemRow from "./MusicBuilderItemRow.svelte";

  type ContentEntry =
    | { kind: "folder"; node: MusicSourceBrowserNode }
    | { kind: "item"; item: MusicItemListEntry };

  let {
    folders,
    items,
    bindings,
    playingItemId = null,
    startingItemId = null,
    playbackActive = false,
    showLocationAction = true,
    onOpenFolder,
    onTogglePlayback,
    onShowLocation,
    onSnooze,
    onRemoveSnooze,
  }: {
    folders: MusicSourceBrowserNode[];
    items: MusicItemListEntry[];
    bindings: readonly LocalRootBinding[];
    playingItemId?: string | null;
    startingItemId?: string | null;
    playbackActive?: boolean;
    showLocationAction?: boolean;
    onOpenFolder: (nodeId: string) => void;
    onTogglePlayback: (item: MusicItemListEntry) => void;
    onShowLocation: (item: MusicItemListEntry) => Promise<void>;
    onSnooze: (item: MusicItemListEntry, duration: MusicSnoozePreset, everywhere: boolean) => Promise<void>;
    onRemoveSnooze: (item: MusicItemListEntry) => Promise<void>;
  } = $props();

  const { t } = getLocalization();
  const rowHeight = 64;
  let viewport = $state<HTMLElement | null>(null);
  let scrollTop = $state(0);
  let viewportHeight = $state(0);
  let revealedPlayingItemId = $state<string | null>(null);
  const entries = $derived<ContentEntry[]>([
    ...folders.map((node): ContentEntry => ({ kind: "folder", node })),
    ...items.map((item): ContentEntry => ({ kind: "item", item })),
  ]);
  const windowed = $derived(musicVirtualWindow({ count: entries.length, scrollTop, viewportHeight, rowHeight, overscan: 6 }));
  const visibleEntries = $derived(entries.slice(windowed.startIndex, windowed.endIndex));

  $effect(() => {
    if (!playingItemId || playingItemId === revealedPlayingItemId || !viewport) return;
    const index = entries.findIndex((entry) => entry.kind === "item" && entry.item.id === playingItemId);
    if (index < 0) return;
    revealedPlayingItemId = playingItemId;
    viewport.scrollTop = revealMusicVirtualIndex(index, viewport.scrollTop, viewport.clientHeight, rowHeight);
  });

  function viewportAction(node: HTMLElement): { destroy: () => void } {
    viewport = node;
    const update = () => {
      scrollTop = node.scrollTop;
      viewportHeight = node.clientHeight;
    };
    const observer = new ResizeObserver(update);
    observer.observe(node);
    node.addEventListener("scroll", update, { passive: true });
    update();
    return { destroy: () => { observer.disconnect(); node.removeEventListener("scroll", update); viewport = null; } };
  }
</script>

<div class="source-content-list h-full min-h-0 overflow-y-auto px-2 py-1 outline-none" use:viewportAction role="list">
  <div style={`height: ${windowed.topSpacer}px`} aria-hidden="true"></div>
  {#each visibleEntries as entry, visibleIndex (entry.kind === "folder" ? `folder:${entry.node.id}` : `item:${entry.item.id}`)}
    {#if entry.kind === "folder"}
      <div class="source-folder-row group" role="listitem" aria-posinset={windowed.startIndex + visibleIndex + 1} aria-setsize={entries.length}>
        <button type="button" class="absolute inset-0 rounded-xl focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring" onclick={() => onOpenFolder(entry.node.id)} aria-label={entry.node.name} data-app-tooltip-disabled="true"></button>
        <span class="folder-artwork pointer-events-none"><Folder size={17} strokeWidth={1.5} /></span>
        <span class="pointer-events-none relative min-w-0 flex-1 text-left">
          <strong class="block truncate text-xs font-semibold">{entry.node.name}</strong>
          <span class="mt-0.5 block text-[0.68rem] text-muted-foreground">{t("music.tracks", entry.node.itemIds.length)}</span>
        </span>
        {#if entry.node.issueCount > 0}<span class="pointer-events-none relative text-[0.62rem] text-destructive">{t("music.builder.issueCount", entry.node.issueCount)}</span>{/if}
        <ChevronRight size={14} class="pointer-events-none relative shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 motion-reduce:transition-none" />
      </div>
    {:else}
      <MusicBuilderItemRow
        item={entry.item}
        {bindings}
        context="source"
        {showLocationAction}
        position={windowed.startIndex + visibleIndex + 1}
        setSize={entries.length}
        playing={playbackActive && entry.item.id === playingItemId}
        starting={entry.item.sourceKind === "youtube-video" && entry.item.id === startingItemId}
        {onTogglePlayback}
        {onShowLocation}
        {onSnooze}
        {onRemoveSnooze}
      />
    {/if}
  {/each}
  <div style={`height: ${windowed.bottomSpacer}px`} aria-hidden="true"></div>
</div>

<style>
  .source-content-list { scrollbar-width: thin; scrollbar-color: color-mix(in srgb, var(--foreground) 18%, transparent) transparent; }
  .source-folder-row { position: relative; display: flex; height: 4rem; min-width: 0; align-items: center; gap: 0.65rem; border-radius: 0.75rem; padding: 0.35rem 0.45rem; }
  .source-folder-row:hover { background: color-mix(in srgb, var(--accent) 55%, transparent); }
  .folder-artwork { position: relative; display: grid; height: 2.75rem; width: 2.75rem; flex: none; place-items: center; overflow: hidden; border-radius: 0.65rem; background: color-mix(in srgb, var(--secondary) 78%, transparent); color: var(--muted-foreground); }
</style>
