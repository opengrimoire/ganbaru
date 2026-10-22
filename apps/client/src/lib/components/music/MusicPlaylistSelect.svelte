<script lang="ts">
  import { tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import AlertTriangle from "@lucide/svelte/icons/triangle-alert";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ListMusic from "@lucide/svelte/icons/list-music";
  import Search from "@lucide/svelte/icons/search";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
  import { orderMusicPlaylists, systemMusicPlaylistName } from "$lib/music/playlists/system";
  import { OPENING_MAY_FOCUS_TEXT } from "$lib/mobile/keyboard";
  import { formatNumber } from "$lib/i18n/formatters";
  import MusicPlaylistIcon from "$lib/components/music/builder/MusicPlaylistIcon.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    pickSelectPopoverGeometry,
    type SelectPopoverGeometry,
  } from "$lib/utils/select-popover-position";

  let {
    value,
    playlists,
    onChange,
    label,
    disabled = false,
    loading = false,
    emptyLabel,
    class: className = "",
    triggerClass = "",
  }: {
    value: string | null;
    playlists: readonly MusicPlaylistSummary[];
    onChange: (playlistId: string | null) => void;
    label: string;
    disabled?: boolean;
    loading?: boolean;
    emptyLabel?: string;
    class?: string;
    /** Classes merged into the trigger so a host surface can match its own controls. */
    triggerClass?: string;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const id = $props.id();
  const PLAYLIST_POPOVER_WIDTH_PX = FLOATING_WIDTH.lg;
  const PLAYLIST_POPOVER_MAX_HEIGHT_PX = 520;
  const noneLabel = $derived(emptyLabel ?? t("music.assignment.noPlaylist"));
  let open = $state(false);
  let search = $state("");
  let trigger = $state<HTMLButtonElement | null>(null);
  let popover = $state<HTMLDivElement | null>(null);
  let searchInput = $state<HTMLInputElement | null>(null);
  let geometry = $state<SelectPopoverGeometry | null>(null);

  const selected = $derived(playlists.find((playlist) => playlist.id === value) ?? null);
  const missing = $derived(Boolean(value && !selected && !loading));
  const matching = $derived.by(() => {
    const query = search.trim().toLocaleLowerCase();
    return orderMusicPlaylists(playlists).filter((playlist) => !query
      || systemMusicPlaylistName(playlist.id, playlist.name, t).toLocaleLowerCase().includes(query));
  });

  /** Toggle the playlist chooser without triggering music playback. */
  async function toggle(): Promise<void> {
    if (disabled || loading) return;
    if (open) {
      close();
      return;
    }
    open = true;
    geometry = null;
    await tick();
    if (!open) return;
    position();
    if (OPENING_MAY_FOCUS_TEXT) searchInput?.focus({ preventScroll: true });
  }

  /** Dismiss the chooser and optionally restore focus to its trigger. */
  function close(restoreFocus = false): void {
    open = false;
    search = "";
    if (restoreFocus && trigger?.isConnected) queueMicrotask(() => trigger?.focus({ preventScroll: true }));
  }

  /** Apply a playlist or the explicit empty choice. */
  function choose(playlistId: string | null): void {
    if (disabled || loading) return;
    onChange(playlistId);
    close(true);
  }

  /** Match the main Music chooser's width and keep it within the viewport. */
  function position(): void {
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    geometry = pickSelectPopoverGeometry({
      triggerRect: rect,
      boundaryRect: {
        top: 8,
        left: 8,
        right: window.innerWidth - 8,
        bottom: window.innerHeight - 8,
        width: window.innerWidth - 16,
        height: window.innerHeight - 16,
      },
      contentHeight: Math.min(popover?.scrollHeight ?? PLAYLIST_POPOVER_MAX_HEIGHT_PX, PLAYLIST_POPOVER_MAX_HEIGHT_PX),
      contentWidth: PLAYLIST_POPOVER_WIDTH_PX,
      horizontalAlign: "end",
    });
  }

  /** Build the popover position style; it stays hidden until its content is measured. */
  function popoverStyle(): string {
    if (!geometry) return "visibility:hidden;top:0;left:0";
    return `top:${geometry.top}px;left:${geometry.left}px;width:${geometry.width ?? PLAYLIST_POPOVER_WIDTH_PX}px;max-width:${geometry.maxWidth}px;max-height:${Math.min(geometry.maxHeight, PLAYLIST_POPOVER_MAX_HEIGHT_PX)}px`;
  }

  /** Close after keyboard focus leaves the trigger and its floating menu. */
  function handlePopoverFocusOut(event: FocusEvent): void {
    const next = event.relatedTarget;
    if (!(next instanceof Node) || popover?.contains(next) || trigger?.contains(next)) return;
    close();
  }

  /** Navigate playlist options without letting Escape close the parent settings. */
  function handleKeydown(event: KeyboardEvent): void {
    if (!(event.target instanceof Node) || (!popover?.contains(event.target) && !trigger?.contains(event.target))) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close(true);
      return;
    }
    const options = [...(popover?.querySelectorAll<HTMLButtonElement>('[role="option"]') ?? [])];
    if (event.target === searchInput && event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      options[0]?.click();
      return;
    }
    if (event.target === searchInput && (event.key === "Home" || event.key === "End")) return;
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) || options.length === 0) return;
    event.preventDefault();
    event.stopPropagation();
    const index = options.findIndex((option) => option === document.activeElement);
    const next = event.key === "Home" ? 0 : event.key === "End" ? options.length - 1
      : event.key === "ArrowDown" ? (index + 1) % options.length
      : (index <= 0 ? options.length : index) - 1;
    options[next]?.focus({ preventScroll: true });
  }

  $effect(() => {
    if (!open) return;
    const pointer = (event: PointerEvent) => {
      if (!(event.target instanceof Node)) return;
      if (!trigger?.contains(event.target) && !popover?.contains(event.target)) close();
    };
    const scroll = (event: Event) => {
      if (event.target instanceof Node && !popover?.contains(event.target)) position();
    };
    window.addEventListener("pointerdown", pointer, true);
    window.addEventListener("keydown", handleKeydown, true);
    window.addEventListener("resize", position);
    window.addEventListener("scroll", scroll, true);
    return () => {
      window.removeEventListener("pointerdown", pointer, true);
      window.removeEventListener("keydown", handleKeydown, true);
      window.removeEventListener("resize", position);
      window.removeEventListener("scroll", scroll, true);
    };
  });
</script>

<div class={cn("min-w-0", className)}>
  <button
    bind:this={trigger}
    type="button"
    disabled={disabled || loading}
    onclick={() => { void toggle(); }}
    onkeydown={(event) => {
      if (!open && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
        event.preventDefault();
        void toggle();
      }
    }}
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-controls={open ? id : undefined}
    aria-label={label}
    class={cn(
      "flex h-7 w-full min-w-0 items-center gap-2 rounded-md border border-border bg-card px-2.5 text-left text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:hover:bg-card dark:bg-transparent dark:disabled:hover:bg-transparent",
      triggerClass,
      missing && "text-warning-foreground ring-1 ring-warning/55",
    )}
  >
    {#if missing}<AlertTriangle size={14} class="shrink-0 text-warning" />{:else if value === null}<VolumeX size={14} strokeWidth={1.5} class="shrink-0 text-muted-foreground" />{:else if selected}<span class="grid size-3.5 shrink-0 place-items-center text-muted-foreground"><MusicPlaylistIcon icon={selected.icon} size={14} strokeWidth={1.5} /></span>{:else}<ListMusic size={14} strokeWidth={1.5} class="shrink-0 text-muted-foreground" />{/if}
    <span class="min-w-0 flex-1 truncate">{#if loading}<Loading layout="inline" label={t("music.assignment.loadingPlaylists")} />{:else}{missing ?t("music.assignment.missingPlaylist") : selected ? systemMusicPlaylistName(selected.id, selected.name, t) : noneLabel}{/if}</span>
    <ChevronDown size={13} strokeWidth={2} class={cn("shrink-0 text-muted-foreground transition-transform", open && "rotate-180")} />
  </button>
</div>

{#if open}
  <div
    use:portal={trigger?.closest<HTMLElement>("[data-floating-root]") ?? "body"}
    bind:this={popover}
    role="dialog"
    {id}
    tabindex="-1"
    data-app-floating-surface
    aria-label={label}
    onfocusout={handlePopoverFocusOut}
    style={popoverStyle()}
    class="surface-floating fixed z-80 flex min-h-0 flex-col overflow-hidden"
  >
    <div class="shrink-0 px-1.5 pt-1.5">
      <label class="field flex items-center gap-1.5">
        <Search size={13} strokeWidth={1.5} class="shrink-0 text-muted-foreground" />
        <input bind:this={searchInput} bind:value={search} type="search" aria-label={t("music.assignment.searchPlaylists")} class="field-bare" placeholder={t("music.assignment.searchPlaylists")} />
      </label>
    </div>
    <div use:scrollEdgeFadeAction class="surface-floating-body min-h-0 flex-1 overflow-y-auto overscroll-contain" data-music-scrollable="true">
      <div role="listbox" aria-label={label}>
      {#if !search.trim()}
        <button type="button" role="option" aria-selected={value === null} onclick={() => choose(null)} class="menu-item">
          <VolumeX size={15} strokeWidth={1.75} />
          <span class="min-w-0 flex-1 truncate font-medium">{noneLabel}</span>
          {#if value === null}<Check size={14} />{/if}
        </button>
      {/if}
      {#each matching as playlist (playlist.id)}
        <button type="button" role="option" aria-selected={value === playlist.id} onclick={() => choose(playlist.id)} class="menu-item">
          <span class="grid size-4 shrink-0 place-items-center"><MusicPlaylistIcon icon={playlist.icon} size={15} /></span>
          <span class="min-w-0 flex-1 truncate font-medium">{systemMusicPlaylistName(playlist.id, playlist.name, t)}</span>
          {#if value === playlist.id}<Check size={14} />{/if}
          <span class="shrink-0 text-panel-detail tabular-nums text-muted-foreground">{formatNumber(localization.locale, playlist.totalCount)}</span>
        </button>
      {:else}
        <p class="px-3 py-6 text-center text-muted-foreground">{t("music.assignment.noPlaylistMatches")}</p>
      {/each}
      </div>
    </div>
  </div>
{/if}
