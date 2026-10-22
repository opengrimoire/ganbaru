<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Check from "@lucide/svelte/icons/check";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Clock3 from "@lucide/svelte/icons/clock-3";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import Info from "@lucide/svelte/icons/info";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import MoreHorizontal from "@lucide/svelte/icons/ellipsis";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Trash2 from "@lucide/svelte/icons/trash";
  import type { Component } from "svelte";
  import { getMusicInspectorDetail } from "$lib/api/music-library";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MusicItemListEntry, MusicSnooze, MusicWeight } from "$lib/music/library/contracts";
  import { formatMusicDuration } from "$lib/music/builder/presentation";
  import { projectMusicItemMenuLayout } from "$lib/music/builder/item-menu-layout";
  import { musicSnoozePreset, type MusicSnoozePreset } from "$lib/music/session/snooze";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  type Subpanel = "details" | "snooze" | "weight";

  let {
    item,
    playlistName = "",
    context = "playlist",
    showLocationAction = true,
    onShowLocation,
    onSnooze,
    onWeight,
    onRemove,
  }: {
    item: MusicItemListEntry;
    playlistName?: string;
    context?: "playlist" | "source";
    showLocationAction?: boolean;
    onShowLocation: (item: MusicItemListEntry) => Promise<void>;
    onSnooze: (item: MusicItemListEntry, duration: MusicSnoozePreset, everywhere: boolean) => Promise<void>;
    onWeight?: (item: MusicItemListEntry, weight: MusicWeight) => Promise<void>;
    onRemove?: (item: MusicItemListEntry) => Promise<void>;
  } = $props();

  const { t } = getLocalization();
  const snoozePresets = ["day", "week", "month"] as const;
  let trigger = $state<HTMLButtonElement | null>(null);
  let panel = $state<HTMLElement | null>(null);
  let open = $state(false);
  let subpanel = $state<Subpanel | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let snoozes = $state<MusicSnooze[]>([]);
  let snoozePlaylistId = $state<string | null>(null);
  let snoozesLoading = $state(false);
  let panelLeft = $state(0);
  let panelTop = $state(0);
  const duration = $derived(formatMusicDuration(item.durationMs));
  const selectedSnooze = $derived.by(() => {
    const now = Date.now();
    const scoped = snoozes.filter((entry) => entry.startsAtMs <= now && (entry.endsAtMs === null || entry.endsAtMs > now)
      && (context === "source" ? entry.scope === "all-playlists" : entry.scope === "playlist" && entry.playlistId === snoozePlaylistId));
    return scoped.length === 1
      ? musicSnoozePreset(scoped[0].startsAtMs, scoped[0].endsAtMs, Intl.DateTimeFormat().resolvedOptions().timeZone)
      : null;
  });

  $effect(() => {
    if (!open) return;
    const closeForLayoutChange = (): void => close(false);
    window.addEventListener("resize", closeForLayoutChange);
    window.addEventListener("scroll", closeForLayoutChange, true);
    return () => {
      window.removeEventListener("resize", closeForLayoutChange);
      window.removeEventListener("scroll", closeForLayoutChange, true);
    };
  });

  function toggle(event: MouseEvent): void {
    event.stopPropagation();
    if (open) { close(); return; }
    if (!trigger) return;
    const layout = projectMusicItemMenuLayout({
      anchor: trigger.getBoundingClientRect(),
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
      panelWidth: FLOATING_WIDTH.sm,
      panelHeight: 250,
    });
    panelLeft = layout.panelLeft;
    panelTop = layout.panelTop;
    error = null;
    subpanel = null;
    open = true;
    queueMicrotask(() => panel?.focus());
  }

  function close(restoreFocus = true): void {
    open = false;
    subpanel = null;
    error = null;
    if (restoreFocus && trigger?.isConnected) queueMicrotask(() => trigger?.focus());
  }

  function showSubpanel(next: Subpanel): void {
    subpanel = next;
    error = null;
    if (next === "snooze") void loadSnoozes();
    queueMicrotask(() => panel?.focus());
  }

  async function loadSnoozes(): Promise<void> {
    snoozesLoading = true;
    try {
      const detail = await getMusicInspectorDetail(item.id);
      if (open && subpanel === "snooze") {
        snoozes = detail.snoozes;
        snoozePlaylistId = detail.memberships.find((entry) => entry.id === item.membershipId)?.playlistId ?? null;
      }
    } catch (cause) {
      if (open && subpanel === "snooze") error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      snoozesLoading = false;
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || !open) return;
    event.preventDefault();
    event.stopPropagation();
    if (subpanel) { subpanel = null; return; }
    close();
  }

  async function run(action: () => Promise<void>, closeAfter = true): Promise<void> {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await action();
      if (closeAfter) close(false);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      busy = false;
    }
  }

  function weightLabel(weight: MusicWeight | null): string {
    return t(`music.builder.weight.${weight ?? "normal"}`);
  }

  function subpanelTitle(value: Subpanel): string {
    if (value === "details") return t("music.builder.details");
    if (value === "snooze") return t("music.builder.snoozeTrack");
    return t("music.builder.frequency");
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="pointer-events-auto relative z-10">
  <button bind:this={trigger} type="button" class="menu-trigger" aria-label={t("music.builder.trackActions", item.title)} aria-expanded={open} aria-haspopup="dialog" onclick={toggle}><MoreHorizontal size={15} strokeWidth={1.7} /></button>

  {#if open}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div use:portal class="fixed inset-0 z-70" onclick={() => close(false)}></div>
    <div
      bind:this={panel}
      use:portal
      class="surface-floating fixed z-80 flex w-floating-sm flex-col overflow-hidden outline-none"
      style={`left:${panelLeft}px;top:${panelTop}px;max-height:calc(100vh - ${panelTop + 6}px)`}
      role="dialog"
      tabindex="-1"
      aria-label={subpanel ? subpanelTitle(subpanel) : t("music.builder.trackActions", item.title)}
      data-app-shortcuts="ignore"
    >
      {#if subpanel}
        <div class="flex shrink-0 items-center gap-1.5 border-b border-border p-1.5">
          <button type="button" class="back-button" aria-label={t("music.builder.back")} onclick={() => subpanel = null}><ArrowLeft size={14} /></button>
          <span class="min-w-0">
            <strong class="block font-semibold">{subpanelTitle(subpanel)}</strong>
            <span class="block truncate text-panel-detail text-muted-foreground">{item.title}</span>
          </span>
        </div>
      {:else}
        <div class="shrink-0 border-b border-border px-2.5 py-2">
          <strong class="block truncate font-semibold">{item.title}</strong>
          <span class="mt-0.5 block truncate text-panel-detail text-muted-foreground">{item.artist || t("music.builder.noArtist")}</span>
        </div>
      {/if}

      <div use:scrollEdgeFadeAction class="surface-floating-body grid min-h-0 overflow-y-auto">
        {#if subpanel === "details"}
          <dl class="grid gap-0.5">
            {@render metadataRow(item.sourceKind === "local-file" ? t("music.builder.artistOnly") : t("music.builder.artist"), item.artist || t("music.builder.noArtist"))}
            {@render metadataRow(t("music.builder.album"), item.album || t("music.builder.noAlbum"))}
            {#if duration}{@render metadataRow(t("music.builder.duration"), duration)}{/if}
            {@render metadataRow(t("music.builder.sources"), item.sourceKind === "local-file" ? t("music.builder.local") : t("music.builder.youtube"))}
          </dl>
        {:else if subpanel === "snooze"}
          {#each snoozePresets as snoozeDuration (snoozeDuration)}
            <button type="button" class="option-row menu-item justify-between" disabled={snoozesLoading} aria-pressed={selectedSnooze === snoozeDuration} onclick={() => { void run(() => onSnooze(item, snoozeDuration, context === "source")); }}><span>{t(`music.preferences.${snoozeDuration}`)}</span>{#if selectedSnooze === snoozeDuration}<Check size={12} />{/if}</button>
          {/each}
        {:else if subpanel === "weight"}
          {#each ["rarely", "less-often", "normal", "more-often", "much-more-often"] as weight (weight)}
            {@const typedWeight = weight as MusicWeight}
            <button type="button" class="option-row menu-item justify-between" onclick={() => { if (onWeight) void run(() => onWeight(item, typedWeight)); }}><span>{weightLabel(typedWeight)}</span>{#if item.membershipWeight === typedWeight}<Check size={12} />{/if}</button>
          {/each}
        {:else}
          {@render panelRow(t("music.builder.details"), item.album || t("music.builder.noAlbum"), Info, "details")}
          {@render panelRow(t("music.builder.snoozeTrack"), item.activeSnoozeCount > 0 ? t("music.builder.snoozed") : "", Clock3, "snooze")}
          {#if context === "playlist"}{@render panelRow(t("music.builder.frequency"), weightLabel(item.membershipWeight), SlidersHorizontal, "weight")}{/if}
          {#if item.sourceKind === "local-file" && showLocationAction}
            <button type="button" class="menu-item" disabled={busy} onclick={() => { void run(() => onShowLocation(item)); }}><FolderSearch size={14} />{t("music.itemMenu.showLocation")}</button>
          {/if}
          {#if context === "playlist" && onRemove}
            <div role="separator" class="menu-separator"></div>
            <button type="button" class="menu-item menu-item-destructive" disabled={busy} onclick={() => { void run(() => onRemove(item)); }}><Trash2 size={14} />{t("music.builder.removeFromPlaylist", playlistName)}</button>
          {/if}
        {/if}
      </div>

      {#if busy}<div class="absolute inset-0 grid place-items-center rounded-floating bg-popover/75"><LoaderCircle class="animate-spin motion-reduce:animate-none" size={16} /></div>{/if}
      {#if error}<Notice class="mx-1.5 mb-1.5 shrink-0 px-2 py-1.5 text-panel-detail" message={error} />{/if}
    </div>
  {/if}
</div>

{#snippet panelRow(label: string, value: string, Icon: Component, target: Subpanel)}
  <button type="button" class="panel-row menu-item" onclick={() => showSubpanel(target)}>
    <Icon size={14} class="text-muted-foreground" />
    <span class="min-w-0 flex-1 truncate">{label}</span>
    <span class="max-w-20 truncate text-panel-detail text-muted-foreground">{value}</span>
    <ChevronRight size={12} class="text-muted-foreground" />
  </button>
{/snippet}

{#snippet metadataRow(label: string, value: string)}
  <div class="grid grid-cols-[4rem_minmax(0,1fr)] gap-2 px-2 py-1.5"><dt class="text-muted-foreground">{label}</dt><dd class="truncate text-right" title={value}>{value}</dd></div>
{/snippet}

<style>
  .menu-trigger { display: grid; height: 1.75rem; width: 1.75rem; place-items: center; border-radius: 0.375rem; color: var(--muted-foreground); opacity: 0.7; }
  .menu-trigger:hover, .menu-trigger:focus-visible, .menu-trigger[aria-expanded="true"] { background: var(--accent); color: var(--accent-foreground); opacity: 1; outline: none; }
  .back-button { display: grid; height: 1.75rem; width: 1.75rem; flex: none; place-items: center; border-radius: var(--floating-item-radius); color: var(--muted-foreground); }
  .back-button:hover, .back-button:focus-visible { background: var(--accent); color: var(--accent-foreground); outline: none; }
</style>
