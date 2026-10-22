<script lang="ts">
  import { onMount, tick } from "svelte";
  import AlertCircle from "@lucide/svelte/icons/circle-alert";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ListMusic from "@lucide/svelte/icons/list-music";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Search from "@lucide/svelte/icons/search";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import {
    getLocalRootBindings,
    getMusicPlaylistPlaybackEntries,
  } from "$lib/api/music-library";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import { projectMusicPlaylistPlayback } from "$lib/music/playlists/playback";
  import { getMusicPlaylistSummaryCache } from "$lib/music/playlists/summary-cache.svelte";
  import { orderMusicPlaylists, systemMusicPlaylistName } from "$lib/music/playlists/system";
  import { getMusicPlayer } from "$lib/stores/music-player.svelte";
  import { requireActiveVaultIdentity } from "$lib/vault/active-vault";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    pickSelectPopoverGeometry,
    type SelectPopoverGeometry,
  } from "$lib/utils/select-popover-position";
  import MusicPlaylistIcon from "$lib/components/music/builder/MusicPlaylistIcon.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { OPENING_MAY_FOCUS_TEXT } from "$lib/mobile/keyboard";

  const PLAYLIST_POPOVER_WIDTH_PX = FLOATING_WIDTH.lg;
  const PLAYLIST_POPOVER_MAX_HEIGHT_PX = 520;

  let {
    onOpenBuilder,
    onOpenIssues,
    active = true,
    mobile = false,
  }: {
    onOpenBuilder: () => void;
    onOpenIssues: () => void;
    active?: boolean;
    mobile?: boolean;
  } = $props();

  const { t } = getLocalization();
  const player = getMusicPlayer();
  const playlistCache = getMusicPlaylistSummaryCache();
  let root = $state<HTMLElement | null>(null);
  let trigger = $state<HTMLButtonElement | null>(null);
  let popover = $state<HTMLDivElement | null>(null);
  let searchInput = $state<HTMLInputElement | null>(null);
  let geometry = $state<SelectPopoverGeometry | null>(null);
  let open = $state(false);
  let search = $state("");
  let opening = $state(false);
  let playingId = $state<string | null>(null);
  let error = $state<string | null>(null);
  let noEligiblePlaylist = $state<MusicPlaylistSummary | null>(null);

  const playlists = $derived(playlistCache.playlists);
  const activePlaylist = $derived(playlists.find((playlist) => playlist.id === player.activePlaylistId) ?? null);
  const matching = $derived.by(() => {
    const query = search.trim().toLocaleLowerCase();
    return orderMusicPlaylists(playlists).filter((playlist) => !query
      || systemMusicPlaylistName(playlist.id, playlist.name, t).toLocaleLowerCase().includes(query));
  });

  $effect(() => {
    if (!active && open) close();
  });

  onMount(() => {
    try {
      playlistCache.setVault(requireActiveVaultIdentity());
      void playlistCache.load();
    } catch {
      // The active vault can publish after this panel mounts; startup preload will connect it.
    }
    const handlePointer = (event: PointerEvent) => {
      if (
        active && open
        && event.target instanceof Node
        && !root?.contains(event.target)
        && !popover?.contains(event.target)
      ) close();
    };
    const handleKey = (event: KeyboardEvent) => {
      if (active && open && event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        close();
        trigger?.focus();
      }
    };
    const handleResize = () => positionPopover();
    window.addEventListener("pointerdown", handlePointer);
    window.addEventListener("keydown", handleKey, true);
    window.addEventListener("resize", handleResize);
    return () => {
      window.removeEventListener("pointerdown", handlePointer);
      window.removeEventListener("keydown", handleKey, true);
      window.removeEventListener("resize", handleResize);
    };
  });

  async function toggle(): Promise<void> {
    if (open) {
      close();
      return;
    }
    if (opening) return;
    opening = true;
    error = null;
    noEligiblePlaylist = null;
    try {
      const loaded = playlistCache.loaded
        ? await playlistCache.refresh()
        : await playlistCache.load();
      if (!loaded) error = playlistCache.error;
      open = true;
      geometry = null;
      await tick();
      positionPopover();
      if (OPENING_MAY_FOCUS_TEXT) searchInput?.focus();
    } finally {
      opening = false;
    }
  }

  function close(): void {
    open = false;
    search = "";
  }

  function handleFocusOut(event: FocusEvent): void {
    if (
      !open
      || !(event.relatedTarget instanceof Node)
      || root?.contains(event.relatedTarget)
      || popover?.contains(event.relatedTarget)
    ) return;
    close();
  }

  function positionPopover(): void {
    if (!open || !trigger) return;
    const triggerRect = trigger.getBoundingClientRect();
    geometry = pickSelectPopoverGeometry({
      triggerRect,
      boundaryRect: {
        top: 0,
        left: 0,
        right: window.innerWidth,
        bottom: window.innerHeight,
        width: window.innerWidth,
        height: window.innerHeight,
      },
      contentHeight: Math.min(popover?.scrollHeight ?? PLAYLIST_POPOVER_MAX_HEIGHT_PX, PLAYLIST_POPOVER_MAX_HEIGHT_PX),
      contentWidth: PLAYLIST_POPOVER_WIDTH_PX,
      horizontalAlign: "start",
    });
  }

  function popoverStyle(): string {
    if (!geometry) return "visibility:hidden;top:0;left:0";
    const maxHeight = Math.min(geometry.maxHeight, PLAYLIST_POPOVER_MAX_HEIGHT_PX);
    return `top:${geometry.top}px;left:${geometry.left}px;width:${geometry.width ?? PLAYLIST_POPOVER_WIDTH_PX}px;max-width:${geometry.maxWidth}px;max-height:${maxHeight}px`;
  }

  async function refresh(): Promise<void> {
    error = null;
    if (!await playlistCache.refresh()) error = playlistCache.error;
  }

  async function play(playlist: MusicPlaylistSummary): Promise<void> {
    if (playingId || playlist.totalCount === 0) return;
    playingId = playlist.id;
    error = null;
    noEligiblePlaylist = null;
    try {
      const entries = await getMusicPlaylistPlaybackEntries(playlist.id, Date.now());
      const rootIds = [...new Set(entries.flatMap((entry) => entry.rootId ? [entry.rootId] : []))];
      const bindings = rootIds.length > 0
        ? await getLocalRootBindings(requireActiveVaultIdentity(), rootIds)
        : [];
      const projection = projectMusicPlaylistPlayback(entries, bindings, {
        nowMs: Date.now(),
        online: player.online,
      });
      if (entries.length === 0) return;
      const loaded = await player.loadSavedPlaylist(
        playlist.id,
        systemMusicPlaylistName(playlist.id, playlist.name, t),
        projection.entries,
        playlist.shuffleEnabled,
        playlist.repeatMode,
        playlist.mixEnabled,
        { structuralSkipped: projection.structuralSkipped },
      );
      if (loaded) close();
      else noEligiblePlaylist = playlist;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      playingId = null;
    }
  }

  function openBuilder(): void {
    close();
    onOpenBuilder();
  }

  function openIssues(): void {
    close();
    onOpenIssues();
  }

</script>

<div bind:this={root} class="relative z-20 min-w-0" onfocusout={handleFocusOut}>
  <button
    bind:this={trigger}
    type="button"
    onclick={() => { void toggle(); }}
    class={cn(
      "flex max-w-56 min-w-0 items-center gap-1.5 rounded-md bg-secondary text-[0.8rem] font-medium text-secondary-foreground transition-colors hover:bg-accent hover:text-accent-foreground",
      mobile ? "h-9 w-9 justify-center px-0" : "h-7 px-2.5",
      open && "bg-accent text-accent-foreground",
    )}
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-busy={opening}
    aria-label={t("music.launcher.choosePlaylist")}
    data-music-playlist-launcher
  >
    {#if activePlaylist}<span class="grid size-3.5 shrink-0 place-items-center"><MusicPlaylistIcon icon={activePlaylist.icon} size={14} strokeWidth={1.5} /></span>{:else}<ListMusic size={14} strokeWidth={1.5} class="shrink-0" />{/if}
    <span class="hidden min-w-0 truncate min-[620px]:inline">{player.activePlaylistName ?? t("music.launcher.playlists")}</span>
    <ChevronDown size={12} class="hidden shrink-0 min-[620px]:block" />
  </button>

  {#if open}
    <div
      use:portal
      bind:this={popover}
      role="dialog"
      aria-label={t("music.launcher.choosePlaylist")}
      tabindex="-1"
      onfocusout={handleFocusOut}
      data-app-floating-surface
      style={popoverStyle()}
      class="playlist-launcher-popover surface-floating fixed z-80 flex flex-col overflow-hidden"
    >
      <div class="shrink-0 px-1.5 pt-1.5" data-music-playlist-search>
        <div class="field flex items-center gap-1.5">
          <Search size={13} strokeWidth={1.5} class="shrink-0 text-muted-foreground" />
          <input bind:this={searchInput} bind:value={search} type="search" aria-label={t("music.launcher.search")} placeholder={t("music.launcher.search")} class="field-bare" />
        </div>
      </div>
      <div use:scrollEdgeFadeAction class="surface-floating-body min-h-0 flex-1 overflow-y-auto overscroll-contain" data-music-scrollable="true">
        {#if error}
          <Notice class="px-2 py-2 text-panel-detail" message={error} retryLabel={t("music.launcher.retry")} onRetry={refresh} />
        {:else if matching.length === 0}
          <div class="grid min-h-32 place-items-center px-5 text-center"><div><ListMusic class="mx-auto mb-2 text-muted-foreground" size={20} /><p class="font-medium">{playlists.length === 0 ? t("music.launcher.empty") : t("music.launcher.noMatches")}</p><p class="mt-1 text-panel-detail text-muted-foreground">{playlists.length === 0 ? t("music.launcher.emptyHint") : t("music.launcher.noMatchesHint")}</p></div></div>
        {:else}
          {#each matching as playlist (playlist.id)}
            <button type="button" onclick={() => { void play(playlist); }} disabled={Boolean(playingId)} aria-disabled={playlist.totalCount === 0} class="menu-item">
              <span class="grid size-4 shrink-0 place-items-center"><MusicPlaylistIcon icon={playlist.icon} size={15} /></span>
              <span class="min-w-0 flex-1 truncate font-medium">{systemMusicPlaylistName(playlist.id, playlist.name, t)}</span>
              {#if player.activePlaylistId === playlist.id}<Check size={14} />{/if}
              <span class="shrink-0 text-panel-detail tabular-nums text-muted-foreground">{#if playingId === playlist.id}<LoaderCircle class="animate-spin motion-reduce:animate-none" size={13} />{:else}{playlist.totalCount}{/if}</span>
            </button>
          {/each}
        {/if}
      </div>

      {#if noEligiblePlaylist}
        <div class="flex shrink-0 items-center gap-2 px-3 pb-2 text-panel-detail text-muted-foreground" role="status" data-music-playlist-unavailable>
          <AlertCircle size={13} class="shrink-0" />
          <span class="min-w-0 flex-1 truncate">{t("music.launcher.unavailable", systemMusicPlaylistName(noEligiblePlaylist.id, noEligiblePlaylist.name, t))}</span>
          <button type="button" onclick={openIssues} class="shrink-0 font-medium text-primary hover:underline">{t("music.launcher.review")}</button>
        </div>
      {/if}

      <div class="shrink-0 border-t border-border p-1.5">
        <button type="button" onclick={openBuilder} class="flex h-8 w-full items-center justify-center gap-1.5 rounded-floating-item bg-secondary font-medium hover:bg-accent"><Settings2 size={13} />{t("music.launcher.openBuilder")}</button>
      </div>
    </div>
  {/if}
</div>
