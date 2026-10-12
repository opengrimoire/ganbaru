<script lang="ts">
  import Plus from "@lucide/svelte/icons/plus";
  import Download from "@lucide/svelte/icons/download";
  import Upload from "@lucide/svelte/icons/upload";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import type { MusicBuilderDestination } from "$lib/music/builder/routing";
  import { orderMusicPlaylists, systemMusicPlaylistName } from "$lib/music/playlists/system";
  import MusicBuilderAsyncState from "./MusicBuilderAsyncState.svelte";
  import MusicPlaylistIcon from "./MusicPlaylistIcon.svelte";
  import MusicSoundscapeBuilder from "$lib/components/music/soundscape/MusicSoundscapeBuilder.svelte";

  let {
    destination,
    search = "",
    playlists,
    onNavigate,
    onPrimary = () => undefined,
    onImport = () => undefined,
    onExport = () => undefined,
    compact = false,
  }: {
    destination: MusicBuilderDestination;
    search?: string;
    playlists: MusicPlaylistSummary[];
    onNavigate: (destination: MusicBuilderDestination) => void;
    onPrimary?: () => void;
    onImport?: () => void;
    onExport?: () => void;
    compact?: boolean;
  } = $props();

  const { t } = getLocalization();
  const visiblePlaylists = $derived(orderMusicPlaylists(playlists.filter((playlist) => {
    const query = search.trim().toLocaleLowerCase();
    return !query || systemMusicPlaylistName(playlist.id, playlist.name, t).toLocaleLowerCase().includes(query);
  })));

</script>

<div class="overview-scroll h-full min-h-0 overflow-y-auto overscroll-contain p-3" data-music-scrollable="true">
  {#if destination.kind === "playlists"}
    {#if !compact}<div class="mb-3 flex flex-wrap items-center justify-end gap-2"><button type="button" onclick={onImport} class="inline-flex h-8 items-center gap-1.5 rounded-lg bg-secondary px-3 text-xs font-medium"><Upload size={13} />{t("music.builder.importPlaylists")}</button><button type="button" onclick={onExport} disabled={playlists.length === 0} class="inline-flex h-8 items-center gap-1.5 rounded-lg bg-secondary px-3 text-xs font-medium disabled:opacity-40"><Download size={13} />{t("music.builder.exportPlaylists")}</button></div>{/if}
    {#if playlists.length === 0}
      <MusicBuilderAsyncState kind="empty" title={t("music.builder.emptyPlaylistsTitle")} description={t("music.builder.emptyPlaylistsDescription")} actionLabel={t("music.builder.newPlaylist")} onAction={onPrimary} />
    {:else if visiblePlaylists.length === 0}
      <MusicBuilderAsyncState kind="empty" title={t("music.builder.noPlaylistFilterResults")} description={t("music.builder.adjustPlaylistFilters")} />
    {:else}
      <div class="grid grid-cols-[repeat(auto-fill,minmax(min(14rem,100%),1fr))] gap-2.5">
        {#each visiblePlaylists as playlist (playlist.id)}
          <button type="button" class="overview-card" onclick={() => onNavigate({ kind: "playlist", playlistId: playlist.id })}>
            <span class="overview-icon"><MusicPlaylistIcon icon={playlist.icon} size={18} strokeWidth={1.45} /></span>
            <span class="min-w-0 flex-1 text-left">
              <strong class="block truncate text-xs font-semibold text-foreground">{systemMusicPlaylistName(playlist.id, playlist.name, t)}</strong>
              <span class="mt-1.5 block text-[0.62rem] text-muted-foreground">{t("music.tracks", playlist.totalCount)}</span>
            </span>
          </button>
        {/each}
      </div>
      {#if !compact}<button type="button" class="overview-card overview-add mt-2.5 w-full" onclick={onPrimary}><span class="overview-icon"><Plus size={18} /></span><span class="text-xs font-semibold">{t("music.builder.newPlaylist")}</span></button>{/if}
    {/if}
  {:else if destination.kind === "soundscapes"}
    <MusicSoundscapeBuilder />
  {:else}
    <MusicBuilderAsyncState kind="empty" title={t("music.builder.emptyLibraryTitle")} description={t("music.builder.emptyLibraryDescription")} actionLabel={t("music.builder.addMusic")} onAction={onPrimary} />
  {/if}
</div>

<style>
  .overview-scroll { scrollbar-width: thin; scrollbar-color: color-mix(in srgb, var(--foreground) 18%, transparent) transparent; }
  .overview-card { display: flex; min-width: 0; align-items: flex-start; gap: 0.75rem; overflow: hidden; border: 1px solid color-mix(in srgb, var(--border) 62%, transparent); border-radius: 0.85rem; background: color-mix(in srgb, var(--card) 75%, transparent); padding: 0.75rem; color: var(--foreground); transition: background-color 100ms ease; }
  button.overview-card:hover { background: var(--accent); }
  .overview-icon { display: grid; height: 2.35rem; width: 2rem; flex: none; place-items: center; color: var(--foreground); }
  .overview-add { min-height: 5.5rem; align-items: center; justify-content: center; border-style: dashed; color: var(--muted-foreground); }
  @media (prefers-reduced-motion: reduce) { .overview-card { transition: none; } }
</style>
