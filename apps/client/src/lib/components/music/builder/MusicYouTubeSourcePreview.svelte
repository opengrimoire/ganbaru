<script lang="ts">
  import Play from "@lucide/svelte/icons/play";
  import type { MusicYouTubeSourcePreview } from "$lib/music/sources/youtube-source-resolver";

  let { preview }: { preview: MusicYouTubeSourcePreview } = $props();

  let selectedVideoId = $state<string | null>(null);
  const selectedIndex = $derived(
    selectedVideoId ? preview.videoIds.indexOf(selectedVideoId) : -1,
  );
  const embedUrl = $derived.by(() => {
    if (preview.kind === "youtube-playlist" && preview.playlistId) {
      if (selectedVideoId) {
        const index = Math.max(0, selectedIndex);
        return `https://www.youtube-nocookie.com/embed/${encodeURIComponent(selectedVideoId)}?list=${encodeURIComponent(preview.playlistId)}&index=${index}&autoplay=1`;
      }
      return `https://www.youtube-nocookie.com/embed/videoseries?list=${encodeURIComponent(preview.playlistId)}`;
    }
    return preview.videoId
      ? `https://www.youtube-nocookie.com/embed/${encodeURIComponent(preview.videoId)}`
      : "";
  });
</script>

<section class="youtube-preview" aria-label={preview.title}>
  {#if embedUrl}
    <div class="youtube-player">
      <iframe
        src={embedUrl}
        title={preview.title}
        allow="autoplay; encrypted-media; picture-in-picture"
        allowfullscreen
        referrerpolicy="strict-origin-when-cross-origin"
      ></iframe>
    </div>
  {/if}

  <div class="youtube-summary">
    <div class="min-w-0">
      <strong class="block truncate text-sm">{preview.title}</strong>
      {#if preview.channel}<span class="mt-0.5 block truncate text-xs text-muted-foreground">{preview.channel}</span>{/if}
    </div>
    {#if preview.kind === "youtube-playlist"}
      <span class="shrink-0 text-xs tabular-nums text-muted-foreground">{preview.videoIds.length}</span>
    {/if}
  </div>

  {#if preview.kind === "youtube-playlist"}
    <div class="youtube-track-list" data-music-scrollable="true">
      {#each preview.videos as video, index (video.videoId)}
        <button
          type="button"
          class:selected={selectedVideoId === video.videoId}
          class="youtube-track"
          onclick={() => { selectedVideoId = video.videoId; }}
          aria-label={`Play ${video.title}`}
        >
          <span class="track-index">{#if selectedVideoId === video.videoId}<Play size={12} fill="currentColor" />{:else}{index + 1}{/if}</span>
          <span class="min-w-0 text-left">
            <span class="block truncate text-xs font-medium text-foreground">{video.title}</span>
            {#if video.channel}<span class="mt-0.5 block truncate text-[0.68rem] text-muted-foreground">{video.channel}</span>{/if}
          </span>
        </button>
      {/each}
    </div>
  {/if}
</section>

<style>
  .youtube-preview { overflow: hidden; border: 1px solid var(--border); border-radius: 0.5rem; }
  .youtube-player { aspect-ratio: 16 / 9; width: 100%; background: #000; }
  .youtube-player iframe { display: block; height: 100%; width: 100%; border: 0; }
  .youtube-summary { display: flex; align-items: center; justify-content: space-between; gap: 1rem; padding: 0.8rem 0.9rem; }
  .youtube-track-list { max-height: 13rem; overflow-y: auto; border-top: 1px solid var(--border); scrollbar-width: thin; }
  .youtube-track { display: grid; width: 100%; grid-template-columns: 1.75rem minmax(0, 1fr); align-items: center; gap: 0.35rem; border-bottom: 1px solid color-mix(in srgb, var(--border) 72%, transparent); padding: 0.62rem 0.9rem; color: var(--muted-foreground); }
  .youtube-track:last-child { border-bottom: 0; }
  .youtube-track:hover, .youtube-track.selected { background: color-mix(in srgb, var(--accent) 65%, transparent); }
  .youtube-track.selected { color: var(--foreground); }
  .track-index { display: grid; min-width: 0; place-items: center; font-size: 0.68rem; font-variant-numeric: tabular-nums; }
</style>
