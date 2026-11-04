<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import MoreHorizontal from "@lucide/svelte/icons/ellipsis";
  import Pause from "@lucide/svelte/icons/pause";
  import Play from "@lucide/svelte/icons/play";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { revealLocalFile } from "$lib/api/music";
  import { bulkEditMusicMemberships, bulkSnoozeMusicItems, getMusicInspectorDetail, removeMusicSnooze, reorderMusicPlaylists } from "$lib/api/music-library";
  import { createMusicBuilderInspectorController } from "$lib/music/builder/inspector.svelte";
  import { projectMusicBuilderLayout } from "$lib/music/builder/layout";
  import {
    backMusicBuilderRoute,
    initialMusicBuilderRoute,
    musicBuilderDestinationForKey,
    pushMusicBuilderRoute,
    type MusicBuilderDestination,
    type MusicBuilderHistory,
  } from "$lib/music/builder/routing";
  import { createMusicLibraryController, type MusicDestinationState } from "$lib/music/library/controller.svelte";
  import { getMusicSourcesController } from "$lib/music/sources/controller.svelte";
  import { createMusicReviewAuditionController, MUSIC_CONTEXT_BOUNDARY_EVENT } from "$lib/music/review/audition.svelte";
  import { createMusicReviewController } from "$lib/music/review/controller.svelte";
  import { createMusicPlaylistController } from "$lib/music/playlists/controller.svelte";
  import { createMusicBulkEditController } from "$lib/music/builder/bulk-edit-controller.svelte";
  import { createMusicInterchangeController } from "$lib/music/library/interchange-controller.svelte";
  import {
    musicReviewArtworkDataUrl,
    parseMusicReviewAutoplay,
  } from "$lib/music/review";
  import {
    firstMusicReviewTreeItemId,
    musicUnreviewedItemCount,
    musicReviewTreeItemIds,
  } from "$lib/music/review/tree";
  import type { MusicItemListEntry, MusicWeight } from "$lib/music/library/contracts";
  import type { MusicIssue, MusicSourceCollection } from "$lib/music/library/contracts";
  import type { MusicSourceRefreshPlan } from "$lib/music/sources/refresh";
  import { musicBuilderPlaybackDecision } from "$lib/music/builder/playback-transition";
  import type { MusicAddSourceKind, MusicLocalSourceSelection } from "$lib/music/sources/drafts";
  import { musicSnoozeEndsAt, musicSnoozePreset, type MusicSnoozePreset } from "$lib/music/session/snooze";
  import {
    createMusicBuilderContextViewState,
    createMusicReviewTreeViewState,
    createMusicReviewWorkspaceViewState,
  } from "$lib/music/builder/view-state";
  import { notifyMusicLibraryChanged, onMusicLibraryChanged } from "$lib/music/library/events";
  import { onActiveVaultIdentityChange, requireActiveVaultIdentity } from "$lib/vault/active-vault";
  import { getConfigKey, setConfigKey } from "$lib/vault/config";
  import MusicBuilderAsyncState from "$lib/components/music/builder/MusicBuilderAsyncState.svelte";
  import MusicBuilderFilterBar from "$lib/components/music/builder/MusicBuilderFilterBar.svelte";
  import MusicPreparationActivity from "$lib/components/music/builder/MusicPreparationActivity.svelte";
  import MusicBuilderOverview from "$lib/components/music/builder/MusicBuilderOverview.svelte";
  import MusicDetectedFolderCard from "$lib/components/music/builder/MusicDetectedFolderCard.svelte";
  import MusicVirtualItemList from "$lib/components/music/builder/MusicVirtualItemList.svelte";
  import MusicAddSourceDialog from "$lib/components/music/builder/MusicAddSourceDialog.svelte";
  import MusicReviewIssuesPanel from "$lib/components/music/builder/MusicReviewIssuesPanel.svelte";
  import MusicItemRepairDialog from "$lib/components/music/builder/MusicItemRepairDialog.svelte";
  import MusicNetworkRefreshDialog from "$lib/components/music/builder/MusicNetworkRefreshDialog.svelte";
  import MusicRelinkWizard from "$lib/components/music/builder/MusicRelinkWizard.svelte";
  import MusicSourceRemovalDialog from "$lib/components/music/builder/MusicSourceRemovalDialog.svelte";
  import MusicSourcesBrowser from "$lib/components/music/builder/MusicSourcesBrowser.svelte";
  import MusicSourcesTree from "$lib/components/music/builder/MusicSourcesTree.svelte";
  import MusicSourceNameDialog from "$lib/components/music/builder/MusicSourceNameDialog.svelte";
  import MusicReviewWorkspace from "$lib/components/music/builder/MusicReviewWorkspace.svelte";
  import MusicReviewTree from "$lib/components/music/builder/MusicReviewTree.svelte";
  import MusicBuilderContextPanel from "$lib/components/music/builder/MusicBuilderContextPanel.svelte";
  import MusicBuilderDock from "$lib/components/music/builder/MusicBuilderDock.svelte";
  import MusicBuilderToolbar from "$lib/components/music/builder/MusicBuilderToolbar.svelte";
  import MusicPlaylistManager from "$lib/components/music/builder/MusicPlaylistManager.svelte";
  import MusicSoundscapeBuilder from "$lib/components/music/soundscape/MusicSoundscapeBuilder.svelte";
  import MusicPlaylistDialog from "$lib/components/music/builder/MusicPlaylistDialog.svelte";
  import MusicInterchangeDialog from "$lib/components/music/builder/MusicInterchangeDialog.svelte";
  import type { MusicBuilderInitialAction } from "$lib/music/builder/loader";
  import { musicIssueGroup } from "$lib/music/library/issue-presentation";
  import { isSystemMusicPlaylistId, orderMusicPlaylists, systemMusicPlaylistName } from "$lib/music/playlists/system";
  import {
    buildMusicSourceBrowser,
    allMusicSourceBrowserItems,
    findMusicSourceBrowserNode,
    LOCAL_MUSIC_SOURCE_ID,
    musicSourceBrowserItems,
    musicSourceBrowserPath,
    YOUTUBE_MUSIC_SOURCE_ID,
  } from "$lib/music/sources/browser";
  import { projectMusicSourceQueue } from "$lib/music/sources/playback";

  let {
    onOpenPlayer,
    presentation = "desktop",
    active = true,
    initialAction = null,
    onInitialActionHandled = () => undefined,
  }: {
    onOpenPlayer: () => void;
    presentation?: "desktop" | "mobile";
    active?: boolean;
    initialAction?: MusicBuilderInitialAction | null;
    onInitialActionHandled?: () => void;
  } = $props();
  const { t } = getLocalization();
  const library = createMusicLibraryController();
  const inspector = createMusicBuilderInspectorController();
  const sources = getMusicSourcesController();
  const audition = createMusicReviewAuditionController();
  const review = createMusicReviewController(library, inspector);
  const playlist = createMusicPlaylistController(library);
  const bulk = createMusicBulkEditController(library);
  const interchange = createMusicInterchangeController(() => library.playlistSummaries, () => sources.bindings, () => library.vaultId);
  const supportsSoundscapes = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.soundscapes");
  const supportsLocalFileReveal = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.local-file-reveal");
  const supportsItemRepair = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.local-item-repair");
  const supportsRelinkPlans = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.local-root-relink-plans");
  const supportsRootReselection = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.local-root-reselection");
  const mobilePresentation = $derived(presentation === "mobile");
  let root = $state<HTMLElement | null>(null);
  let width = $state(1000);
  let height = $state(680);
  let history = $state<MusicBuilderHistory>(initialMusicBuilderRoute(1, null, { playlistIds: new Set() }));
  let unsubscribeVault: (() => void) | null = null;
  let unsubscribeLibraryChanges: (() => void) | null = null;
  let localRefreshBroadcastSuppression = 0;
  let sourceSurface = $state<"add-local" | "add-youtube" | "relink" | "remove" | "item-repair" | null>(null);
  let localSourceSelection = $state<MusicLocalSourceSelection | null>(null);
  let sourceSurfaceCollection = $state<MusicSourceCollection | null>(null);
  let sourceRenameNodeId = $state<string | null>(null);
  let sourceRenameSaving = $state(false);
  let sourceRenameError = $state<string | null>(null);
  let localSourceNameOverride = $state(savedSourceName("music.sources.localName"));
  let youtubeSourceNameOverride = $state(savedSourceName("music.sources.youtubeName"));

  /** Read a saved source name, ignoring a value of any other type that a vault file may hold. */
  function savedSourceName(key: string): string | null {
    const saved = getConfigKey<unknown>(key, null);
    return typeof saved === "string" ? saved : null;
  }

  let repairItemId = $state<string | null>(null);
  let pendingRefreshPlan = $state<MusicSourceRefreshPlan | null>(null);
  let playlistSurface = $state<"create" | "edit" | "duplicate" | "delete" | null>(null);
  let playlistSurfaceReturnsToCurrentView = $state(false);
  let playlistSurfaceTargetId = $state<string | null>(null);
  let reviewAutoplay = $state(parseMusicReviewAutoplay(getConfigKey<unknown>("music.review.autoplay", undefined)));
  let choosingFirstUseFolder = $state(false);
  let firstUseFolderError = $state<string | null>(null);
  let builderInitializing = $state(true);
  let firstUsePreparationActive = $state(false);
  let firstUseFinalizing = $state(false);
  let firstUseFinalizationGeneration = 0;
  let playlistManagementOpen = $state(false);
  interface PlaylistManagerHandle {
    cancelManaging: () => Promise<void>;
    finishManaging: () => Promise<void>;
  }
  let playlistManager = $state<PlaylistManagerHandle | null>(null);
  let playlistManagerActionsDisabled = $state(false);
  let soundscapeAddRequest = $state(0);
  let toolbarMenuOpen = $state(false);
  let pendingPlaylistItemId = $state<string | null>(null);
  let pendingSourceItemId = $state<string | null>(null);
  let reviewSelectionClearRequest = $state(0);
  let showIgnoredReviewItems = $state(false);
  const addSourceKind = $derived<MusicAddSourceKind | null>(
    sourceSurface === "add-local"
      ? "local-root"
      : sourceSurface === "add-youtube"
        ? "youtube"
          : null,
  );
  let previouslyActive = false;
  const contextViewState = $state(createMusicBuilderContextViewState());
  const reviewTreeViewState = $state(createMusicReviewTreeViewState());
  let reviewWorkspaceViewState = $state(createMusicReviewWorkspaceViewState());
  const layout = $derived(projectMusicBuilderLayout({ width, height }));
  const destination = $derived(history.current.destination);
  const playlistManagementActive = $derived(
    playlistManagementOpen || (destination.kind === "review" && reviewWorkspaceViewState.managingPlaylists),
  );
  const isPlaylistDestination = $derived(destination.kind === "playlist");
  const reviewItems = $derived(library.currentWindow.items.filter((item) =>
    showIgnoredReviewItems || item.reviewState !== "ignored"));
  const issueCount = $derived(library.issues.length);
  const allLibraryItems = $derived(library.windows.review?.items ?? []);
  const reviewCount = $derived(musicUnreviewedItemCount(allLibraryItems));
  const sourceBrowserNames = $derived({
    local: localSourceNameOverride?.trim() || t("music.builder.localMusic"),
    youtube: youtubeSourceNameOverride?.trim() || t("music.builder.youtube"),
    savedVideos: t("music.builder.savedVideos"),
    unlinkedFiles: t("music.builder.unlinkedFiles"),
  });
  const sourceBrowserNodes = $derived(buildMusicSourceBrowser(
    allLibraryItems,
    sources.collections,
    library.sourceSummaries,
    sourceBrowserNames,
  ));
  const selectedSourceNode = $derived(findMusicSourceBrowserNode(sourceBrowserNodes, contextViewState.selectedSourceId));
  const selectedSourcePath = $derived(selectedSourceNode
    ? musicSourceBrowserPath(sourceBrowserNodes, selectedSourceNode.id)
    : []);
  const selectedSourceCollection = $derived(selectedSourceNode?.collectionId
    ? sources.collections.find((collection) => collection.id === selectedSourceNode.collectionId) ?? null
    : null);
  const unlinkedSelectedSources = $derived(sources.unlinkedLocalSources(sourceCollectionIdsForSelection()));
  const selectedSourceItems = $derived(selectedSourceNode
    ? musicSourceBrowserItems(selectedSourceNode)
    : allMusicSourceBrowserItems(sourceBrowserNodes));
  const selectedSourceQueue = $derived(projectMusicSourceQueue(selectedSourceItems, sources.bindings));
  const selectedSourceQueueId = $derived(`source-browser:${selectedSourceNode?.id ?? "all"}`);
  const selectedSourceQueueName = $derived(selectedSourceNode?.name ?? t("music.builder.allSources"));
  const sourceRenameNode = $derived(findMusicSourceBrowserNode(sourceBrowserNodes, sourceRenameNodeId));
  const reviewIssueItemIds = $derived(new Set(library.issues.flatMap((issue) => issue.itemId ? [issue.itemId] : [])));
  const activeReviewItemId = $derived(library.selectedItem?.id ?? firstMusicReviewTreeItemId(reviewItems));
  const activeReviewIssue = $derived(activeReviewItemId
    ? library.issues.find((issue) => issue.itemId === activeReviewItemId && issue.actionRequired)
      ?? library.issues.find((issue) => issue.itemId === activeReviewItemId)
      ?? null
    : null);
  const routeContext = $derived({ playlistIds: new Set(library.playlistSummaries.map((playlist) => playlist.id)), itemIds: new Set(library.currentWindow.items.map((item) => item.id)) });
  const playingItemId = $derived(audition.musicPlayer.activeQueueItemIds[audition.musicPlayer.currentQueueIndex] ?? null);
  const activePlaylistSummary = $derived(destination.kind === "playlist" ? library.playlistSummaries.find((entry) => entry.id === destination.playlistId) ?? null : null);
  const activePlaylistInPlayer = $derived(Boolean(activePlaylistSummary && audition.musicPlayer.activePlaylistId === activePlaylistSummary.id && audition.musicPlayer.currentSource));
  const activePlaylistPlaying = $derived(activePlaylistInPlayer && audition.musicPlayer.isPlaying);
  const playlistPlaybackAvailable = $derived(Boolean(
    audition.musicPlayer.activePlaylistId
    && audition.musicPlayer.currentSource,
  ));
  const playlistPlaybackPlaying = $derived(playlistPlaybackAvailable && audition.musicPlayer.isPlaying);
  const activeSourceQueueInPlayer = $derived(Boolean(
    audition.musicPlayer.activeSourceQueueId === selectedSourceQueueId
    && audition.musicPlayer.currentSource,
  ));
  const activeSourceQueuePlaying = $derived(activeSourceQueueInPlayer && audition.musicPlayer.isPlaying);
  const sourcePlaybackAvailable = $derived(Boolean(
    audition.musicPlayer.activeSourceQueueId
    && audition.musicPlayer.currentSource,
  ));
  const sourcePlaybackPlaying = $derived(sourcePlaybackAvailable && audition.musicPlayer.isPlaying);
  const firstUseProjectionPending = $derived(
    sources.firstUseSession && sources.roots.length > 0,
  );
  const builderPreparation = $derived(
    builderInitializing
      || sources.preparingDefaultFolder
      || firstUsePreparationActive
      || firstUseProjectionPending,
  );
  const firstUseNeedsFolder = $derived(
    destination.kind === "review"
      && sources.loaded
      && !sources.busy
      && !sources.preparingDefaultFolder
      && sources.roots.length === 0
      && library.currentWindow.items.length === 0,
  );
  const firstUseRefreshProgress = $derived(Object.values(sources.refreshStatuses).find((status) => status.kind === "local-root"));
  const localSourceCollectionIds = $derived(sources.collections.filter((collection) => collection.kind === "local-root").map((collection) => collection.id));
  const sourceRefreshActive = $derived(Object.values(sources.refreshStatuses).some((status) => status.state === "queued" || status.state === "running"));
  const localSourceRefreshActive = $derived(Object.values(sources.refreshStatuses).some((status) => status.kind === "local-root" && (status.state === "queued" || status.state === "running")));

  $effect(() => {
    if (!firstUseProjectionPending
      || sources.preparingDefaultFolder
      || !sources.loaded
      || firstUseFinalizing
      || library.busy
      || library.loadingMore
      || !library.vaultId) return;
    beginFirstUsePreparation();
    void finalizeFirstUsePreparation();
  });

  $effect(() => {
    const selectedSourceId = contextViewState.selectedSourceId;
    if (selectedSourceId && !findMusicSourceBrowserNode(sourceBrowserNodes, selectedSourceId)) contextViewState.selectedSourceId = null;
  });

  $effect(() => {
    if (contextViewState.reviewPanel !== "issues") return;
    if (library.issues.length === 0) {
      contextViewState.reviewPanel = "folders";
      contextViewState.reviewIssueGroup = null;
      return;
    }
    const selectedGroup = contextViewState.reviewIssueGroup;
    if (selectedGroup && !library.issues.some((issue) => musicIssueGroup(issue) === selectedGroup)) contextViewState.reviewIssueGroup = null;
  });

  function beginFirstUsePreparation(): void {
    builderInitializing = true;
    if (firstUsePreparationActive) return;
    firstUsePreparationActive = true;
    firstUseFinalizationGeneration += 1;
  }

  async function finalizeFirstUsePreparation(): Promise<void> {
    if (!firstUsePreparationActive || sources.preparingDefaultFolder || firstUseFinalizing) return;
    const generation = firstUseFinalizationGeneration;
    const vaultId = library.vaultId;
    firstUseFinalizing = true;
    try {
      const refreshed = await library.refreshAfterMutation();
      if (generation !== firstUseFinalizationGeneration || vaultId !== library.vaultId) return;
      if (!refreshed) throw library.error ?? new Error("The prepared music library could not be loaded.");
      const fullyLoaded = await library.loadAllCurrentItems();
      if (generation !== firstUseFinalizationGeneration || vaultId !== library.vaultId) return;
      if (!fullyLoaded) throw library.loadMoreError ?? new Error("The prepared review list could not be completed.");
      const itemId = firstMusicReviewTreeItemId(reviewItems);
      if (itemId) {
        library.selectItem(itemId);
        const selected = await inspector.select(itemId);
        if (!selected || generation !== firstUseFinalizationGeneration || vaultId !== library.vaultId) return;
        const orderedIds = musicReviewTreeItemIds(reviewItems);
        const itemIndex = orderedIds.indexOf(itemId);
        const nearbyIds = orderedIds.slice(Math.max(0, itemIndex - 1), itemIndex + 4);
        const details = await inspector.prefetch(nearbyIds);
        await Promise.all(details.map(async (detail) => {
          const url = await musicReviewArtworkDataUrl(detail, sources.bindings).catch(() => null);
          if (!url || typeof Image === "undefined") return;
          const image = new Image();
          image.src = url;
          await image.decode().catch(() => undefined);
        }));
        const detail = inspector.detail;
        if (active && reviewAutoplay && detail?.item.id === itemId) {
          await audition.preview(detail, sources.bindings, true);
        }
      }
      if (generation !== firstUseFinalizationGeneration || vaultId !== library.vaultId) return;
      sources.completeFirstUseSession();
      builderInitializing = false;
      firstUsePreparationActive = false;
    } catch (error) {
      if (generation === firstUseFinalizationGeneration && vaultId === library.vaultId) {
        library.error = error instanceof Error ? error : new Error(String(error));
        builderInitializing = false;
        firstUsePreparationActive = false;
      }
    } finally {
      firstUseFinalizing = false;
    }
  }

  $effect(() => {
    const action = initialAction;
    if (!action || !library.vaultId || builderInitializing) return;
    if (action === "new-playlist") playlistSurface = "create";
    else if (action === "open-playlists") void navigateNow({ kind: "playlists" });
    else if (action === "open-review") void navigateNow({ kind: "review" });
    else if (action.kind === "open-review-item") void openReviewItem(action.itemId);
    else if (action.kind === "open-issues") void openReviewIssues();
    else if (action.kind === "open-soundscapes") {
      if (supportsSoundscapes) void navigateNow({ kind: "soundscapes" });
    }
    else void openInitialItem(action.itemId);
    onInitialActionHandled();
  });

  $effect(() => {
    if (!active && previouslyActive && audition.active) void audition.restore();
    previouslyActive = active;
  });

  function observeRoot(node: HTMLElement): { destroy: () => void } {
    root = node;
    const update = () => { width = node.clientWidth; height = node.clientHeight; };
    const observer = new ResizeObserver(update);
    observer.observe(node);
    update();
    return { destroy: () => { observer.disconnect(); root = null; } };
  }

  async function loadVault(vaultId: string): Promise<void> {
    const generation = ++firstUseFinalizationGeneration;
    builderInitializing = true;
    firstUsePreparationActive = false;
    Object.assign(contextViewState, createMusicBuilderContextViewState());
    Object.assign(reviewTreeViewState, createMusicReviewTreeViewState());
    Object.assign(reviewWorkspaceViewState, createMusicReviewWorkspaceViewState());
    showIgnoredReviewItems = false;
    bulk.clear();
    reviewSelectionClearRequest += 1;
    playlistManagementOpen = false;
    inspector.reset();
    library.setVault(vaultId);
    sources.setVault(vaultId);
    try {
      const [libraryLoaded, sourcesLoaded] = await Promise.all([
        library.preloadCoreDestinations(),
        sources.load(),
      ]);
      if (generation !== firstUseFinalizationGeneration
        || vaultId !== library.vaultId || vaultId !== sources.vaultId) return;
      if (!libraryLoaded) throw library.error ?? new Error("The music library could not be loaded.");
      if (!sourcesLoaded) throw new Error(sources.error ?? "The music sources could not be loaded.");
      const recoveryPlan = sources.prepareUninitializedLocalRefresh();
      if (recoveryPlan.targets.length > 0) {
        await sources.runRefresh(recoveryPlan, false);
        if (!await library.refreshAfterMutation()) {
          throw library.error ?? new Error("The refreshed music library could not be loaded.");
        }
      }
      const remembered = history.current.destination;
      history = initialMusicBuilderRoute(reviewCount, remembered, routeContext);
      library.navigate(history.current.destination);
      if (!await library.ensureCurrentDestination()) {
        throw library.error ?? new Error("The music workspace could not be loaded.");
      }
      if (history.current.destination.kind === "playlist") {
        await playlist.load(history.current.destination.playlistId);
      }
      if (sources.firstUseSession && sources.roots.length > 0) {
        beginFirstUsePreparation();
        await finalizeFirstUsePreparation();
      } else {
        const fullyLoaded = await library.loadAllCurrentItems();
        if (generation !== firstUseFinalizationGeneration || vaultId !== library.vaultId) return;
        if (!fullyLoaded) {
          throw library.loadMoreError ?? new Error("The music workspace could not be completed.");
        }
        builderInitializing = false;
      }
    } catch (error) {
      if (generation !== firstUseFinalizationGeneration || vaultId !== library.vaultId) return;
      library.error = error instanceof Error ? error : new Error(String(error));
      builderInitializing = false;
      firstUsePreparationActive = false;
    }
  }

  function primaryAction(): void {
    if (destination.kind === "playlists") { playlist.clear(); playlistSurfaceReturnsToCurrentView = false; playlistSurfaceTargetId = null; playlistSurface = "create"; return; }
    if (destination.kind === "sources") contextViewState.selectedSourceId = LOCAL_MUSIC_SOURCE_ID;
  }

  function openYouTubeSource(): void {
    toolbarMenuOpen = false;
    sourceSurface = "add-youtube";
  }

  async function chooseLocalSource(): Promise<void> {
    toolbarMenuOpen = false;
    const selection = await sources.chooseLocalFolder();
    if (!selection) return;
    localSourceSelection = selection;
    sourceSurface = "add-local";
  }

  function sourceCollectionIdsForSelection(): string[] | undefined {
    if (!selectedSourceNode) return undefined;
    if (selectedSourceNode.collectionId) return [selectedSourceNode.collectionId];
    if (selectedSourceNode.sourceType === "local") {
      return sources.collections.filter((collection) => collection.kind === "local-root").map((collection) => collection.id);
    }
    return sources.collections.filter((collection) => collection.kind === "youtube-playlist").map((collection) => collection.id);
  }

  function refreshSelectedSource(): void {
    requestSourceRefresh(sourceCollectionIdsForSelection());
  }

  function openSourceRename(): void {
    if (!selectedSourceNode || (selectedSourceNode.kind !== "group" && !selectedSourceNode.collectionId)) return;
    toolbarMenuOpen = false;
    sourceRenameError = null;
    sourceRenameNodeId = selectedSourceNode.id;
  }

  async function saveSourceName(name: string): Promise<void> {
    const node = sourceRenameNode;
    if (!node) return;
    sourceRenameSaving = true;
    sourceRenameError = null;
    try {
      if (node.id === LOCAL_MUSIC_SOURCE_ID) {
        localSourceNameOverride = name;
        setConfigKey("music.sources.localName", name);
      } else if (node.id === YOUTUBE_MUSIC_SOURCE_ID) {
        youtubeSourceNameOverride = name;
        setConfigKey("music.sources.youtubeName", name);
      } else if (node.collectionId) {
        await sources.renameCollection(node.collectionId, name);
        await library.refreshSummariesAfterMutation();
      }
      sourceRenameNodeId = null;
    } catch (error) {
      sourceRenameError = error instanceof Error ? error.message : String(error);
    } finally {
      sourceRenameSaving = false;
    }
  }

  function createPlaylistFromWorkspace(): void {
    playlist.clear();
    playlistSurfaceReturnsToCurrentView = true;
    playlistSurfaceTargetId = null;
    playlistSurface = "create";
  }

  function workspaceStatus(): string {
    if (destination.kind === "playlists") return t("music.builder.playlistCount", library.playlistSummaries.length);
    if (destination.kind === "playlist") {
      const summary = library.playlistSummaries.find((entry) => entry.id === destination.playlistId);
      return summary ? `${systemMusicPlaylistName(summary.id, summary.name, t)} · ${t("music.tracks", summary.totalCount)}` : t("music.builder.playlists");
    }
    if (destination.kind === "sources") return selectedSourceNode?.name ?? t("music.builder.allSources");
    return t("music.builder.soundscapes");
  }

  function closeSourceSurface(): void {
    sources.cancelResolution();
    if (sourceSurface === "relink" && sources.relinkCancellable) {
      void sources.cancelRelink();
    }
    sourceSurface = null;
    localSourceSelection = null;
    sourceSurfaceCollection = null;
    repairItemId = null;
    sources.clearItemRepair();
  }

  function requestSourceRefresh(collectionIds?: string[]): void {
    const plan = sources.prepareRefresh(collectionIds);
    if (plan.targets.length === 0) {
      // A source without a folder on this device cannot refresh, so offer to choose its folder instead.
      const unlinked = sources.unlinkedLocalSources(collectionIds);
      if (unlinked.length === 1) void openRelink(unlinked[0].collection.id);
      return;
    }
    if (plan.requiresNetworkConfirmation) pendingRefreshPlan = plan;
    else void runSourceRefresh(plan, false);
  }

  function refreshReviewFolders(): void {
    requestSourceRefresh(localSourceCollectionIds);
  }

  function setIgnoredTracksVisible(showIgnored: boolean): void {
    showIgnoredReviewItems = showIgnored;
  }

  async function runSourceRefresh(plan: MusicSourceRefreshPlan, allowNetwork: boolean): Promise<void> {
    pendingRefreshPlan = null;
    localRefreshBroadcastSuppression += 1;
    try {
      await sources.runRefresh(plan, allowNetwork);
      await library.refreshAfterMutation();
      notifyMusicLibraryChanged();
    } finally {
      localRefreshBroadcastSuppression -= 1;
    }
  }

  function detectedFolderAdded(): void {
    void library.refreshAfterMutation();
    if (destination.kind !== "sources") navigate({ kind: "sources" });
  }

  function collectionById(collectionId: string): MusicSourceCollection | null {
    return sources.collections.find((collection) => collection.id === collectionId) ?? null;
  }

  async function openRemoval(collectionId: string): Promise<void> {
    const collection = collectionById(collectionId);
    if (!collection) return;
    sourceSurfaceCollection = collection;
    await sources.inspectRemoval(collectionId);
    sourceSurface = "remove";
  }

  async function openRelink(collectionId: string): Promise<void> {
    const collection = collectionById(collectionId);
    if (!collection?.localRootId) return;
    // Without a folder on this device there are no prior locations to match, so the folder is chosen directly.
    const neverLinked = sources.unlinkedLocalSources([collection.id])[0]?.status === "unbound";
    if (supportsRootReselection || neverLinked) {
      try {
        if (await sources.reselectLocalRoot(collection)) await library.refreshAfterMutation();
      } catch (error) {
        sources.error = error instanceof Error ? error.message : String(error);
      }
      return;
    }
    if (!supportsRelinkPlans) return;
    sourceSurfaceCollection = collection;
    sourceSurface = "relink";
  }

  function chooseRootFolder(rootId: string): void {
    const collection = sources.collections.find((entry) => entry.localRootId === rootId);
    if (collection) void openRelink(collection.id);
  }

  function repairIssue(issue: MusicIssue): void {
    if (issue.rootId) {
      const collection = sources.collections.find((entry) => entry.localRootId === issue.rootId);
      if (collection) void openRelink(collection.id);
      return;
    }
    if (issue.itemId && supportsItemRepair) { openItemRepair(issue.itemId); return; }
    if (issue.collectionId) requestSourceRefresh([issue.collectionId]);
  }

  function canRepairIssue(issue: MusicIssue): boolean {
    if (issue.rootId) return supportsRelinkPlans || supportsRootReselection;
    if (issue.itemId) return supportsItemRepair;
    return Boolean(issue.collectionId);
  }

  function openItemRepair(itemId: string): void {
    if (!supportsItemRepair) return;
    repairItemId = itemId;
    sources.clearItemRepair();
    sourceSurface = "item-repair";
  }

  async function navigateNow(next: MusicBuilderDestination): Promise<void> {
    if (next.kind === "soundscapes" && !supportsSoundscapes) return;
    history = pushMusicBuilderRoute(history, { destination: next, inspectorItemId: null }, routeContext);
    library.navigate(next);
    inspector.clear();
    await library.ensureCurrentDestination();
    if (next.kind === "playlist") await playlist.load(next.playlistId);
    else playlist.clear();
  }

  async function openInitialItem(_itemId: string): Promise<void> {
    const activePlaylistId = audition.musicPlayer.activePlaylistId;
    const next: MusicBuilderDestination = activePlaylistId
      && library.playlistSummaries.some((entry) => entry.id === activePlaylistId)
      ? { kind: "playlist", playlistId: activePlaylistId }
      : { kind: "playlists" };
    await navigateNow(next);
  }

  async function openReviewItem(itemId: string): Promise<void> {
    await navigateNow({ kind: "review" });
    if (destination.kind !== "review") return;
    if (!library.currentWindow.items.some((item) => item.id === itemId)) {
      library.patchCurrentState({
        search: "",
        sourceKind: null,
        availability: null,
        reviewState: null,
        sourceCollectionId: null,
        membershipPlaylistId: null,
        snoozed: null,
      });
      if (!await library.refresh()) return;
    }
    if (destination.kind !== "review") return;
    const item = library.currentWindow.items.find((entry) => entry.id === itemId);
    if (!item) return;
    clearReviewSelection();
    reviewTreeViewState.search = "";
    if (item.reviewState === "ignored") showIgnoredReviewItems = true;
    library.selectItem(itemId);
  }

  function navigate(next: MusicBuilderDestination): void {
    if (next.kind === "soundscapes" && !supportsSoundscapes) return;
    contextViewState.contextPanelOpen = false;
    toolbarMenuOpen = false;
    if (next.kind !== "playlists" && next.kind !== "playlist") playlistManagementOpen = false;
    if (next.kind === destination.kind && next.kind !== "playlist") return;
    if (next.kind === "playlist" && destination.kind === "playlist" && next.playlistId === destination.playlistId) return;
    void navigateNow(next);
  }

  async function openReviewIssues(): Promise<void> {
    if (destination.kind !== "review") await navigateNow({ kind: "review" });
    contextViewState.reviewPanel = "issues";
    contextViewState.reviewIssueGroup = null;
    if (layout.contextPanelPresentation === "sheet") contextViewState.contextPanelOpen = true;
  }

  async function selectReviewIssue(issue: MusicIssue): Promise<void> {
    if (!issue.itemId) return;
    while (!library.currentWindow.items.some((item) => item.id === issue.itemId)
      && library.currentWindow.items.length < library.currentWindow.totalCount) {
      if (!await library.loadMore()) break;
    }
    if (library.currentWindow.items.some((item) => item.id === issue.itemId)) {
      clearReviewSelection();
      library.selectItem(issue.itemId);
    }
  }

  async function openPlayerFromBuilder(): Promise<void> {
    if (audition.active) await audition.restore();
    onOpenPlayer();
  }

  function takePlaybackOwnership(): void {
    if (musicBuilderPlaybackDecision(audition.ownsPlayback, "explicit-playback") === "release-review") audition.discard();
  }

  function setReviewAutoplay(value: boolean): void {
    reviewAutoplay = value;
    setConfigKey("music.review.autoplay", value);
  }

  async function handleBack(): Promise<void> {
    const previous = backMusicBuilderRoute(history, routeContext);
    if (!previous) { void openPlayerFromBuilder(); return; }
    history = previous;
    library.navigate(history.current.destination);
    await library.ensureCurrentDestination();
    if (history.current.destination.kind === "playlist") await playlist.load(history.current.destination.playlistId);
    else playlist.clear();
  }

  function patchFilters(patch: Partial<MusicDestinationState>): void {
    library.patchCurrentState({ ...patch, offset: 0 });
    void library.refresh();
  }

  function updateSearch(search: string): void {
    library.patchCurrentState({ search, offset: 0 });
    if (destination.kind !== "playlists") void library.refresh();
  }

  async function openPlaylistManagementSurface(playlistId: string, mode: "edit" | "delete"): Promise<void> {
    if (!await playlist.load(playlistId)) return;
    if (mode === "delete" && !await playlist.inspectDelete()) return;
    playlistSurfaceReturnsToCurrentView = true;
    playlistSurfaceTargetId = playlistId;
    playlistSurface = mode;
  }

  async function reorderPlaylistSummaries(playlistIds: string[]): Promise<boolean> {
    const previous = orderMusicPlaylists(library.playlistSummaries).map((entry) => ({ ...entry, intendedUses: [...entry.intendedUses] }));
    const byId = new Map(previous.map((entry) => [entry.id, entry]));
    if (playlistIds.length !== previous.length || playlistIds.some((id) => !byId.has(id))) return false;
    // The optimistic order reuses the current keys in their sorted order; the receipts then supply the stored keys.
    const next = playlistIds.map((id, index) => ({ ...byId.get(id)!, orderKey: previous[index].orderKey }));
    library.playlistSummaries = next;
    try {
      const receipts = await reorderMusicPlaylists({
        playlists: next.map((entry) => ({ playlistId: entry.id, expectedVersion: entry.version })),
        updatedAtMs: Date.now(),
      });
      const stored = new Map(receipts.map((receipt) => [receipt.id, receipt]));
      library.playlistSummaries = next.map((entry) => {
        const receipt = stored.get(entry.id);
        return receipt ? { ...entry, orderKey: receipt.orderKey, version: receipt.version } : entry;
      });
      playlist.clear();
      return true;
    } catch {
      library.playlistSummaries = previous;
      await library.refreshAfterMutation();
      return false;
    }
  }

  async function playCurrentPlaylist(itemId?: string): Promise<void> {
    takePlaybackOwnership();
    await playlist.play(sources.bindings, itemId);
  }

  async function playReplacementPlaylist(playlistId: string, navigateToPlaylist: boolean): Promise<void> {
    if (navigateToPlaylist) await navigateNow({ kind: "playlist", playlistId });
    else await playlist.load(playlistId);
    await playCurrentPlaylist();
  }

  async function toggleCurrentPlaylist(): Promise<void> {
    if (!playlistPlaybackAvailable) return;
    await audition.musicPlayer.togglePlay();
  }

  async function togglePlaylistItem(item: MusicItemListEntry): Promise<void> {
    if (pendingPlaylistItemId === item.id) return;
    const currentItemId = audition.musicPlayer.activeQueueItemIds[audition.musicPlayer.currentQueueIndex] ?? null;
    if (activePlaylistInPlayer && currentItemId === item.id) {
      await audition.musicPlayer.togglePlay();
      return;
    }
    if (item.sourceKind === "youtube-video") pendingPlaylistItemId = item.id;
    try {
      await playCurrentPlaylist(item.id);
    } finally {
      if (pendingPlaylistItemId === item.id) pendingPlaylistItemId = null;
    }
  }

  async function playCurrentSourceQueue(itemId?: string): Promise<void> {
    takePlaybackOwnership();
    await audition.musicPlayer.loadSourceQueue(
      selectedSourceQueueId,
      selectedSourceQueueName,
      selectedSourceQueue,
      itemId,
    );
  }

  async function toggleCurrentSourceQueue(): Promise<void> {
    if (!sourcePlaybackAvailable) return;
    await audition.musicPlayer.togglePlay();
  }

  async function toggleSourceItem(item: MusicItemListEntry): Promise<void> {
    if (!selectedSourceQueue.some((entry) => entry.itemId === item.id)) return;
    if (pendingSourceItemId === item.id) return;
    const currentItemId = audition.musicPlayer.activeQueueItemIds[audition.musicPlayer.currentQueueIndex] ?? null;
    if (activeSourceQueueInPlayer && currentItemId === item.id) {
      await audition.musicPlayer.togglePlay();
      return;
    }
    if (item.sourceKind === "youtube-video") pendingSourceItemId = item.id;
    try {
      await playCurrentSourceQueue(item.id);
    } finally {
      if (pendingSourceItemId === item.id) pendingSourceItemId = null;
    }
  }

  function clearReviewSelection(): void {
    reviewTreeViewState.selectedItemIds = [];
    reviewTreeViewState.selectedFolderIds = [];
    reviewSelectionClearRequest += 1;
    bulk.clear();
  }

  function activateReviewItem(itemId: string): void {
    if (reviewTreeViewState.selectedItemIds.length > 0) clearReviewSelection();
    library.selectItem(itemId);
  }

  async function chooseFirstUseFolder(): Promise<void> {
    if (choosingFirstUseFolder) return;
    choosingFirstUseFolder = true;
    firstUseFolderError = null;
    beginFirstUsePreparation();
    try {
      const draft = await sources.chooseLocalFolder();
      if (!draft) {
        builderInitializing = false;
        firstUsePreparationActive = false;
        return;
      }
      await sources.addLocalFolder(draft.selection, draft.name, true);
      await finalizeFirstUsePreparation();
    } catch (error) {
      firstUseFolderError = error instanceof Error ? error.message : String(error);
      builderInitializing = false;
      firstUsePreparationActive = false;
    } finally {
      choosingFirstUseFolder = false;
    }
  }

  async function removePlaylistItem(item: MusicItemListEntry): Promise<void> {
    if (destination.kind !== "playlist") return;
    await bulkEditMusicMemberships({
      actionId: crypto.randomUUID(),
      itemIds: [item.id],
      addPlaylistIds: [],
      removePlaylistIds: [destination.playlistId],
      weightPlaylistIds: [],
      weight: null,
      updatedAtMs: Date.now(),
    });
    await library.refreshAfterMutation();
    await playlist.refreshActivePlayback(sources.bindings);
  }

  async function setPlaylistItemWeight(item: MusicItemListEntry, weight: MusicWeight): Promise<void> {
    if (destination.kind !== "playlist") return;
    await bulkEditMusicMemberships({
      actionId: crypto.randomUUID(),
      itemIds: [item.id],
      addPlaylistIds: [],
      removePlaylistIds: [],
      weightPlaylistIds: [destination.playlistId],
      weight,
      updatedAtMs: Date.now(),
    });
    await library.refreshAfterMutation();
    await playlist.refreshActivePlayback(sources.bindings);
  }

  async function snoozeItem(item: MusicItemListEntry, duration: MusicSnoozePreset, playlistId: string | null): Promise<void> {
    const now = Date.now();
    const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    const existing = (await getMusicInspectorDetail(item.id)).snoozes;
    const scoped = existing.filter((entry) => entry.startsAtMs <= now && (entry.endsAtMs === null || entry.endsAtMs > now)
      && (playlistId === null ? entry.scope === "all-playlists" : entry.scope === "playlist" && entry.playlistId === playlistId));
    const turningOff = scoped.length === 1 && musicSnoozePreset(scoped[0].startsAtMs, scoped[0].endsAtMs, timeZone) === duration;
    await Promise.all(scoped.map((entry) => removeMusicSnooze(entry.id)));
    const endsAtMs = musicSnoozeEndsAt(duration, now, timeZone);
    if (!turningOff) {
      await bulkSnoozeMusicItems({
        actionId: crypto.randomUUID(),
        itemIds: [item.id],
        scope: playlistId === null ? "all-playlists" : "playlist",
        playlistId,
        startsAtMs: now,
        endsAtMs,
        reason: "",
        createdAtMs: now,
      });
    }
    const queueIndex = audition.musicPlayer.activeQueueItemIds.indexOf(item.id);
    if (queueIndex >= 0 && (playlistId === null || playlistId === audition.musicPlayer.activePlaylistId)) {
      if (turningOff) {
        const remaining = existing.filter((entry) => !scoped.some((removed) => removed.id === entry.id)
          && entry.startsAtMs <= now && (entry.endsAtMs === null || entry.endsAtMs > now)
          && (audition.musicPlayer.activeSourceQueueId !== null
            || entry.scope === "all-playlists" || entry.playlistId === audition.musicPlayer.activePlaylistId));
        if (remaining.length === 0) audition.musicPlayer.clearQueueItemSnooze(queueIndex);
        else audition.musicPlayer.applyQueueItemSnooze(queueIndex, remaining.some((entry) => entry.endsAtMs === null)
          ? null : Math.max(...remaining.map((entry) => entry.endsAtMs ?? 0)));
      } else {
        audition.musicPlayer.applyQueueItemSnooze(queueIndex, endsAtMs);
      }
    }
    await library.refreshAfterMutation();
    await playlist.refreshActivePlayback(sources.bindings);
  }

  async function snoozePlaylistItem(item: MusicItemListEntry, duration: MusicSnoozePreset, everywhere: boolean): Promise<void> {
    if (destination.kind !== "playlist") return;
    await snoozeItem(item, duration, everywhere ? null : destination.playlistId);
  }

  async function snoozeSourceItem(item: MusicItemListEntry, duration: MusicSnoozePreset): Promise<void> {
    await snoozeItem(item, duration, null);
  }

  async function removeItemSnoozes(item: MusicItemListEntry, playlistId: string | null): Promise<void> {
    const now = Date.now();
    const active = (await getMusicInspectorDetail(item.id)).snoozes.filter((entry) =>
      entry.startsAtMs <= now && (entry.endsAtMs === null || entry.endsAtMs > now)
      && (playlistId === null || entry.scope === "all-playlists" || entry.playlistId === playlistId));
    await Promise.all(active.map((entry) => removeMusicSnooze(entry.id)));
    const queueIndex = audition.musicPlayer.activeQueueItemIds.indexOf(item.id);
    if (queueIndex >= 0 && (playlistId === null || playlistId === audition.musicPlayer.activePlaylistId)) {
      audition.musicPlayer.clearQueueItemSnooze(queueIndex);
    }
    await library.refreshAfterMutation();
    await playlist.refreshActivePlayback(sources.bindings);
  }

  async function showItemLocation(item: MusicItemListEntry): Promise<void> {
    if (!supportsLocalFileReveal) return;
    if (inspector.itemId !== item.id) await inspector.select(item.id);
    const location = inspector.detail?.locations.find((entry) => entry.availability === "available");
    if (!location) { openItemRepair(item.id); return; }
    const folder = sources.bindings.find((binding) => binding.rootId === location.rootId)?.folderPath;
    if (!folder) { openItemRepair(item.id); return; }
    const separator = folder.includes("\\") && !folder.includes("/") ? "\\" : "/";
    const path = `${folder.replace(/[\\/]+$/, "")}${separator}${location.relativePath.replace(/[\\/]+/g, separator)}`;
    await revealLocalFile(path);
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (!active) return;
    if (playlistManagementActive) return;
    if (event.key === "Escape") {
      if (contextViewState.contextPanelOpen) { event.preventDefault(); event.stopPropagation(); contextViewState.contextPanelOpen = false; return; }
      if (toolbarMenuOpen) { event.preventDefault(); event.stopPropagation(); toolbarMenuOpen = false; return; }
      if (sourceRenameNodeId) { event.preventDefault(); event.stopPropagation(); sourceRenameNodeId = null; return; }
      if (sourceSurface || pendingRefreshPlan) { event.stopPropagation(); closeSourceSurface(); pendingRefreshPlan = null; return; }
      if (destination.kind === "review" && reviewTreeViewState.selectedItemIds.length > 0) { event.preventDefault(); event.stopPropagation(); clearReviewSelection(); return; }
      event.stopPropagation();
      void handleBack();
    }
    const target = event.target instanceof Element ? event.target : null;
    const shortcutDestination = musicBuilderDestinationForKey(event.key);
    const shortcutBlocked = event.ctrlKey || event.metaKey || event.altKey || event.shiftKey
      || Boolean(target?.closest("input, textarea, [contenteditable='true'], [role='dialog']"))
      || Boolean(sourceRenameNodeId || sourceSurface || pendingRefreshPlan || playlistSurface || interchange.open);
    if (shortcutDestination && !shortcutBlocked && (shortcutDestination.kind !== "soundscapes" || supportsSoundscapes)) {
      event.preventDefault();
      event.stopImmediatePropagation();
      navigate(shortcutDestination);
      return;
    }
    if (event.key === "/" && !(event.target instanceof HTMLInputElement) && !(event.target instanceof HTMLTextAreaElement)) {
      if (destination.kind === "review") return;
      if (!isPlaylistDestination) return;
      event.preventDefault();
      if (layout.contextPanelPresentation === "sheet") contextViewState.contextPanelOpen = true;
      void tick().then(() => root?.querySelector<HTMLInputElement>("[data-builder-context-search]")?.focus());
    }
  }

  function handleWindowPointerDown(event: PointerEvent): void {
    if (!active) return;
    if (!toolbarMenuOpen || !(event.target instanceof Element)) return;
    if (event.target.closest(".toolbar-menu, .toolbar-icon")) return;
    toolbarMenuOpen = false;
  }

  onMount(() => {
    const handleBoundary = (event: Event) => {
      if (!(event instanceof CustomEvent)) return;
      const owner = event.detail?.owner;
      if ((owner === "calendar-event" || owner === "pomodoro") && musicBuilderPlaybackDecision(audition.ownsPlayback, "automation-boundary") === "supersede-review") audition.supersedeForBoundary(owner);
    };
    window.addEventListener(MUSIC_CONTEXT_BOUNDARY_EVENT, handleBoundary);
    try { void loadVault(requireActiveVaultIdentity()); }
    catch (error) { library.error = error instanceof Error ? error : new Error(String(error)); }
    unsubscribeVault = onActiveVaultIdentityChange((_previous, next) => {
      if (audition.active) audition.discard();
      if (next) void loadVault(next);
      else { library.setVault(null); sources.setVault(null); }
    });
    unsubscribeLibraryChanges = onMusicLibraryChanged(() => {
      if (localRefreshBroadcastSuppression > 0) return;
      void library.refreshAfterMutation();
    });
    return () => window.removeEventListener(MUSIC_CONTEXT_BOUNDARY_EVENT, handleBoundary);
  });

  onDestroy(() => {
    firstUseFinalizationGeneration += 1;
    unsubscribeVault?.();
    unsubscribeLibraryChanges?.();
    if (audition.active) void audition.restore();
  });
</script>

<svelte:window onkeydown={handleWindowKeydown} onpointerdown={handleWindowPointerDown} />

<section bind:this={root} use:observeRoot class="builder-root flex h-full min-h-0 select-none flex-col overflow-hidden text-foreground" style="background-color: var(--cal-bg);">
  <div class="builder-shell relative grid min-h-0 flex-1" class:builder-wide={layout.mode === "wide"} class:builder-medium={layout.mode === "medium"} class:builder-narrow={layout.mode === "narrow"} class:builder-contextless={builderPreparation || firstUseNeedsFolder}>
    {#if !builderPreparation && !firstUseNeedsFolder}
      <aside class:context-open={contextViewState.contextPanelOpen} class:builder-controls-locked={playlistManagementActive} class="builder-context-panel relative z-20 flex min-h-0 flex-col overflow-hidden bg-background/20" inert={playlistManagementActive}>
        {#if layout.contextPanelPresentation === "sheet"}
          <div class="flex h-11 shrink-0 items-center px-2">
            <button
              type="button"
              onclick={() => contextViewState.contextPanelOpen = false}
              class="inline-flex h-8 items-center gap-1.5 rounded-lg px-2 text-[0.7rem] font-medium text-foreground active:bg-secondary"
              aria-label={t("music.builder.closeContextPanel")}
            ><ArrowLeft size={15} />{t("music.builder.back")}</button>
          </div>
        {/if}
        {#if destination.kind === "review"}
          {#if contextViewState.reviewPanel === "issues" && issueCount > 0}
            <MusicReviewIssuesPanel
              issues={library.issues}
              items={library.currentWindow.items}
              sources={library.sourceSummaries}
              selectedGroup={contextViewState.reviewIssueGroup}
              activeItemId={activeReviewItemId}
              refreshing={sourceRefreshActive}
              onSelectGroup={(group) => contextViewState.reviewIssueGroup = group}
              onBack={() => { contextViewState.reviewPanel = "folders"; contextViewState.reviewIssueGroup = null; }}
              onSelectIssue={(issue) => { void selectReviewIssue(issue); }}
              onRepair={repairIssue}
              {canRepairIssue}
              onRefresh={() => requestSourceRefresh()}
            />
          {:else}
            <MusicReviewTree
              items={reviewItems}
              totalCount={reviewItems.length}
              activeItemId={reviewTreeViewState.selectedItemIds.length > 0 ? null : library.selectedItem?.id ?? null}
              onActivate={activateReviewItem}
              issueCount={issueCount}
              issueItemIds={reviewIssueItemIds}
              onOpenIssues={() => { void openReviewIssues(); }}
              selectionClearRequest={reviewSelectionClearRequest}
              selectionDisabled={bulk.saving}
              canRefresh={localSourceCollectionIds.length > 0}
              refreshing={localSourceRefreshActive}
              onRefresh={refreshReviewFolders}
              showIgnored={showIgnoredReviewItems}
              onShowIgnoredChange={setIgnoredTracksVisible}
              viewState={reviewTreeViewState}
              onViewStateChange={(state) => Object.assign(reviewTreeViewState, state)}
            />
          {/if}
        {:else if destination.kind === "sources"}
          <MusicSourcesTree
            nodes={sourceBrowserNodes}
            selectedNodeId={contextViewState.selectedSourceId}
            onSelect={(nodeId) => {
              contextViewState.selectedSourceId = nodeId;
              if (layout.contextPanelPresentation === "sheet") contextViewState.contextPanelOpen = false;
            }}
          />
        {:else}
          <MusicBuilderContextPanel
            {destination}
            state={library.currentState}
            playlists={library.playlistSummaries}
            soundscapeFilter={contextViewState.soundscapeFilter}
            onSearch={updateSearch}
            onNavigate={navigate}
            onCreatePlaylist={createPlaylistFromWorkspace}
            onManagePlaylists={() => playlistManagementOpen = true}
            onSoundscapeFilter={(filter) => contextViewState.soundscapeFilter = filter}
          />
        {/if}
        {#if layout.dockPresentation === "sidebar"}<MusicBuilderDock {destination} {reviewCount} showAllLabels={mobilePresentation} includeSoundscapes={supportsSoundscapes} onNavigate={navigate} />{/if}
      </aside>
    {/if}
    <main class="relative flex min-h-0 min-w-0 flex-col overflow-hidden bg-background/30">
      {#if destination.kind !== "review"}
        <MusicBuilderToolbar status={workspaceStatus()} showPanelButton={layout.contextPanelPresentation === "sheet"} onOpenPanel={() => contextViewState.contextPanelOpen = true} onOpenPlayer={openPlayerFromBuilder} compactPlayerLabel={mobilePresentation} locked={playlistManagementOpen}>
          {#snippet actions()}
            {#if playlistManagementOpen && (destination.kind === "playlists" || destination.kind === "playlist")}
              <button type="button" onclick={() => { void playlistManager?.cancelManaging(); }} disabled={playlistManagerActionsDisabled} class="h-8 rounded-lg px-3 text-xs font-medium text-muted-foreground hover:bg-secondary hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50">{t("music.builder.cancel")}</button>
              <button type="button" onclick={() => { void playlistManager?.finishManaging(); }} disabled={playlistManagerActionsDisabled} class="h-8 rounded-lg bg-primary px-3 text-xs font-semibold text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50">{t("music.builder.doneManaging")}</button>
            {:else if destination.kind === "playlists"}
              <button type="button" onclick={createPlaylistFromWorkspace} class="toolbar-primary"><Plus size={13} />{t("music.builder.newPlaylist")}</button>
              <div class="relative"><button type="button" onclick={() => toolbarMenuOpen = !toolbarMenuOpen} class="toolbar-icon" aria-label={t("music.builder.moreActions")}><MoreHorizontal size={15} /></button>{#if toolbarMenuOpen}<div class="toolbar-menu surface-floating surface-floating-body w-floating-sm"><button type="button" class="menu-item" onclick={() => { toolbarMenuOpen = false; interchange.show("import"); }}>{t("music.builder.importPlaylists")}</button><button type="button" class="menu-item" disabled={library.playlistSummaries.length === 0} onclick={() => { toolbarMenuOpen = false; interchange.show("export", null); }}>{t("music.builder.exportPlaylists")}</button></div>{/if}</div>
            {:else if destination.kind === "playlist"}
              {#if library.undoCount > 0}<button type="button" onclick={() => { void library.undoLast(); }} class="toolbar-icon" aria-label={t("music.builder.undo")}><Undo2 size={14} /></button>{/if}
              <button type="button" onclick={() => { void toggleCurrentPlaylist(); }} disabled={!playlistPlaybackAvailable || audition.musicPlayer.youtubePlaybackStarting} class="toolbar-icon" aria-label={audition.musicPlayer.youtubePlaybackStarting ? t("music.builder.loading") : playlistPlaybackPlaying ? t("music.pause") : t("music.play")}>
                {#if audition.musicPlayer.youtubePlaybackStarting}<LoaderCircle size={16} strokeWidth={3} class="animate-spin motion-reduce:animate-none" />{:else if playlistPlaybackPlaying}<Pause size={14} fill="currentColor" />{:else}<Play size={14} />{/if}
              </button>
              <div class="relative"><button type="button" onclick={() => toolbarMenuOpen = !toolbarMenuOpen} class="toolbar-icon" aria-label={t("music.builder.moreActions")}><MoreHorizontal size={15} /></button>{#if toolbarMenuOpen}<div class="toolbar-menu surface-floating surface-floating-body w-floating-sm"><button type="button" class="menu-item" onclick={() => { toolbarMenuOpen = false; playlistSurface = "edit"; }}>{t("music.builder.editPlaylist")}</button><button type="button" class="menu-item" onclick={() => { toolbarMenuOpen = false; playlistSurface = "duplicate"; }}>{t("music.builder.duplicatePlaylist")}</button><button type="button" class="menu-item" onclick={() => { toolbarMenuOpen = false; interchange.show("export", destination.playlistId); }}>{t("music.builder.exportPlaylists")}</button><button type="button" class="menu-item menu-item-destructive" disabled={isSystemMusicPlaylistId(destination.playlistId)} title={isSystemMusicPlaylistId(destination.playlistId) ? t("music.builder.defaultPlaylistDeleteProtected") : undefined} onclick={() => { toolbarMenuOpen = false; void playlist.inspectDelete().then((loaded) => { if (loaded) playlistSurface = "delete"; }); }}>{t("music.builder.deletePlaylist")}</button></div>{/if}</div>
            {:else if destination.kind === "sources"}
              <button type="button" onclick={() => { void toggleCurrentSourceQueue(); }} disabled={!sourcePlaybackAvailable || audition.musicPlayer.youtubePlaybackStarting} class="toolbar-icon" aria-label={audition.musicPlayer.youtubePlaybackStarting ? t("music.builder.loading") : sourcePlaybackPlaying ? t("music.pause") : t("music.play")}>
                {#if audition.musicPlayer.youtubePlaybackStarting}<LoaderCircle size={16} strokeWidth={3} class="animate-spin motion-reduce:animate-none" />{:else if sourcePlaybackPlaying}<Pause size={14} fill="currentColor" />{:else}<Play size={14} />{/if}
              </button>
              <button type="button" onclick={refreshSelectedSource} disabled={sources.collections.length === 0} class="toolbar-secondary"><RefreshCw size={13} />{selectedSourceNode ? t("music.builder.refresh") : t("music.builder.refreshAll")}</button>
              {#if selectedSourceNode?.sourceType === "local" && selectedSourceNode.kind === "group"}
                <button type="button" onclick={() => { void chooseLocalSource(); }} class="toolbar-primary"><Plus size={13} />{t("music.builder.addFolder")}</button>
              {:else if selectedSourceNode?.sourceType === "youtube"}
                <button type="button" onclick={openYouTubeSource} class="toolbar-primary"><Plus size={13} />{t("music.builder.addLink")}</button>
              {/if}
              {#if selectedSourceNode && (selectedSourceNode.kind === "group" || selectedSourceCollection)}
                <div class="relative">
                  <button type="button" onclick={() => toolbarMenuOpen = !toolbarMenuOpen} class="toolbar-icon" aria-label={t("music.builder.moreActions")}><MoreHorizontal size={15} /></button>
                  {#if toolbarMenuOpen}
                    <div class="toolbar-menu surface-floating surface-floating-body w-floating-sm">
                      <button type="button" class="menu-item" onclick={openSourceRename}>{t("music.builder.renameSource")}</button>
                      {#if selectedSourceCollection?.kind === "local-root" && (unlinkedSelectedSources.length > 0 || (library.sourceSummaries.find((summary) => summary.id === selectedSourceCollection.id)?.openIssueCount ?? 0) > 0)}
                        <button type="button" class="menu-item" onclick={() => { toolbarMenuOpen = false; void openRelink(selectedSourceCollection.id); }}>{t("music.builder.repair")}</button>
                      {/if}
                      {#if selectedSourceCollection}<button type="button" class="menu-item menu-item-destructive" onclick={() => { toolbarMenuOpen = false; void openRemoval(selectedSourceCollection.id); }}>{t("music.builder.removeSource")}</button>{/if}
                    </div>
                  {/if}
                </div>
              {/if}
            {:else if destination.kind === "soundscapes"}
              <button type="button" onclick={() => { if (contextViewState.soundscapeFilter === "presets") contextViewState.soundscapeFilter = "layers"; soundscapeAddRequest += 1; }} class="toolbar-primary"><Plus size={13} />{t("music.soundscape.addLoop")}</button>
            {/if}
          {/snippet}
        </MusicBuilderToolbar>
      {/if}
      {#if destination.kind === "review"}
        {#if builderPreparation}
          <div class="relative grid h-full min-h-40 place-items-center overflow-hidden p-5">
            <div class="w-full max-w-lg text-center">
              <MusicPreparationActivity />
              <h1 class="mt-3 text-lg font-semibold tracking-tight">
                {sources.firstUseSession || sources.preparingDefaultFolder
                  ? t("music.builder.preparingMusicFolder")
                  : t("music.builder.loading")}
              </h1>
              {#if sources.preparingDefaultFolderPath}<p class="mx-auto mt-2 max-w-md truncate text-xs text-muted-foreground" title={sources.preparingDefaultFolderPath}>{sources.preparingDefaultFolderPath}</p>{/if}
              {#if firstUseRefreshProgress}
                <div class="mx-auto mt-5 max-w-sm">
                  <div class="h-1 overflow-hidden rounded-full bg-secondary"><div class="h-full rounded-full bg-primary transition-[width] motion-reduce:transition-none" style={`width: ${firstUseRefreshProgress.progress && firstUseRefreshProgress.progress.discoveredCount > 0 ? Math.min(96, Math.max(8, firstUseRefreshProgress.progress.processedCount / firstUseRefreshProgress.progress.discoveredCount * 100)) : 8}%`}></div></div>
                  <p class="mt-2 text-[0.68rem] tabular-nums text-muted-foreground">{t("music.builder.preparingMusicFolderProgress", firstUseRefreshProgress.progress?.processedCount ?? 0, firstUseRefreshProgress.progress?.discoveredCount ?? 0)}</p>
                </div>
              {/if}
            </div>
          </div>
        {:else if firstUseNeedsFolder}
          <div class="relative grid h-full min-h-40 place-items-center overflow-hidden p-5">
            <button type="button" onclick={openPlayerFromBuilder} class="absolute left-3 top-3 grid h-9 w-9 place-items-center rounded-full bg-secondary/75 text-secondary-foreground hover:bg-accent" aria-label={t("music.backToPlayer")}><ArrowLeft size={17} /></button>
            <div class="w-full max-w-sm text-center">
              <button type="button" onclick={() => { void chooseFirstUseFolder(); }} disabled={choosingFirstUseFolder} class="group mx-auto grid h-24 w-24 place-items-center rounded-3xl bg-primary/10 text-primary transition-transform hover:scale-105 active:scale-95 disabled:opacity-50 motion-reduce:transform-none">
                {#if choosingFirstUseFolder}<LoaderCircle class="animate-spin motion-reduce:animate-none" size={32} />{:else}<FolderSearch size={34} strokeWidth={1.35} />{/if}
              </button>
              <h1 class="mt-5 text-lg font-semibold tracking-tight">{t("music.builder.chooseMusicFolder")}</h1>
              {#if firstUseFolderError || sources.defaultFolderPreparationError}<Notice class="mt-3 justify-center text-xs" message={firstUseFolderError || sources.defaultFolderPreparationError || ""} />{/if}
            </div>
          </div>
        {:else if library.error && library.currentWindow.items.length === 0}
          <MusicBuilderAsyncState kind="error" title={library.error.message} onRetry={() => library.refresh()} />
        {:else if library.busy && library.currentWindow.items.length === 0}
          <MusicBuilderAsyncState kind="loading" />
        {:else if library.currentWindow.items.length === 0}
          <div class="grid h-full min-h-40 place-items-center p-5 text-center">
            {#if sources.detectedDefaultFolder}
              <MusicDetectedFolderCard controller={sources} onAdded={detectedFolderAdded} />
            {:else}
              <div class="max-w-sm rounded-2xl border border-border/60 bg-card/55 p-5 shadow-sm">
                <div class="mx-auto grid h-12 w-12 place-items-center rounded-2xl bg-success/12 text-success"><Check size={21} strokeWidth={1.8} aria-hidden="true" /></div>
                <h2 class="mt-3 text-sm font-semibold">{t("music.builder.emptyReviewTitle")}</h2>
                <p class="mt-1.5 text-xs leading-relaxed text-muted-foreground">{t("music.builder.emptyReviewDescription")}</p>
                <div class="mt-4 flex flex-wrap justify-center gap-2">
                  {#if library.currentState.reviewState !== null}
                    <button type="button" onclick={() => { library.patchCurrentState({ reviewState: null }); void library.refresh(); }} class="h-8 rounded-md bg-secondary px-3 text-xs font-medium text-secondary-foreground">{t("music.builder.reviewDeferred")}</button>
                  {/if}
                  <button type="button" onclick={() => navigate({ kind: "sources" })} class="h-8 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground">{t("music.builder.sources")}</button>
                </div>
              </div>
            {/if}
          </div>
        {:else}
          <MusicReviewWorkspace items={reviewItems} totalCount={reviewItems.length} {library} {inspector} {sources} {audition} {review} {bulk} {active} selectedItemIds={reviewTreeViewState.selectedItemIds} selectedFolderIds={reviewTreeViewState.selectedFolderIds} onClearSelection={clearReviewSelection} autoplay={reviewAutoplay} onAutoplayChange={setReviewAutoplay} onOpenPlayer={openPlayerFromBuilder} compactPlayerLabel={mobilePresentation} showPanelButton={layout.contextPanelPresentation === "sheet"} onOpenPanel={() => contextViewState.contextPanelOpen = true} onEditPlaylist={(playlistId) => { void openPlaylistManagementSurface(playlistId, "edit"); }} onDeletePlaylist={(playlistId) => { void openPlaylistManagementSurface(playlistId, "delete"); }} onReorderPlaylists={reorderPlaylistSummaries} issue={activeReviewIssue} repairAvailable={activeReviewIssue ? canRepairIssue(activeReviewIssue) : false} onRepairIssue={repairIssue} onChooseFolder={chooseRootFolder} bind:viewState={reviewWorkspaceViewState} />
        {/if}
      {:else if playlistManagementOpen && (destination.kind === "playlists" || destination.kind === "playlist")}
        <div class="min-h-0 flex-1 overflow-y-auto p-3" data-music-scrollable="true"><MusicPlaylistManager bind:this={playlistManager} bind:actionsDisabled={playlistManagerActionsDisabled} playlists={library.playlistSummaries} onEdit={(playlistId) => { void openPlaylistManagementSurface(playlistId, "edit"); }} onDelete={(playlistId) => { void openPlaylistManagementSurface(playlistId, "delete"); }} onReorder={reorderPlaylistSummaries} onDone={() => playlistManagementOpen = false} showHeader={false} /></div>
      {:else if isPlaylistDestination}
        {#if destination.kind === "playlist" && playlist.detail && playlist.playbackIssue === "no-eligible-items"}
              <div class="mx-3 mt-2 flex flex-wrap items-center gap-2 rounded-lg border border-warning/30 bg-warning/8 px-3 py-2 text-[0.68rem] text-warning" role="status">
                <span class="min-w-0 flex-1">{t("music.builder.noEligiblePlaylistItems")}</span>
                {#if issueCount > 0}<button type="button" onclick={() => { void openReviewIssues(); }} class="h-7 rounded-md bg-secondary px-2.5 font-medium text-secondary-foreground">{t("music.builder.openIssues")}</button>{/if}
              </div>
        {/if}
        {#if destination.kind === "playlist"}
        <MusicBuilderFilterBar
          sourceKind={library.currentState.sourceKind}
          availability={library.currentState.availability}
          sort={library.currentState.sort}
          direction={library.currentState.direction}
          resultCount={library.currentWindow.totalCount}
          snoozed={library.currentState.snoozed}
          onChange={patchFilters}
        />
        {/if}
        {#if library.error && library.currentWindow.items.length === 0}
          <MusicBuilderAsyncState kind="error" title={library.error.message} onRetry={() => library.refresh()} />
        {:else if library.busy && library.currentWindow.items.length === 0}
          <MusicBuilderAsyncState kind="loading" />
        {:else if library.currentWindow.items.length === 0}
          {#if destination.kind === "playlist" && (library.playlistSummaries.find((entry) => entry.id === destination.playlistId)?.totalCount ?? 0) === 0}
            <div class="grid min-h-0 flex-1 place-items-center p-4 text-center">
              <div class="max-w-sm rounded-2xl border border-border/60 bg-card/55 p-5 shadow-sm">
                <h2 class="text-sm font-semibold">{t("music.builder.emptyPlaylistActionTitle")}</h2>
                <p class="mt-1.5 text-xs leading-relaxed text-muted-foreground">{t("music.builder.emptyPlaylistActionDescription")}</p>
                <div class="mt-4 flex flex-wrap justify-center gap-2"><button type="button" onclick={() => { void navigate({ kind: "review" }); }} class="h-8 rounded-md bg-primary px-3 text-xs font-semibold text-primary-foreground">{t("music.builder.startReview")}</button></div>
              </div>
            </div>
          {:else}
            <MusicBuilderAsyncState
              kind="empty"
              title={t("music.builder.noPlaylistFilterResults")}
              description={t("music.builder.adjustPlaylistFilters")}
            />
          {/if}
        {:else}
          {#if library.busy}<div class="absolute inset-x-0 top-10 z-10 bg-secondary/90 px-3 py-1 text-center text-[0.62rem] text-muted-foreground backdrop-blur-sm">{t("music.builder.staleData")}</div>{/if}
          <MusicVirtualItemList
            items={library.currentWindow.items}
            bindings={sources.bindings}
            playlistName={activePlaylistSummary ? systemMusicPlaylistName(activePlaylistSummary.id, activePlaylistSummary.name, t) : playlist.detail?.name ?? ""}
            playingItemId={activePlaylistInPlayer ? playingItemId : null}
            startingItemId={pendingPlaylistItemId ?? (activePlaylistInPlayer && audition.musicPlayer.youtubePlaybackStarting ? playingItemId : null)}
            playbackActive={activePlaylistPlaying}
            initialScrollTop={library.currentState.scrollTop}
            onScrollTop={(scrollTop) => library.setScrollTop(scrollTop)}
            onTogglePlayback={(item) => { void togglePlaylistItem(item); }}
            onShowLocation={showItemLocation}
            showLocationAction={supportsLocalFileReveal}
            onSnooze={snoozePlaylistItem}
            onRemoveSnooze={(item) => removeItemSnoozes(item, destination.kind === "playlist" ? destination.playlistId : null)}
            onWeight={setPlaylistItemWeight}
            onRemove={removePlaylistItem}
            hasMore={library.currentWindow.items.length < library.currentWindow.totalCount}
            loadingMore={library.loadingMore}
            onLoadMore={() => { void library.loadMore(); }}
          />
        {/if}
      {:else if destination.kind === "sources"}
        <MusicSourcesBrowser
          nodes={sourceBrowserNodes}
          selectedNode={selectedSourceNode}
          selectedPath={selectedSourcePath}
          bindings={sources.bindings}
          unlinkedSources={unlinkedSelectedSources}
          onChooseFolder={(collectionId) => { void openRelink(collectionId); }}
          playingItemId={activeSourceQueueInPlayer ? playingItemId : null}
          startingItemId={pendingSourceItemId ?? (activeSourceQueueInPlayer && audition.musicPlayer.youtubePlaybackStarting ? playingItemId : null)}
          playbackActive={activeSourceQueuePlaying}
          showLocationAction={supportsLocalFileReveal}
          onSelect={(nodeId) => contextViewState.selectedSourceId = nodeId}
          onTogglePlayback={(item) => { void toggleSourceItem(item); }}
          onShowLocation={showItemLocation}
          onSnooze={async (item, duration) => snoozeSourceItem(item, duration)}
          onRemoveSnooze={(item) => removeItemSnoozes(item, null)}
        />
      {:else if destination.kind === "soundscapes"}
        <MusicSoundscapeBuilder filter={contextViewState.soundscapeFilter} addRequest={soundscapeAddRequest} onPlaybackStart={takePlaybackOwnership} />
      {:else}
        <MusicBuilderOverview {destination} search={library.currentState.search} playlists={library.playlistSummaries} onNavigate={(next) => { void navigate(next); }} onPrimary={primaryAction} onImport={() => interchange.show("import")} onExport={() => interchange.show("export", destination.kind === "playlist" ? destination.playlistId : null)} compact />
      {/if}
    </main>

    {#if layout.dockPresentation === "bottom" && !builderPreparation && !firstUseNeedsFolder}
      <div class:builder-controls-locked={playlistManagementActive} class="builder-mobile-dock" inert={playlistManagementActive} aria-disabled={playlistManagementActive}><MusicBuilderDock {destination} {reviewCount} compact showAllLabels={mobilePresentation} includeSoundscapes={supportsSoundscapes} onNavigate={navigate} /></div>
    {/if}

    {#if addSourceKind}
      <MusicAddSourceDialog controller={sources} kind={addSourceKind} localSource={localSourceSelection} onClose={closeSourceSurface} onSaved={() => { closeSourceSurface(); void library.refreshAfterMutation(); }} />
    {:else if sourceRenameNode}
      <MusicSourceNameDialog currentName={sourceRenameNode.name} saving={sourceRenameSaving} error={sourceRenameError} onClose={() => sourceRenameNodeId = null} onSave={(name) => { void saveSourceName(name); }} />
    {:else if sourceSurface === "relink" && sourceSurfaceCollection}
      <MusicRelinkWizard controller={sources} collection={sourceSurfaceCollection} onClose={closeSourceSurface} onApplied={() => { sourceSurface = null; sourceSurfaceCollection = null; void library.refreshAfterMutation(); }} />
    {:else if sourceSurface === "remove" && sourceSurfaceCollection}
      <MusicSourceRemovalDialog controller={sources} collection={sourceSurfaceCollection} onClose={closeSourceSurface} onRemoved={() => { sourceSurface = null; sourceSurfaceCollection = null; void library.refreshAfterMutation(); }} />
    {:else if sourceSurface === "item-repair" && repairItemId}
      <MusicItemRepairDialog controller={sources} itemId={repairItemId} onClose={closeSourceSurface} onRepaired={() => { void library.refreshAfterMutation(); void inspector.select(repairItemId); void playlist.refreshActivePlayback(sources.bindings); }} />
    {/if}
    {#if pendingRefreshPlan}
      <MusicNetworkRefreshDialog onlineCount={pendingRefreshPlan.onlineCount} onClose={() => pendingRefreshPlan = null} onLocalOnly={() => { void runSourceRefresh(pendingRefreshPlan!, false); }} onContinue={() => { void runSourceRefresh(pendingRefreshPlan!, true); }} />
    {/if}
    {#if playlistSurface}
      <MusicPlaylistDialog
        controller={playlist}
        mode={playlistSurface}
        playlists={library.playlistSummaries}
        activeInPlayer={Boolean(playlist.detail && audition.musicPlayer.activePlaylistId === playlist.detail.id)}
        onClose={() => { playlistSurface = null; playlistSurfaceReturnsToCurrentView = false; playlistSurfaceTargetId = null; }}
        onSaved={(playlistId) => {
          const returnToCurrentView = playlistSurfaceReturnsToCurrentView;
          playlistSurface = null;
          playlistSurfaceReturnsToCurrentView = false;
          playlistSurfaceTargetId = null;
          if (!returnToCurrentView) void navigateNow({ kind: "playlist", playlistId });
        }}
        onDeleted={(replacementPlaylistId) => {
          const returnToCurrentView = playlistSurfaceReturnsToCurrentView;
          const deletedPlaylistId = playlistSurfaceTargetId ?? (destination.kind === "playlist" ? destination.playlistId : null);
          playlistSurface = null;
          playlistSurfaceReturnsToCurrentView = false;
          playlistSurfaceTargetId = null;
          if (deletedPlaylistId && audition.musicPlayer.activePlaylistId === deletedPlaylistId) {
            audition.musicPlayer.detachDeletedPlaylist(deletedPlaylistId);
            if (replacementPlaylistId) {
              void playReplacementPlaylist(replacementPlaylistId, !returnToCurrentView);
              return;
            }
          }
          if (returnToCurrentView) return;
          const neighboringPlaylistId = replacementPlaylistId ?? library.playlistSummaries[0]?.id ?? null;
          if (neighboringPlaylistId) void navigateNow({ kind: "playlist", playlistId: neighboringPlaylistId });
          else void navigateNow({ kind: "playlists" });
        }}
      />
    {/if}
    {#if interchange.open}<MusicInterchangeDialog controller={interchange} playlists={library.playlistSummaries} onClose={() => interchange.close()} onImported={() => { void library.refreshAfterMutation(); void sources.load(); }} />{/if}
  </div>
</section>

<style>
  .builder-root { container-type: size; }
  .builder-root :global(input),
  .builder-root :global(textarea),
  .builder-root :global([contenteditable="true"]) { user-select: text; }
  .builder-context-panel { grid-column: 1; border-right: 1px solid color-mix(in srgb, var(--border) 46%, transparent); }
  .builder-controls-locked :global(button),
  .builder-controls-locked :global(input),
  .builder-controls-locked :global([role="button"]) { cursor: not-allowed !important; opacity: 0.38; }
  .builder-shell > main { grid-column: 2; }
  .builder-wide, .builder-medium { grid-template-columns: minmax(14rem, 0.72fr) minmax(22rem, 2fr); }
  .builder-contextless { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); }
  .builder-contextless > main { grid-column: 1; grid-row: 1; }
  .builder-narrow { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr) auto; }
  .builder-narrow > main { grid-column: 1; grid-row: 1; }
  .builder-narrow .builder-context-panel { position: absolute; inset: 0 0 2.75rem; grid-column: 1; border-right: 0; background: var(--background); transform: translateX(-102%); transition: transform 150ms ease; }
  .builder-narrow .builder-context-panel.context-open { transform: translateX(0); }
  .builder-mobile-dock { z-index: 25; grid-column: 1; grid-row: 2; background: color-mix(in srgb, var(--background) 94%, transparent); }
  :global(.toolbar-primary), :global(.toolbar-secondary) { display: inline-flex; height: 2rem; flex: none; align-items: center; justify-content: center; gap: 0.35rem; border-radius: 999px; padding-inline: 0.75rem; font-size: calc(0.65rem * var(--type-scale)); font-weight: 600; white-space: nowrap; }
  :global(.toolbar-primary) { background: var(--primary); color: var(--primary-foreground); }
  :global(.toolbar-secondary) { background: var(--secondary); color: var(--secondary-foreground); }
  :global(.toolbar-primary:disabled), :global(.toolbar-secondary:disabled) { opacity: 0.4; }
  :global(.toolbar-icon) { display: grid; height: 2rem; width: 2rem; flex: none; place-items: center; border-radius: 999px; color: var(--foreground); }
  :global(.toolbar-icon:hover:not(:disabled)) { background: var(--secondary); }
  :global(.toolbar-icon:disabled) { cursor: default; opacity: 0.32; }
  :global(.toolbar-menu) { position: absolute; right: 0; top: calc(100% + 0.3rem); z-index: 55; }
  @container (width < 520px) { :global(.toolbar-primary), :global(.toolbar-secondary) { width: 2rem; gap: 0; padding-inline: 0; font-size: 0; } }
  @media (prefers-reduced-motion: reduce) { :global(.builder-root *) { scroll-behavior: auto; } }
  @media (prefers-reduced-motion: reduce) { .builder-narrow .builder-context-panel { transition: none; } }
</style>
