<script lang="ts">
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Pause from "@lucide/svelte/icons/pause";
  import Play from "@lucide/svelte/icons/play";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { LocalRootBinding, MusicItemListEntry, MusicWeight } from "$lib/music/library/contracts";
  import {
    formatMusicDuration,
    musicAvailabilityTone,
    musicItemSecondaryText,
  } from "$lib/music/builder/presentation";
  import type { MusicSnoozePreset } from "$lib/music/session/snooze";
  import { cn } from "$lib/utils";
  import MusicSnoozeButton from "$lib/components/music/MusicSnoozeButton.svelte";
  import MusicArtworkThumbnail from "./MusicArtworkThumbnail.svelte";
  import MusicPlaylistItemMenu from "./MusicPlaylistItemMenu.svelte";

  let {
    item,
    playing = false,
    starting = false,
    position = undefined,
    setSize = undefined,
    bindings,
    playlistName = "",
    context = "playlist",
    showLocationAction = true,
    onTogglePlayback,
    onShowLocation,
    onSnooze,
    onRemoveSnooze,
    onWeight,
    onRemove,
  }: {
    item: MusicItemListEntry;
    bindings: readonly LocalRootBinding[];
    playing?: boolean;
    starting?: boolean;
    position?: number;
    setSize?: number;
    playlistName?: string;
    context?: "playlist" | "source";
    showLocationAction?: boolean;
    onTogglePlayback: (item: MusicItemListEntry) => void;
    onShowLocation: (item: MusicItemListEntry) => Promise<void>;
    onSnooze: (item: MusicItemListEntry, duration: MusicSnoozePreset, everywhere: boolean) => Promise<void>;
    onRemoveSnooze: (item: MusicItemListEntry) => Promise<void>;
    onWeight?: (item: MusicItemListEntry, weight: MusicWeight) => Promise<void>;
    onRemove?: (item: MusicItemListEntry) => Promise<void>;
  } = $props();

  const { t } = getLocalization();
  const secondary = $derived(musicItemSecondaryText(item, t("music.builder.noArtist"), t("music.builder.noAlbum")));
  const duration = $derived(formatMusicDuration(item.durationMs));
  const availabilityTone = $derived(musicAvailabilityTone(item.availability));
  const youtubeArtwork = $derived(item.sourceKind === "youtube-video");
  function availabilityLabel(): string {
    if (item.availability === "available") return t("music.builder.available");
    if (item.availability === "missing") return t("music.builder.missing");
    if (item.availability === "unavailable") return t("music.builder.unavailable");
    if (item.availability === "ambiguous") return t("music.builder.ambiguous");
    return t("music.builder.unknownAvailability");
  }

</script>

<div
  class={cn("music-builder-row group", playing && "music-builder-row-playing")}
  role="listitem"
  tabindex="-1"
  aria-posinset={position}
  aria-setsize={setSize}
  data-music-focus-key={`item:${item.id}`}
>
  <button
    type="button"
    class={cn("absolute inset-0 z-0 rounded-xl focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring", youtubeArtwork && "music-youtube-play-target")}
    onclick={() => onTogglePlayback(item)}
    disabled={starting}
    aria-label={starting ? t("music.builder.loading") : playing ? t("music.pause") : t("music.play")}
    data-app-tooltip-disabled="true"
  ></button>
  <span class={cn("music-builder-artwork pointer-events-none relative z-1", youtubeArtwork && "music-builder-artwork-youtube")}>
    <MusicArtworkThumbnail {item} {bindings} />
    {#if !youtubeArtwork}<span class:playing class="music-builder-play-overlay">{#if playing}<Pause size={14} fill="currentColor" strokeWidth={1.5} />{:else}<Play size={14} fill="currentColor" strokeWidth={1.5} />{/if}</span>{/if}
  </span>

  <div class="pointer-events-none relative z-1 grid min-w-0 flex-1 grid-cols-[minmax(0,1fr)_minmax(5rem,0.65fr)] items-center gap-3 text-left max-[470px]:grid-cols-1 max-[470px]:gap-0.5">
    <span class="min-w-0">
      <span class="flex min-w-0 items-center gap-1.5">
        <span class="block min-w-0 truncate text-xs font-semibold text-foreground">{item.title}</span>
        {#if item.activeSnoozeCount > 0}<MusicSnoozeButton onRemove={() => onRemoveSnooze(item)} />{/if}
      </span>
      <span class="mt-0.5 block truncate text-[0.68rem] text-muted-foreground">{secondary.primary}</span>
    </span>
    <span class="min-w-0 max-[470px]:hidden">
      <span class="block truncate text-[0.7rem] text-muted-foreground">{secondary.secondary}</span>
      <span class="mt-0.5 flex min-w-0 items-center gap-2 truncate text-[0.62rem] text-muted-foreground/75">
        {#if context === "source"}
          <span>{t(`music.builder.${item.reviewState}`)}</span>
        {:else}
          {#if item.membershipWeight}<span>{t(`music.builder.weight.${item.membershipWeight}`)}</span>{/if}
          {#if item.membershipEnabled === false}<span class="text-warning">{t("music.builder.membershipDisabled")}</span>{/if}
        {/if}
      </span>
    </span>
  </div>

  <div class="pointer-events-none relative z-1 flex shrink-0 items-center gap-1.5">
    {#if youtubeArtwork}<span class="row-status" aria-hidden="true">{#if starting}<LoaderCircle size={16} strokeWidth={3} class="animate-spin motion-reduce:animate-none" />{:else if playing}<Pause size={14} fill="currentColor" strokeWidth={1.5} />{:else}<Play size={14} fill="currentColor" strokeWidth={1.5} />{/if}</span>{/if}
    {#if availabilityTone !== "neutral"}
      <span
        class={cn("row-status", availabilityTone === "danger" ? "row-status-danger" : "row-status-warning")}
        title={availabilityLabel()}
      ><CircleAlert size={12} strokeWidth={1.8} /><span class="sr-only">{availabilityLabel()}</span></span>
    {/if}
    {#if duration}<span class="w-10 text-right text-[0.65rem] tabular-nums text-muted-foreground">{duration}</span>{/if}
    <MusicPlaylistItemMenu {item} {playlistName} {context} {showLocationAction} {onShowLocation} {onSnooze} {onWeight} {onRemove} />
  </div>
</div>

<style>
  .music-builder-row {
    --music-youtube-artwork-width: 3.6667rem;
    display: flex;
    position: relative;
    height: 4rem;
    min-width: 0;
    align-items: center;
    gap: 0.65rem;
    border: 1px solid transparent;
    border-radius: 0.75rem;
    padding: 0.35rem 0.45rem;
    transition: background-color 120ms ease, border-color 120ms ease, transform 120ms ease;
  }
  .music-builder-row:hover { background: color-mix(in srgb, var(--accent) 55%, transparent); }
  .music-youtube-play-target { left: calc(0.45rem + var(--music-youtube-artwork-width) + 0.65rem); }
  .music-builder-row-playing { background: color-mix(in srgb, var(--primary) 7%, transparent); }
  .music-builder-artwork { position: relative; display: grid; height: 2.75rem; width: 2.75rem; flex: none; place-items: center; overflow: hidden; border-radius: 0.65rem; background: linear-gradient(145deg, color-mix(in srgb, var(--primary) 13%, var(--secondary)), var(--secondary)); color: var(--muted-foreground); }
  .music-builder-artwork-youtube { width: var(--music-youtube-artwork-width); }
  .music-builder-play-overlay { position: absolute; inset: 0; display: grid; place-items: center; background: color-mix(in srgb, var(--background) 55%, transparent); color: var(--foreground); opacity: 0; transition: opacity 120ms ease; }
  .music-builder-play-overlay.playing { opacity: 1; }
  .music-builder-row:hover .music-builder-play-overlay, .music-builder-row:focus-within .music-builder-play-overlay { opacity: 1; }
  .row-status { display: inline-flex; color: var(--muted-foreground); }
  .row-status-danger { color: var(--destructive); }
  .row-status-warning { color: color-mix(in srgb, var(--destructive) 65%, var(--foreground)); }
  @media (prefers-reduced-motion: reduce) { .music-builder-row, .music-builder-play-overlay { transition: none; } }
</style>
