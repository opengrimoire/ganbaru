<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Disc3 from "@lucide/svelte/icons/disc-3";
  import Eye from "@lucide/svelte/icons/eye";
  import ListPlus from "@lucide/svelte/icons/list-plus";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Pause from "@lucide/svelte/icons/pause";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import Play from "@lucide/svelte/icons/play";
  import Slash from "@lucide/svelte/icons/slash";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import X from "@lucide/svelte/icons/x";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatList } from "$lib/i18n/formatters";
  import type { MusicBuilderInspectorController } from "$lib/music/builder/inspector.svelte";
  import type { MusicBulkEditController } from "$lib/music/builder/bulk-edit-controller.svelte";
  import type { MusicLibraryController } from "$lib/music/library/controller.svelte";
  import type { MusicReviewAuditionController } from "$lib/music/review/audition.svelte";
  import type { MusicReviewController } from "$lib/music/review/controller.svelte";
  import type { MusicReviewWorkspaceViewState } from "$lib/music/builder/view-state";
  import type { MusicSourcesController } from "$lib/music/sources/controller.svelte";
  import type { MusicIssue, MusicItemListEntry } from "$lib/music/library/contracts";
  import {
    isMusicReviewEditableTarget,
    musicReviewArtworkDataUrl,
    musicReviewDurationMs,
  } from "$lib/music/review";
  import {
    buildMusicReviewTree,
    firstMusicReviewTreeItemId,
    nextPendingMusicReviewSelectionItemId,
    nextPendingMusicReviewTreeItemId,
    summarizeMusicReviewTreeSelection,
  } from "$lib/music/review/tree";
  import { clampRate, formatPlaybackTime } from "$lib/music/playback";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import MusicPlaylistIcon from "./MusicPlaylistIcon.svelte";
  import MusicPlaylistManager from "./MusicPlaylistManager.svelte";
  import MusicPlaylistPicker from "./MusicPlaylistPicker.svelte";

  let {
    items,
    totalCount,
    library,
    inspector,
    sources,
    audition,
    review,
    bulk,
    active = true,
    selectedItemIds,
    selectedFolderIds,
    onClearSelection,
    autoplay,
    onAutoplayChange,
    onOpenPlayer,
    compactPlayerLabel = false,
    showPanelButton = false,
    onOpenPanel = () => undefined,
    onEditPlaylist,
    onDeletePlaylist,
    onReorderPlaylists,
    issue = null,
    repairAvailable = true,
    onRepairIssue = () => undefined,
    viewState = $bindable(),
  }: {
    items: MusicItemListEntry[];
    totalCount: number;
    library: MusicLibraryController;
    inspector: MusicBuilderInspectorController;
    sources: MusicSourcesController;
    audition: MusicReviewAuditionController;
    review: MusicReviewController;
    bulk: MusicBulkEditController;
    active?: boolean;
    selectedItemIds: string[];
    selectedFolderIds: string[];
    onClearSelection: () => void;
    autoplay: boolean;
    onAutoplayChange: (value: boolean) => void;
    onOpenPlayer: () => void;
    compactPlayerLabel?: boolean;
    showPanelButton?: boolean;
    onOpenPanel?: () => void;
    onEditPlaylist: (playlistId: string) => void;
    onDeletePlaylist: (playlistId: string) => void;
    onReorderPlaylists: (playlistIds: string[]) => Promise<boolean>;
    issue?: MusicIssue | null;
    repairAvailable?: boolean;
    onRepairIssue?: (issue: MusicIssue) => void;
    viewState: MusicReviewWorkspaceViewState;
  } = $props();

  const { t, locale } = getLocalization();
  let surface = $state<HTMLElement | null>(null);
  let lastSelectedId = $state<string | null>(null);
  let lastAutoplayedId = $state<string | null>(null);
  let newPlaylistNameInput = $state<HTMLInputElement | null>(null);
  let checklistRoot = $state<HTMLElement | null>(null);
  let prefetchedArtworkUrls = $state<Record<string, string>>({});
  let prefetchedArtworkReadyIds = $state<Set<string>>(new Set());
  let artworkPrefetchGeneration = 0;
  let preparingNext = $state(false);
  let ignoreConfirmOpen = $state(false);
  let selectionPlaybackHandled = $state(false);
  let reviewStartItemId = $state<string | null>(null);
  interface PlaylistManagerHandle {
    cancelManaging: () => Promise<void>;
    finishManaging: () => Promise<void>;
  }
  let playlistManager = $state<PlaylistManagerHandle | null>(null);
  let playlistManagerActionsDisabled = $state(false);
  const selectedIdSet = $derived(new Set(selectedItemIds));
  const selectedFolderIdSet = $derived(new Set(selectedFolderIds));
  const selectionMode = $derived(selectedItemIds.length > 0);
  const reviewTree = $derived(buildMusicReviewTree(items));
  const selectionSummary = $derived(summarizeMusicReviewTreeSelection(
    reviewTree,
    items,
    selectedIdSet,
    selectedFolderIdSet,
  ));
  const selectionContext = $derived.by(() => {
    const labels = [...selectionSummary.contextLabels];
    if (selectionSummary.hiddenContextCount > 0) {
      labels.push(t("music.builder.reviewSelectionMore", selectionSummary.hiddenContextCount));
    }
    return labels.length > 0 ? formatList(locale, labels) : t("music.builder.reviewSelectionItems");
  });
  const selectionNeedsSave = $derived(bulk.membershipsChanged);
  const selectionMatchesBulk = $derived(selectedItemIds.length === bulk.itemIds.length
    && selectedItemIds.every((itemId, index) => bulk.itemIds[index] === itemId));
  const selectionAllIgnored = $derived(selectionMode && selectedItemIds.every((itemId) =>
    items.find((item) => item.id === itemId)?.reviewState === "ignored"));
  const selectionProjectionReady = $derived(Object.keys(bulk.states).length > 0
    || (selectionMatchesBulk && !bulk.loading));
  const selectionReady = $derived(selectionMode && selectionMatchesBulk
    && !bulk.loading && !bulk.error && !bulk.selectionStale);
  const selectionCanIgnore = $derived(selectionMode && selectionMatchesBulk);
  const sessionSkippedIds = $derived(new Set(viewState.sessionSkippedIds));
  const reviewItemsFullyLoaded = $derived(
    library.currentWindow.items.length >= library.currentWindow.totalCount,
  );
  const reviewItemIds = $derived(reviewTree.flatMap((node) => node.itemIds));
  const initialReviewItemId = $derived(reviewItemsFullyLoaded
    ? firstMusicReviewTreeItemId(items)
    : null);
  const initialReviewItem = $derived(initialReviewItemId
    ? items.find((entry) => entry.id === initialReviewItemId) ?? null
    : null);
  const item = $derived(library.selectedItem ?? initialReviewItem);
  const ignoreActive = $derived(selectionMode ? selectionAllIgnored : item?.reviewState === "ignored");
  const ignoreActionLabel = $derived(ignoreActive ? t("music.builder.stopIgnoring") : t("music.builder.ignore"));
  const detail = $derived(inspector.detail?.item.id === item?.id ? inspector.detail : null);
  const checkedIds = $derived(new Set(detail?.memberships.map((membership) => membership.playlistId) ?? []));
  const membershipSignature = $derived([...checkedIds].sort().join("\n"));
  const membershipsChanged = $derived(Boolean(item)
    && viewState.membershipBaselines[item!.id] !== undefined
    && viewState.membershipBaselines[item!.id] !== membershipSignature);
  const needsSave = $derived(Boolean(item) && membershipsChanged);
  const membershipSaving = $derived(review.membershipBusy.size > 0);
  const reviewTreeIndex = $derived(item ? reviewItemIds.indexOf(item.id) : -1);
  const reviewedCount = $derived(items.filter((entry) => entry.reviewState === "reviewed").length);
  const player = $derived(audition.musicPlayer);
  const previewTitle = $derived(detail
    ? detail.item.titleOverride ?? detail.item.originalTitle
    : item?.title ?? "");
  const previewArtist = $derived(detail
    ? (detail.item.artistOverride ?? detail.item.originalArtist) || t("music.builder.noArtist")
    : item?.artist || t("music.builder.noArtist"));
  const reviewPlayerReady = $derived(audition.ownsPlayback && audition.reviewItemId === item?.id);
  const reviewPlaybackStarting = $derived(Boolean(item && (
    (reviewStartItemId === item.id && !reviewPlayerReady)
    || (reviewPlayerReady && player.youtubePlaybackStarting)
  )));
  const prefetchedArtworkUrl = $derived(item ? prefetchedArtworkUrls[item.id] ?? null : null);
  const previewDurationMs = $derived(musicReviewDurationMs(
    reviewPlayerReady ? player.snapshot.durationMs : null,
    detail?.item.youtubeVideoId ? player.youtubeKnownDurations[detail.item.youtubeVideoId] ?? null : null,
    detail?.item.durationMs ?? item?.durationMs ?? null,
  ));
  const seekSliderProgress = $derived(reviewPlayerReady && player.progressMax > 0
    ? `${Math.min(100, Math.max(0, (player.progressValue / player.progressMax) * 100))}%`
    : "0%");

  $effect(() => {
    const validItemIds = new Set(items.map((item) => item.id));
    const nextIds = selectedItemIds.filter((itemId) => validItemIds.has(itemId));
    const matchesCurrent = nextIds.length === bulk.itemIds.length
      && nextIds.every((itemId, index) => bulk.itemIds[index] === itemId);
    if (matchesCurrent) return;
    if (nextIds.length === 0) {
      bulk.clear();
      return;
    }
    void bulk.open(nextIds, library.playlistSummaries, bulk.itemIds.length > 0);
  });

  $effect(() => {
    if (!selectionMode) {
      selectionPlaybackHandled = false;
      return;
    }
    if (selectionPlaybackHandled) return;
    selectionPlaybackHandled = true;
    if (active && audition.ownsPlayback && player.isPlaying) void player.pausePlayback();
  });

  $effect(() => {
    if (library.loadingMore || library.loadMoreError || library.currentWindow.items.length >= library.currentWindow.totalCount) return;
    void library.loadMore();
  });

  function availabilityLabel(): string {
    if (!detail) return "";
    if (detail.item.availability === "available") return t("music.builder.available");
    if (detail.item.availability === "missing") return t("music.builder.missing");
    if (detail.item.availability === "unavailable") return t("music.builder.unavailable");
    if (detail.item.availability === "ambiguous") return t("music.builder.ambiguous");
    return t("music.builder.unknownAvailability");
  }

  function attentionLabel(): string {
    if (detail?.item.availability !== "available") return availabilityLabel();
    return issue?.message ?? "";
  }

  $effect(() => {
    if (!active || !surface) return;
    return player.claimSurface("playlist-builder-review", surface, 100);
  });

  $effect(() => {
    const nextId = item?.id ?? null;
    if (selectionMode || !nextId || nextId === lastSelectedId) return;
    lastSelectedId = nextId;
    library.selectItem(nextId);
    void inspector.select(nextId);
  });

  $effect(() => {
    if (selectionMode || reviewTreeIndex < 0) return;
    const nearbyIds = reviewItemIds
      .slice(Math.max(0, reviewTreeIndex - 1), reviewTreeIndex + 4)
    const generation = ++artworkPrefetchGeneration;
    const bindings = [...sources.bindings];
    void inspector.prefetch(nearbyIds).then(async (details) => {
      const entries = await Promise.all(details.map(async (entry) => {
        const url = await loadDecodedReviewArtwork(entry, bindings);
        return url ? [entry.item.id, url] as const : null;
      }));
      if (generation !== artworkPrefetchGeneration) return;
      prefetchedArtworkUrls = Object.fromEntries(entries.filter((entry) => entry !== null));
      prefetchedArtworkReadyIds = new Set(details.map((entry) => entry.item.id));
    });
  });

  $effect(() => {
    if (!active) {
      lastAutoplayedId = null;
      return;
    }
    if (!autoplay || selectionMode || !detail || lastAutoplayedId === detail.item.id) return;
    lastAutoplayedId = detail.item.id;
    void audition.preview(detail, sources.bindings, true);
  });

  $effect(() => {
    if (selectionMode || !detail || viewState.membershipBaselines[detail.item.id] !== undefined) return;
    viewState.membershipBaselines = { ...viewState.membershipBaselines, [detail.item.id]: membershipSignature };
  });

  onDestroy(() => {
    inspector.clear();
  });


  function openInlineCreate(): void {
    viewState.inlineCreateOpen = true;
    review.createError = null;
    bulk.createError = null;
    void tick().then(() => newPlaylistNameInput?.focus());
  }

  async function createPlaylistAndAdd(): Promise<void> {
    const playlistId = selectionMode
      ? await bulk.createPlaylistAndSelect(viewState.newPlaylistName, viewState.newPlaylistIcon)
      : await review.createPlaylistAndAdd(viewState.newPlaylistName, viewState.newPlaylistIcon);
    if (!playlistId) return;
    viewState.newPlaylistName = "";
    viewState.newPlaylistIcon = "lucide:list-music";
    viewState.inlineCreateOpen = false;
    await tick();
    checklistRoot?.querySelector<HTMLElement>(`[data-review-playlist-id="${playlistId}"]`)?.focus();
  }

  function reloadSelection(): Promise<boolean> | undefined {
    if (!selectionMode || bulk.loading) return undefined;
    return bulk.open(selectedItemIds, library.playlistSummaries, true);
  }

  async function loadDecodedReviewArtwork(
    entry: NonNullable<typeof inspector.detail>,
    bindings = sources.bindings,
  ): Promise<string | null> {
    const url = await musicReviewArtworkDataUrl(entry, bindings);
    if (url && typeof Image !== "undefined") {
      const image = new Image();
      image.src = url;
      await image.decode().catch(() => undefined);
    }
    return url;
  }

  async function ensureReviewArtwork(itemId: string | null): Promise<void> {
    if (!itemId) return;
    const details = await inspector.prefetch([itemId]);
    if (prefetchedArtworkReadyIds.has(itemId)) return;
    const entry = details.find((candidate) => candidate.item.id === itemId);
    if (!entry) return;
    const url = await loadDecodedReviewArtwork(entry);
    if (url) prefetchedArtworkUrls = { ...prefetchedArtworkUrls, [itemId]: url };
    prefetchedArtworkReadyIds = new Set([...prefetchedArtworkReadyIds, itemId]);
  }

  async function finishReviewState(reviewState: "unreviewed" | "reviewed" | "ignored"): Promise<void> {
    if (preparingNext || review.actionBusy) return;
    const nextItemId = reviewTreeIndex >= 0 ? reviewItemIds[reviewTreeIndex + 1] ?? null : null;
    preparingNext = true;
    try {
      const artworkReady = ensureReviewArtwork(nextItemId);
      await review.changeReviewState(reviewState, null, nextItemId);
      await artworkReady;
    } finally {
      preparingNext = false;
    }
  }

  function confirmIgnore(): void {
    ignoreConfirmOpen = false;
    if (selectionMode) void ignoreSelection();
    else void finishReviewState("ignored");
  }

  async function restoreSelectionToReview(): Promise<void> {
    if (!selectionCanIgnore || bulk.saving || preparingNext) return;
    const restoredItemIds = [...selectedItemIds];
    const restoredItemId = restoredItemIds[0] ?? null;
    preparingNext = true;
    try {
      if (!await bulk.setReviewState("unreviewed")) return;
      for (const itemId of restoredItemIds) inspector.invalidate(itemId);
      onClearSelection();
      lastSelectedId = null;
      library.selectItem(restoredItemId);
    } finally {
      preparingNext = false;
    }
  }

  function toggleIgnore(): void {
    if (ignoreActive) {
      if (selectionMode) void restoreSelectionToReview();
      else void finishReviewState("unreviewed");
      return;
    }
    ignoreConfirmOpen = true;
  }

  async function skipCurrentItem(): Promise<void> {
    if (!item || preparingNext || review.actionBusy) return;
    preparingNext = true;
    try {
      const skipped = new Set(sessionSkippedIds).add(item.id);
      let nextItemId = nextPendingMusicReviewTreeItemId(items, item.id, skipped);
      if (!nextItemId) {
        viewState.sessionSkippedIds = [];
        nextItemId = nextPendingMusicReviewTreeItemId(items, item.id, new Set());
      } else {
        viewState.sessionSkippedIds = [...skipped];
      }
      await ensureReviewArtwork(nextItemId);
      if (nextItemId) inspector.selectCached(nextItemId);
      library.selectItem(nextItemId);
    } finally {
      preparingNext = false;
    }
  }

  async function continueCurrentItem(): Promise<void> {
    if (!item || preparingNext || review.actionBusy) return;
    const nextItemId = reviewTreeIndex >= 0 ? reviewItemIds[reviewTreeIndex + 1] ?? null : null;
    viewState.membershipBaselines = { ...viewState.membershipBaselines, [item.id]: membershipSignature };
    preparingNext = true;
    try {
      await ensureReviewArtwork(nextItemId);
      if (nextItemId) inspector.selectCached(nextItemId);
      library.selectItem(nextItemId);
    } finally {
      preparingNext = false;
    }
  }

  async function saveAndContinue(): Promise<void> {
    if (!item || !needsSave || membershipSaving || preparingNext || review.actionBusy) return;
    if (item.reviewState !== "reviewed") {
      viewState.membershipBaselines = { ...viewState.membershipBaselines, [item.id]: membershipSignature };
      await finishReviewState("reviewed");
      return;
    }
    await continueCurrentItem();
  }

  async function saveSelectionAndContinue(): Promise<void> {
    if (!selectionReady || !selectionNeedsSave || bulk.saving || preparingNext) return;
    const savedItemIds = new Set(selectedIdSet);
    const nextItemId = nextPendingMusicReviewSelectionItemId(items, savedItemIds);
    preparingNext = true;
    try {
      if (!await bulk.saveReviewSelection()) return;
      await ensureReviewArtwork(nextItemId);
      for (const itemId of savedItemIds) inspector.invalidate(itemId);
      onClearSelection();
      const targetItemId = nextItemId ?? firstMusicReviewTreeItemId(items);
      if (targetItemId) inspector.selectCached(targetItemId);
      lastSelectedId = null;
      library.selectItem(targetItemId);
    } finally {
      preparingNext = false;
    }
  }

  async function ignoreSelection(): Promise<void> {
    if (!selectionCanIgnore || bulk.saving || preparingNext) return;
    const ignoredItemIds = new Set(selectedIdSet);
    const nextItemId = nextPendingMusicReviewSelectionItemId(items, ignoredItemIds);
    preparingNext = true;
    try {
      if (!await bulk.ignoreReviewSelection()) return;
      await ensureReviewArtwork(nextItemId);
      for (const itemId of ignoredItemIds) inspector.invalidate(itemId);
      onClearSelection();
      const targetItemId = nextItemId ?? firstMusicReviewTreeItemId(items);
      if (targetItemId) inspector.selectCached(targetItemId);
      lastSelectedId = null;
      library.selectItem(targetItemId);
    } finally {
      preparingNext = false;
    }
  }

  async function selectRelative(delta: number): Promise<void> {
    const targetId = reviewItemIds[reviewTreeIndex + delta];
    if (!targetId) return;
    library.selectItem(targetId);
    lastSelectedId = null;
  }

  /** Toggle review playback, marking YouTube items as starting so the preview gives feedback while the review queue is prepared. */
  async function toggleReviewPlayback(): Promise<void> {
    if (!detail || reviewStartItemId === detail.item.id || (reviewPlayerReady && player.youtubePlaybackStarting)) return;
    if (reviewPlayerReady) {
      await player.togglePlay();
      return;
    }
    const itemId = detail.item.id;
    if (detail.item.sourceKind === "youtube-video") reviewStartItemId = itemId;
    try {
      await audition.preview(detail, sources.bindings, true);
    } finally {
      if (reviewStartItemId === itemId) reviewStartItemId = null;
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (!active || viewState.managingPlaylists) return;
    if (event.isComposing || event.altKey || isMusicReviewEditableTarget(event.target)) return;
    const modified = event.ctrlKey || event.metaKey;
    if (selectionMode) {
      if (event.key === "Enter" && modified) {
        event.preventDefault();
        void saveSelectionAndContinue();
      }
      return;
    }
    if (event.code === "Space" && !modified) {
      event.preventDefault();
      void toggleReviewPlayback();
    } else if (event.key === "ArrowLeft" && modified) {
      event.preventDefault(); void selectRelative(-1);
    } else if (event.key === "ArrowRight" && modified) {
      event.preventDefault(); void selectRelative(1);
    } else if (event.key === "Enter" && modified) {
      event.preventDefault();
      if (needsSave) void saveAndContinue();
      else if (item?.reviewState === "reviewed") void continueCurrentItem();
    } else if (modified) {
      return;
    } else if (event.key === "ArrowLeft") {
      event.preventDefault(); void player.seekByMs(-10_000);
    } else if (event.key === "ArrowRight") {
      event.preventDefault(); void player.seekByMs(10_000);
    } else if (event.key === "ArrowUp") {
      event.preventDefault(); void player.adjustVolume(0.05);
    } else if (event.key === "ArrowDown") {
      event.preventDefault(); void player.adjustVolume(-0.05);
    } else if (event.key.toLowerCase() === "m") {
      event.preventDefault(); void player.toggleMute();
    } else if (event.key === "+" || event.key === "=" || event.code === "NumpadAdd") {
      event.preventDefault(); void player.setRate(clampRate(player.snapshot.rate + 0.25));
    } else if (event.key === "-" || event.code === "NumpadSubtract") {
      event.preventDefault(); void player.setRate(clampRate(player.snapshot.rate - 0.25));
    } else if (!event.shiftKey && /^[07-9]$/.test(event.key) && player.snapshot.durationMs) {
      event.preventDefault(); void player.seekToMs(Math.round(player.snapshot.durationMs * Number(event.key) / 10));
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="review-main flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
  <section class:review-controls-locked={viewState.managingPlaylists} class="review-audition min-h-0 overflow-y-auto px-4 pb-3 pt-2" data-music-scrollable="true" inert={viewState.managingPlaylists}>
    <div class="review-toolbar flex items-center justify-between gap-3">
      {#if showPanelButton}<button type="button" onclick={onOpenPanel} class="grid h-8 w-8 shrink-0 place-items-center rounded-lg text-foreground hover:bg-secondary" aria-label={t("music.builder.openContextPanel")} title={t("music.builder.openContextPanel")}><PanelLeft size={14} /></button>{/if}
      <button type="button" onclick={onOpenPlayer} class="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-full bg-secondary px-2.5 text-[0.7rem]" aria-label={t("music.backToPlayer")} data-music-focus-key="builder:back-to-player"><ChevronLeft size={14} />{compactPlayerLabel ? t("music.returnToPlayerShort") : t("music.backToPlayer")}</button>
      <p class="min-w-0 flex-1 truncate text-center text-[0.68rem] font-medium text-muted-foreground" role="status" aria-live="polite">{t("music.builder.reviewProgress", reviewedCount, totalCount)}</p>
      <div class="review-toolbar-actions flex flex-wrap items-center justify-end gap-x-3 gap-y-1">
        {#if selectionMode}
          <button type="button" onclick={() => onAutoplayChange(!autoplay)} aria-pressed={autoplay} class="review-toolbar-action inline-flex h-8 items-center gap-1.5 rounded-md px-2 text-[0.65rem] text-foreground transition-colors hover:bg-secondary">
            {#if autoplay}
              <Play size={13} />
            {:else}
              <span class="relative size-3.25 shrink-0" aria-hidden="true"><Play class="absolute inset-0" size={13} /><Slash class="absolute inset-0" size={13} /></span>
            {/if}
            {autoplay ? t("music.builder.reviewAutoplayOn") : t("music.builder.reviewAutoplayOff")}
          </button>
          <button
            type="button"
            onclick={toggleIgnore}
            disabled={!selectionCanIgnore || bulk.saving || preparingNext}
            class="review-toolbar-action inline-flex h-8 items-center gap-1.5 rounded-md px-2 text-[0.65rem] text-foreground hover:bg-secondary disabled:cursor-not-allowed disabled:text-muted-foreground disabled:hover:bg-transparent"
            aria-label={ignoreActionLabel}
          >{#if ignoreActive}<Eye size={13} />{:else}<X size={13} />{/if}{ignoreActionLabel}</button>
        {:else}
          <button type="button" onclick={() => onAutoplayChange(!autoplay)} aria-pressed={autoplay} class="review-toolbar-action inline-flex h-8 items-center gap-1.5 rounded-md px-2 text-[0.65rem] text-foreground transition-colors hover:bg-secondary">
            {#if autoplay}
              <Play size={13} />
            {:else}
              <span class="relative size-3.25 shrink-0" aria-hidden="true"><Play class="absolute inset-0" size={13} /><Slash class="absolute inset-0" size={13} /></span>
            {/if}
            {autoplay ? t("music.builder.reviewAutoplayOn") : t("music.builder.reviewAutoplayOff")}
          </button>
          <button type="button" onclick={toggleIgnore} disabled={!detail || review.actionBusy || preparingNext} class="review-toolbar-action inline-flex h-8 items-center gap-1.5 rounded-md px-2 text-[0.65rem] text-foreground hover:bg-secondary">{#if ignoreActive}<Eye size={13} />{:else}<X size={13} />{/if}{ignoreActionLabel}</button>
        {/if}
      </div>
    </div>
    {#if !selectionMode && library.currentState.groupBy !== "none" && library.currentWindow.groups.length > 0}
      <div class="mt-2 flex gap-1.5 overflow-x-auto pb-1" aria-label={t("music.builder.reviewGroups")}>
        {#each library.currentWindow.groups as group (group.key)}
          <span class="shrink-0 rounded-full bg-secondary px-2 py-1 text-[0.62rem] text-secondary-foreground">{group.key} · {group.count}</span>
        {/each}
      </div>
    {/if}

    {#if selectionMode}
      <div class="review-player mt-4 flex min-w-0 items-center gap-4">
        <div class="review-media relative grid h-28 w-28 shrink-0 place-items-center overflow-hidden rounded-xl bg-primary/10 text-primary" aria-hidden="true">
          <span class="absolute left-6 top-6 h-14 w-14 rounded-lg border border-primary/20"></span>
          <span class="absolute left-8 top-8 h-14 w-14 rounded-lg border border-primary/35 bg-background/45"></span>
          <span class="absolute bottom-3 right-3 grid h-6 w-6 place-items-center rounded-full bg-primary text-primary-foreground"><Check size={13} strokeWidth={2.6} /></span>
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="truncate text-base font-semibold">{t("music.builder.reviewSelectionTracks", selectionSummary.itemCount)}</h2>
          <p class="mt-1 truncate text-xs text-muted-foreground" title={selectionContext}>{selectionContext}</p>
        </div>
      </div>
    {:else if item}
      <div class="review-player mt-4 flex min-w-0 items-center gap-4">
        <div bind:this={surface} class:review-media-youtube={item.sourceKind === "youtube-video"} class="review-media relative grid h-28 w-28 shrink-0 place-items-center overflow-hidden rounded-xl">
          {#if item.sourceKind === "youtube-video"}
            {#if prefetchedArtworkUrl}
              <img src={prefetchedArtworkUrl} alt="" class="absolute inset-0 h-full w-full object-contain" draggable="false" />
            {:else if !player.currentSource || !reviewPlayerReady}
              <div class="grid h-full w-full place-items-center rounded-xl bg-primary/10 text-primary">
                <Disc3 size={38} strokeWidth={1.3} />
              </div>
            {/if}
          {:else if item.sourceKind === "local-file" && !player.localHasVideo}
            {#if prefetchedArtworkUrl}
              <img src={prefetchedArtworkUrl} alt="" class="absolute inset-0 h-full w-full object-contain" draggable="false" onload={() => player.handleArtworkLoaded()} />
            {:else if reviewPlayerReady && player.currentArtworkUrl}
              <img src={player.currentArtworkUrl} alt="" class="absolute inset-0 h-full w-full object-contain" draggable="false" onload={() => player.handleArtworkLoaded()} onerror={() => player.handleArtworkError()} />
            {:else}
              <div class="grid h-full w-full place-items-center rounded-xl bg-primary/10 text-primary">
                <Disc3 size={38} strokeWidth={1.3} />
              </div>
            {/if}
          {:else if !player.currentSource || !reviewPlayerReady}
            <div class="grid h-full w-full place-items-center rounded-xl bg-primary/10 text-primary">
              <Disc3 size={38} strokeWidth={1.3} />
            </div>
          {/if}
        </div>

        <div class="min-w-0 flex-1">
          <div class="min-w-0">
            <h2 class="truncate text-base font-semibold">{previewTitle}</h2>
            <p class="mt-0.5 truncate text-xs text-muted-foreground">{previewArtist}</p>
            {#if detail && (detail.item.availability !== "available" || issue)}
              <div class="mt-1 flex min-w-0 items-center gap-1.5 text-[0.65rem]">
                <TriangleAlert size={12} class="shrink-0 text-destructive" />
                <span class="min-w-0 truncate text-muted-foreground">{attentionLabel()}</span>
                {#if issue?.actionRequired && repairAvailable}<button type="button" onclick={() => onRepairIssue(issue)} class="shrink-0 font-semibold text-primary hover:underline">{t("music.builder.repair")}</button>{/if}
              </div>
            {/if}
          </div>

          <div class="mt-4 flex items-center gap-3">
            <div class="min-w-0 flex-1 text-[0.68rem] tabular-nums text-muted-foreground">
              <input type="range" min="0" max={reviewPlayerReady ? player.progressMax : previewDurationMs} value={reviewPlayerReady ? player.progressValue : 0} disabled={!reviewPlayerReady} oninput={(event) => { void player.seekToMs(Number(event.currentTarget.value)); }} class="music-seek-slider music-seek-slider-edge-aligned block" style={`--music-seek-progress: ${seekSliderProgress}; --music-seek-thumb-size: 1rem; --music-seek-track-height: 0.3rem;`} aria-label={t("music.seek")} />
              <div class="mt-1 flex justify-between">
                <span>{formatPlaybackTime(reviewPlayerReady ? player.snapshot.positionMs : 0)}</span>
                <span>{formatPlaybackTime(previewDurationMs)}</span>
              </div>
            </div>
            <button type="button" onclick={() => { void toggleReviewPlayback(); }} disabled={!detail || item.availability !== "available" || reviewPlaybackStarting} class="review-play shrink-0" aria-label={reviewPlaybackStarting ? t("music.builder.loading") : reviewPlayerReady && player.isPlaying ? t("music.pause") : t("music.play")} title={t("music.builder.reviewPlayTitle", formatShortcut("Space"))}>
              {#if reviewPlaybackStarting}<LoaderCircle size={20} strokeWidth={3} class="animate-spin motion-reduce:animate-none" />{:else if reviewPlayerReady && player.isPlaying}<Pause size={18} fill="currentColor" />{:else}<Play size={18} fill="currentColor" />{/if}
            </button>
          </div>
        </div>
      </div>
    {/if}
  </section>

  <section class="review-classify flex min-h-0 flex-col">
    <div class="shrink-0 p-3">
      <h2 class="text-sm font-semibold">{t("music.builder.classifyPlaylists")}</h2>
      {#if selectionMode}<p class="mt-0.5 text-[0.65rem] text-muted-foreground">{t("music.builder.reviewSelectionApplyHint")}</p>{/if}
      {#if viewState.inlineCreateOpen && !viewState.managingPlaylists}
        <form class="mt-2" onsubmit={(event) => { event.preventDefault(); void createPlaylistAndAdd(); }}>
          <div class="flex items-center gap-2">
            <IconPicker value={viewState.newPlaylistIcon} onChange={(value) => viewState.newPlaylistIcon = value} ariaLabel={t("music.builder.selectPlaylistIcon")} showUpload={false}>
              {#snippet trigger({ open, toggle, panelId })}
                <button
                  type="button"
                  class={`grid h-8 w-8 shrink-0 place-items-center rounded-lg bg-secondary text-foreground transition-colors hover:bg-accent ${open ? "bg-accent" : ""}`}
                  aria-label={t("music.builder.selectPlaylistIcon")}
                  aria-haspopup="dialog"
                  aria-expanded={open}
                  aria-controls={panelId}
                  title={t("music.builder.selectPlaylistIcon")}
                  onclick={toggle}
                >
                  <MusicPlaylistIcon icon={viewState.newPlaylistIcon} size={16} strokeWidth={1.6} />
                </button>
              {/snippet}
            </IconPicker>
            <input bind:this={newPlaylistNameInput} bind:value={viewState.newPlaylistName} aria-label={t("music.builder.inlinePlaylistName")} class="field h-8 min-w-0 flex-1 px-2.5 text-xs" placeholder={t("music.builder.inlinePlaylistName")} />
          </div>
          {#if selectionMode ? bulk.createError : review.createError}<Notice class="mt-1.5 text-[0.65rem]" message={(selectionMode ? bulk.createError : review.createError) ?? ""} />{/if}
          <div class="mt-2 flex justify-end gap-2">
            <button type="button" onclick={() => { viewState.inlineCreateOpen = false; review.createError = null; bulk.createError = null; }} class="h-8 rounded-md bg-secondary px-2.5 text-xs font-medium">{t("music.builder.cancel")}</button>
            <button type="submit" disabled={!viewState.newPlaylistName.trim() || (selectionMode ? bulk.creatingPlaylist : review.creatingPlaylist)} class="inline-flex h-8 items-center gap-1.5 rounded-md bg-primary px-2.5 text-xs font-medium text-primary-foreground disabled:opacity-40"><ListPlus size={14} />{t("music.builder.createAndAdd")}</button>
          </div>
        </form>
      {:else}
        <div class="mt-2 flex flex-wrap items-center gap-2">
          <button type="button" onclick={openInlineCreate} disabled={viewState.managingPlaylists} class="inline-flex h-8 items-center gap-1.5 rounded-full bg-primary px-3 text-xs font-medium text-primary-foreground disabled:cursor-not-allowed disabled:bg-secondary disabled:text-muted-foreground disabled:opacity-50"><ListPlus size={14} />{t("music.builder.newPlaylist")}</button>
          <button type="button" onclick={() => { viewState.inlineCreateOpen = false; review.createError = null; bulk.createError = null; viewState.managingPlaylists = true; }} disabled={viewState.managingPlaylists} class="inline-flex h-8 items-center gap-1.5 rounded-full bg-secondary px-3 text-xs font-medium text-foreground disabled:cursor-not-allowed disabled:text-muted-foreground disabled:opacity-50"><Pencil size={13} />{t("music.builder.managePlaylists")}</button>
          {#if viewState.managingPlaylists}
            <div class="ml-auto flex items-center gap-2">
              <button type="button" onclick={() => { void playlistManager?.cancelManaging(); }} disabled={playlistManagerActionsDisabled} class="h-8 rounded-lg px-3 text-xs font-medium text-muted-foreground hover:bg-secondary hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50">{t("music.builder.cancel")}</button>
              <button type="button" onclick={() => { void playlistManager?.finishManaging(); }} disabled={playlistManagerActionsDisabled} class="h-8 rounded-lg bg-primary px-3 text-xs font-semibold text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50">{t("music.builder.doneManaging")}</button>
            </div>
          {/if}
        </div>
      {/if}
    </div>

    {#if viewState.managingPlaylists}
      <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 pb-3" data-music-scrollable="true">
        <MusicPlaylistManager
          bind:this={playlistManager}
          bind:actionsDisabled={playlistManagerActionsDisabled}
          playlists={library.playlistSummaries}
          onEdit={onEditPlaylist}
          onDelete={onDeletePlaylist}
          onReorder={onReorderPlaylists}
          onDone={() => viewState.managingPlaylists = false}
          showHeader={false}
        />
      </div>
    {:else}
      <div bind:this={checklistRoot} class="flex min-h-0 flex-1 flex-col">
        <MusicPlaylistPicker
          playlists={library.playlistSummaries}
          checkedIds={selectionMode && selectionProjectionReady ? bulk.checkedIds : checkedIds}
          mixedIds={selectionMode && selectionProjectionReady ? bulk.mixedIds : new Set<string>()}
          mixedCounts={selectionMode && selectionProjectionReady ? bulk.initialCounts : {}}
          selectionSize={selectionMode ? selectionSummary.itemCount : 0}
          onToggle={(playlist) => { if (selectionMode) bulk.toggle(playlist.id); else void review.toggleMembership(playlist); }}
          disabled={selectionMode && !selectionProjectionReady}
          errors={selectionMode ? {} : review.membershipErrors}
        />
        {#if selectionMode && (bulk.error || bulk.selectionStale)}
          <Notice
            class="mx-3 mb-2 shrink-0 px-1 py-1 text-[0.68rem]"
            message={bulk.selectionStale ? t("music.builder.selectionChanged") : (bulk.error ?? "")}
            tone={bulk.selectionStale ? "warning" : "neutral"}
            copy={bulk.selectionStale ? false : undefined}
            onRetry={reloadSelection}
          />
        {/if}
      </div>
    {/if}

    <div class:review-controls-locked={viewState.managingPlaylists} class="review-actions grid shrink-0 grid-cols-2 gap-3 p-3" inert={viewState.managingPlaylists} aria-disabled={viewState.managingPlaylists}>
      {#if selectionMode}
        <button type="button" onclick={onClearSelection} disabled={bulk.saving || preparingNext} class="review-action h-full w-full border border-border/70 bg-background text-foreground">{t("music.builder.clearReviewSelection")}</button>
      {:else if item?.reviewState === "reviewed"}
        <button type="button" onclick={() => { void continueCurrentItem(); }} disabled={!detail || review.actionBusy || preparingNext} class="review-action h-full w-full border border-border/70 bg-background text-foreground">{t("music.builder.continue")}</button>
      {:else}
        <button type="button" onclick={() => { void skipCurrentItem(); }} disabled={!detail || review.actionBusy || preparingNext} class="review-action h-full w-full border border-border/70 bg-background text-foreground">{t("music.builder.skipTrack")}</button>
      {/if}
      {#if selectionMode}
        <button type="button" onclick={() => { void saveSelectionAndContinue(); }} disabled={!selectionReady || !selectionNeedsSave || bulk.saving || preparingNext} class={`review-action review-save ${selectionNeedsSave ? "bg-primary text-primary-foreground" : "bg-secondary text-muted-foreground"}`} title={selectionNeedsSave ? t("music.builder.markReviewedTitle", formatShortcut("Mod + Enter")) : undefined}>
          {#if bulk.saving}<LoaderCircle class="animate-spin motion-reduce:animate-none" size={14} />{t("music.builder.saving")}{:else}<Check size={14} />{t("music.builder.saveAndContinue")}<ChevronRight size={14} />{/if}
        </button>
      {:else}
        <button type="button" onclick={() => { void saveAndContinue(); }} disabled={!detail || !needsSave || membershipSaving || review.actionBusy || preparingNext} class={`review-action review-save ${needsSave ? "bg-primary text-primary-foreground" : "bg-secondary text-muted-foreground"}`} title={needsSave ? t("music.builder.markReviewedTitle", formatShortcut("Mod + Enter")) : undefined}><Check size={14} />{t("music.builder.saveAndContinue")}<ChevronRight size={14} /></button>
      {/if}
    </div>
  </section>
</div>

{#if ignoreConfirmOpen}
  <ConfirmDialog
    title={selectionMode ? t("music.builder.ignoreSelectionTitle", selectionSummary.itemCount) : t("music.builder.ignoreTrackTitle")}
    message={selectionMode ? t("music.builder.ignoreSelectionDescription") : t("music.builder.ignoreTrackDescription")}
    confirmLabel={selectionMode ? t("music.builder.ignoreSelectionConfirm") : t("music.builder.ignoreTrackConfirm")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirmIgnore}
    onCancel={() => ignoreConfirmOpen = false}
  />
{/if}

<style>
  .review-main { min-height: 0; }
  .review-audition { flex: 0 0 auto; }
  .review-classify { min-height: 14rem; flex: 1 1 0; }
  .review-play { display: grid; height: 2.5rem; width: 2.5rem; place-items: center; border-radius: 9999px; background: var(--primary); color: var(--primary-foreground); }
  .review-play:disabled { opacity: 0.4; }
  .review-action { display: inline-flex; min-height: 2.25rem; align-items: center; justify-content: center; gap: 0.375rem; border-radius: 0.5rem; padding: 0 0.5rem; font-size: calc(0.72rem * var(--type-scale)); font-weight: 600; }
  .review-action:disabled { cursor: not-allowed; }
  .review-action:disabled:not(.review-save) { opacity: 0.4; }
  .review-media-youtube { width: 9.3333rem; }
  .review-controls-locked :global(button),
  .review-controls-locked :global(input),
  .review-controls-locked :global([role="button"]) { cursor: not-allowed !important; opacity: 0.38; }
  @container (width < 620px) {
    .review-main { min-height: 32rem; flex: 1 0 auto; overflow: visible; }
    .review-toolbar { gap: 0.375rem; }
    .review-toolbar-actions { flex-wrap: nowrap; column-gap: 0.125rem; }
    .review-toolbar-action { gap: 0.25rem; padding-inline: 0.375rem; }
    .review-audition, .review-classify { min-height: auto; overflow: visible; }
    .review-audition { flex: 0 0 auto; padding: 0.625rem; }
    .review-classify { flex: 1 0 18rem; border-left: 0; }
    .review-classify > :global(div:nth-child(2)) { min-height: 9rem; }
    .review-actions { position: sticky; bottom: 0; z-index: 5; }
  }
  @container (width < 380px) {
    .review-player { align-items: flex-start; }
    .review-media { height: 5rem; width: 5rem; }
    .review-media-youtube { width: 6.6667rem; }
    .review-play { height: 2rem; width: 2rem; }
  }
  @container (width >= 620px) and (width < 860px) {
    .review-classify { min-height: 15rem; }
  }
  @container (height < 300px) and (width >= 620px) {
    .review-audition { padding-block: 0.5rem; }
    .review-audition > :global(.aspect-video) { max-height: 7rem; }
  }
</style>
