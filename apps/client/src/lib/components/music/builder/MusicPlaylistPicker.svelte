<script lang="ts">
  import Search from "@lucide/svelte/icons/search";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { sortReviewPlaylists } from "$lib/music/review";
  import { partitionMusicPlaylists, systemMusicPlaylistName } from "$lib/music/playlists/system";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import { cn } from "$lib/utils";
  import MusicPlaylistIcon from "./MusicPlaylistIcon.svelte";

  let {
    playlists,
    checkedIds,
    mixedIds = new Set<string>(),
    mixedCounts = {},
    selectionSize = 0,
    search = "",
    onSearch = () => undefined,
    showSearch = false,
    showSections = false,
    onToggle,
    disabled = false,
    errors = {},
    onSearchInput = () => undefined,
  }: {
    playlists: MusicPlaylistSummary[];
    checkedIds: Set<string>;
    mixedIds?: Set<string>;
    mixedCounts?: Record<string, number>;
    selectionSize?: number;
    search?: string;
    onSearch?: (value: string) => void;
    showSearch?: boolean;
    showSections?: boolean;
    onToggle: (playlist: MusicPlaylistSummary) => void;
    disabled?: boolean;
    errors?: Record<string, string>;
    onSearchInput?: (element: HTMLInputElement | null) => void;
  } = $props();

  const { t } = getLocalization();
  const visible = $derived(sortReviewPlaylists(playlists, "").filter((playlist) => {
    const query = search.trim().toLocaleLowerCase();
    return !query || systemMusicPlaylistName(playlist.id, playlist.name, t).toLocaleLowerCase().includes(query);
  }));
  const sections = $derived(partitionMusicPlaylists(visible));
  const displayedSections = $derived(showSections
    ? [
        { title: t("music.builder.defaultPlaylists"), playlists: sections.defaults },
        { title: t("music.builder.customPlaylists"), playlists: sections.custom },
      ]
    : [{ title: null, playlists: visible }]);

  function searchInputAction(node: HTMLInputElement): { destroy: () => void } {
    onSearchInput(node);
    return { destroy: () => onSearchInput(null) };
  }
</script>

<div class="flex min-h-0 flex-1 flex-col">
  {#if showSearch}
    <label class="field mx-3 mt-3 flex h-9 shrink-0 items-center gap-2 px-2.5">
      <Search size={14} class="text-muted-foreground" />
      <input use:searchInputAction value={search} oninput={(event) => onSearch(event.currentTarget.value)} aria-label={t("music.builder.searchPlaylists")} class="field-bare text-xs" placeholder={t("music.builder.searchPlaylists")} />
    </label>
  {/if}
  <div class="min-h-0 flex-1 overflow-y-auto p-3" data-music-scrollable="true">
    {#if visible.length === 0}
      <p class="p-4 text-center text-xs text-muted-foreground">{t("music.builder.noPlaylistMatches")}</p>
    {:else}
    {#each displayedSections as section (section.title ?? "all")}
      {#if section.playlists.length > 0}
        {#if section.title}<h3 class="mb-2 mt-1 text-[0.7rem] font-semibold text-muted-foreground">{section.title}</h3>{/if}
        <div class="playlist-grid mb-4 grid gap-2">
    {#each section.playlists as playlist (playlist.id)}
      {@const checked = checkedIds.has(playlist.id)}
      {@const mixed = mixedIds.has(playlist.id)}
      {@const playlistName = systemMusicPlaylistName(playlist.id, playlist.name, t)}
      {@const errorId = errors[playlist.id] ? `music-playlist-membership-error-${playlist.id}` : undefined}
      <label class={cn("playlist-card flex min-w-0 flex-wrap items-center gap-2 rounded-lg px-3 py-2.5 transition-colors", disabled ? "cursor-default" : "cursor-pointer", checked ? "bg-primary/10" : mixed ? "bg-secondary/55" : "bg-secondary/35")}>
        <span class="flex min-w-0 flex-1 items-center gap-3 text-left">
          <span class="grid h-8 w-8 shrink-0 place-items-center text-foreground">
            <MusicPlaylistIcon icon={playlist.icon} size={16} />
          </span>
          <span class="min-w-0 flex-1"><strong class="block truncate text-xs font-medium">{playlistName}</strong><span class="block text-[0.62rem] tabular-nums text-muted-foreground">{mixed && selectionSize > 0 ? t("music.builder.bulkExistingMembership", mixedCounts[playlist.id] ?? 0, selectionSize) : t("music.tracks", playlist.totalCount)}</span></span>
        </span>
        <span class="grid h-6 w-6 shrink-0 place-items-center">
          <Checkbox data-review-playlist-id={playlist.id} {checked} indeterminate={mixed && !checked} {disabled} onChange={() => onToggle(playlist)} label={checked ? t("music.builder.removeFromPlaylist", playlist.name) : t("music.builder.addToPlaylist", playlist.name)} aria-describedby={errorId} />
        </span>
      </label>
      {#if errors[playlist.id]}<div id={errorId}><Notice class="mb-1 px-2 text-[0.62rem]" message={errors[playlist.id]} /></div>{/if}
    {/each}
        </div>
      {/if}
    {/each}
    {/if}
  </div>
</div>

<style>
  .playlist-grid { grid-template-columns: repeat(auto-fit, minmax(min(12rem, 100%), 1fr)); }
  .playlist-card { align-content: center; }
</style>
