<script lang="ts">
  import CloudRain from "@lucide/svelte/icons/cloud-rain";
  import ListMusic from "@lucide/svelte/icons/list-music";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicDestinationState } from "$lib/music/library/controller.svelte";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import type { MusicBuilderDestination } from "$lib/music/builder/routing";
  import { orderMusicPlaylists, systemMusicPlaylistName } from "$lib/music/playlists/system";
  import { getSoundscapeStore } from "$lib/stores/soundscape.svelte";
  import MusicPlaylistIcon from "./MusicPlaylistIcon.svelte";

  import type { SoundscapeFilter } from "$lib/music/builder/view-state";
  import MusicSoundscapeGroupIcon from "$lib/components/music/soundscape/MusicSoundscapeGroupIcon.svelte";

  let {
    destination,
    state,
    playlists,
    soundscapeFilter,
    onSearch,
    onNavigate,
    onCreatePlaylist,
    onManagePlaylists,
    onSoundscapeFilter,
  }: {
    destination: MusicBuilderDestination;
    state: MusicDestinationState;
    playlists: MusicPlaylistSummary[];
    soundscapeFilter: SoundscapeFilter;
    onSearch: (search: string) => void;
    onNavigate: (destination: MusicBuilderDestination) => void;
    onCreatePlaylist: () => void;
    onManagePlaylists: () => void;
    onSoundscapeFilter: (filter: SoundscapeFilter) => void;
  } = $props();

  const { t } = getLocalization();
  const soundscape = getSoundscapeStore();
  const orderedPlaylists = $derived(orderMusicPlaylists(playlists).filter((playlist) => {
    const query = destination.kind === "playlists" ? state.search.trim().toLocaleLowerCase() : "";
    return !query || systemMusicPlaylistName(playlist.id, playlist.name, t).toLocaleLowerCase().includes(query);
  }));
  const activePlaylistId = $derived(destination.kind === "playlist" ? destination.playlistId : null);
</script>

<div class="flex min-h-0 flex-1 flex-col overflow-hidden">
  {#if destination.kind === "playlists" || destination.kind === "playlist"}
    <div class="shrink-0 p-2">
      <div class="field flex h-8 items-center gap-2 px-2.5">
        <Search size={13} class="shrink-0 text-muted-foreground" />
        <input data-builder-context-search value={state.search} oninput={(event) => onSearch(event.currentTarget.value)} type="search" aria-label={destination.kind === "playlist" ? t("music.builder.search") : t("music.builder.searchPlaylists")} placeholder={destination.kind === "playlist" ? t("music.builder.search") : t("music.builder.searchPlaylists")} class="field-bare text-[0.7rem]" />
        {#if state.search}<button type="button" onclick={() => onSearch("")} class="grid h-6 w-6 place-items-center rounded-full text-muted-foreground hover:bg-background/60 hover:text-foreground" aria-label={t("music.builder.clearSearch")}><X size={12} /></button>{/if}
        <button type="button" onclick={onCreatePlaylist} class="grid h-6 w-6 place-items-center rounded-full text-muted-foreground hover:bg-background/60 hover:text-foreground" aria-label={t("music.builder.newPlaylist")} title={t("music.builder.newPlaylist")}><Plus size={13} /></button>
      </div>
    </div>
    <div class="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2" data-music-scrollable="true">
      <button type="button" class:active-row={destination.kind === "playlists"} class="context-row" onclick={() => onNavigate({ kind: "playlists" })}><span class="context-icon"><ListMusic size={14} /></span><span class="min-w-0 flex-1 truncate">{t("music.builder.allPlaylists")}</span><span class="context-count">{playlists.length}</span></button>
      {#each orderedPlaylists as playlist (playlist.id)}
        <button type="button" class:active-row={activePlaylistId === playlist.id} class="context-row" onclick={() => onNavigate({ kind: "playlist", playlistId: playlist.id })}>
          <span class="context-icon"><MusicPlaylistIcon icon={playlist.icon} size={14} /></span><span class="min-w-0 flex-1 truncate">{systemMusicPlaylistName(playlist.id, playlist.name, t)}</span><span class="context-count">{playlist.totalCount}</span>
        </button>
      {/each}
    </div>
    <div class="shrink-0 p-2 pt-0"><button type="button" onclick={onManagePlaylists} class="flex h-8 w-full items-center justify-center gap-1.5 rounded-lg bg-secondary text-[0.68rem] font-medium hover:bg-accent"><Pencil size={12} />{t("music.builder.managePlaylists")}</button></div>
  {:else if destination.kind === "soundscapes"}
    <div class="shrink-0 p-2"><h2 class="px-1 text-[0.68rem] font-semibold text-muted-foreground">{t("music.builder.soundscapes")}</h2></div>
    <div class="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2">
      <button type="button" class:active-row={soundscapeFilter === "all"} class="context-row" onclick={() => onSoundscapeFilter("all")}><span class="context-icon"><CloudRain size={14} /></span><span class="min-w-0 flex-1 truncate">{t("music.soundscape.all")}</span><span class="context-count">{soundscape.definitions.length}</span></button>
      <button type="button" class:active-row={soundscapeFilter === "generated"} class="context-row" onclick={() => onSoundscapeFilter("generated")}><span class="context-icon"><CloudRain size={14} /></span><span class="min-w-0 flex-1 truncate">{t("music.soundscape.generated")}</span><span class="context-count">{soundscape.definitions.filter((entry) => entry.sourceKind === "generated-noise").length}</span></button>
      <button type="button" class:active-row={soundscapeFilter === "local"} class="context-row" onclick={() => onSoundscapeFilter("local")}><span class="context-icon"><ListMusic size={14} /></span><span class="min-w-0 flex-1 truncate">{t("music.soundscape.localLoops")}</span><span class="context-count">{soundscape.definitions.filter((entry) => entry.sourceKind === "local-loop").length}</span></button>
      {#each soundscape.groups as group (group.id)}
        <button type="button" class:active-row={soundscapeFilter === `group:${group.id}`} class="context-row pl-5" onclick={() => onSoundscapeFilter(`group:${group.id}`)}><span class="context-icon"><MusicSoundscapeGroupIcon icon={group.icon} size={14} /></span><span class="min-w-0 flex-1 truncate">{group.name}</span><span class="context-count">{soundscape.definitions.filter((entry) => entry.groupId === group.id).length}</span></button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .context-row { display: flex; height: 2rem; width: 100%; min-width: 0; align-items: center; gap: 0.45rem; border-radius: 0.55rem; padding-inline: 0.45rem; color: var(--foreground); font-size: calc(0.68rem * var(--type-scale)); text-align: left; }
  .context-row:hover { background: color-mix(in srgb, var(--accent) 48%, transparent); }
  .active-row { background: color-mix(in srgb, var(--primary) 10%, transparent); }
  .context-icon { display: grid; height: 1.5rem; width: 1.5rem; flex: none; place-items: center; color: var(--muted-foreground); }
  .context-count { flex: none; color: var(--muted-foreground); font-size: calc(0.58rem * var(--type-scale)); font-variant-numeric: tabular-nums; }
</style>
