<script lang="ts">
  import Music2 from "@lucide/svelte/icons/music-2";
  import Youtube from "@lucide/svelte/icons/youtube";
  import type { LocalRootBinding, MusicItemListEntry } from "$lib/music/library/contracts";
  import { musicArtworkDataUrl, musicEmbeddedArtworkDataUrl, musicYouTubeThumbnailDataUrl } from "$lib/music/library/artwork-cache";
  import { musicListArtworkSource } from "$lib/music/builder/list-artwork";
  import { youtubeVideoIdFromIdentity } from "$lib/music/sources";

  let {
    item,
    bindings,
  }: {
    item: MusicItemListEntry;
    bindings: readonly LocalRootBinding[];
  } = $props();

  let currentUrl = $state<string | null>(null);
  let refreshTick = $state(0);
  let requestVersion = 0;
  const visibleThumbnailRefreshMs = 24 * 60 * 60 * 1000;

  $effect(() => {
    const source = musicListArtworkSource(item, bindings);
    const youtubeVideoId = item.sourceKind === "youtube-video"
      ? youtubeVideoIdFromIdentity(item.identityKey)
      : null;
    void refreshTick;
    item.updatedAtMs;
    const request = ++requestVersion;
    currentUrl = null;
    const load = source
      ? source.kind === "file"
        ? musicArtworkDataUrl(source.path)
        : musicEmbeddedArtworkDataUrl(source.path, source.identity)
      : youtubeVideoId
        ? musicYouTubeThumbnailDataUrl(youtubeVideoId)
        : null;
    if (!load) return;
    void load.then((url) => {
      if (request !== requestVersion || !url) return;
      const image = new Image();
      image.onload = () => {
        if (request === requestVersion) currentUrl = url;
      };
      image.src = url;
    });
    if (youtubeVideoId && !source) {
      const refreshTimer = window.setTimeout(() => { refreshTick += 1; }, visibleThumbnailRefreshMs);
      return () => { window.clearTimeout(refreshTimer); };
    }
  });
</script>

{#if currentUrl}
  <img src={currentUrl} alt="" aria-hidden="true" class={item.sourceKind === "youtube-video" && !item.artworkOverride ? "h-full w-full object-contain" : "h-full w-full object-cover"} />
{:else if item.sourceKind === "youtube-video"}
  <Youtube size={17} strokeWidth={1.5} />
{:else}
  <Music2 size={16} strokeWidth={1.5} />
{/if}
