<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import AlertCircle from "@lucide/svelte/icons/alert-circle";
  import ListMusic from "@lucide/svelte/icons/list-music";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import CalendarClock from "@lucide/svelte/icons/calendar-clock";
  import Pause from "@lucide/svelte/icons/pause";
  import Play from "@lucide/svelte/icons/play";
  import SkipBack from "@lucide/svelte/icons/skip-back";
  import SkipForward from "@lucide/svelte/icons/skip-forward";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import MusicPlaylistLauncher from "./MusicPlaylistLauncher.svelte";
  import MusicCurrentItemMenu from "./MusicCurrentItemMenu.svelte";
  import MusicTrackPreferences from "./MusicTrackPreferences.svelte";
  import MusicSnoozeButton from "./MusicSnoozeButton.svelte";
  import MusicPlaybackModeControl from "./MusicPlaybackModeControl.svelte";
  import MusicSoundscapeControl from "$lib/components/music/soundscape/MusicSoundscapeControl.svelte";
  import MusicPreparationActivity from "$lib/components/music/builder/MusicPreparationActivity.svelte";
  import { revealLocalFile } from "$lib/api/music";
  import { getMusicInspectorDetail, removeMusicSnooze } from "$lib/api/music-library";
  import { notifyMusicLibraryChanged } from "$lib/music/library/events";
  import { formatPlaybackTime } from "$lib/music/playback";
  import { fittedSidePlaylistPanelHeight } from "$lib/music/panel-layout";
  import {
    MUSIC_PLAYLIST_ROW_HEIGHT_PX,
    musicPlaylistWindow,
  } from "$lib/music/playlists/window";
  import { getMusicPlayer } from "$lib/stores/music-player.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { cn } from "$lib/utils";
  import { formatShortcut, hasShortcutModifier } from "$lib/keyboard-shortcuts";
  import { captureFocusReturn } from "$lib/modal-focus";
  import {
    musicBuilderLoader,
    type MusicBuilderComponent,
    type MusicBuilderInitialAction,
  } from "$lib/music/builder/loader";
  import { getMusicSourcesController } from "$lib/music/sources/controller.svelte";

  let {
    onClose,
    visible = true,
    presentation = "desktop",
    mobilePlayerPanelStyle = "",
    mobilePlaylistPanelStyle = "",
  }: {
    onClose: () => void;
    visible?: boolean;
    presentation?: "desktop" | "mobile";
    mobilePlayerPanelStyle?: string;
    mobilePlaylistPanelStyle?: string;
  } = $props();

  const player = getMusicPlayer();
  const sources = getMusicSourcesController();
  const { t } = getLocalization();
  const supportsLocalFileReveal = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.local-file-reveal");
  const supportsSoundscapes = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.soundscapes");

  type MusicPage = "player" | "playlist-builder";

  let musicHeader = $state<HTMLElement | null>(null);
  let mediaSurface = $state<HTMLElement | null>(null);
  let mediaCell = $state<HTMLElement | null>(null);
  let playlistPanel = $state<HTMLElement | null>(null);
  let playbackControls = $state<HTMLElement | null>(null);
  let playlistScrollContainer = $state<HTMLElement | undefined>();
  let volumeMenuRoot = $state<HTMLElement | null>(null);
  let volumeMenuOpen = $state(false);
  const playlistVisible = $derived(player.playlistVisible);
  const playlistHasContent = $derived(player.queue.length > 0);
  let musicPage = $state<MusicPage>(sources.firstUseSession ? "playlist-builder" : "player");
  let firstUseRedirectHandled = $state(sources.firstUseSession);
  let playlistBuilderComponent = $state<MusicBuilderComponent | null>(musicBuilderLoader.peek());
  let playlistBuilderLoading = $state(false);
  let playlistBuilderLoadError = $state<string | null>(null);
  let playlistBuilderMounted = $state(sources.firstUseSession);
  let playlistBuilderInitialAction = $state<MusicBuilderInitialAction | null>(null);
  const PlaylistBuilder = $derived(playlistBuilderComponent);
  let mediaSurfaceFullscreen = $state(false);
  let volumeFeedbackVisible = $state(false);
  let mediaSurfaceClickTimeoutId: number | null = null;
  let volumeFeedbackTimeoutId: number | null = null;
  let lastVolumeFeedbackId = 0;
  let playlistAutoScrollActive = false;
  let lastPlaylistAutoScrollIndex = -1;
  let lastPlaylistAutoScrollIdentity: string | null = null;
  let playlistScrollTop = $state(0);
  let playlistViewportHeight = $state(0);
  let mediaTitleMeasuredCenterPx = $state<number | null>(null);
  let panel = $state<HTMLElement | null>(null);
  let fittedPanelHeightPx = $state<number | null>(null);
  let returnFocus: ReturnType<typeof captureFocusReturn> | null = null;
  let previouslyVisible = false;

  const mediaSurfaceFullscreenEvent = "ganbaru-ai-music-media-surface-fullscreen";
  const volumeMax = $derived(player.volumeMax);
  const volumeSliderProgress = $derived(volumeMax > 0
    ? `${Math.min(100, Math.max(0, (player.volumeControlValue / volumeMax) * 100))}%`
    : "0%");
  const seekSliderProgress = $derived(player.progressMax > 0
    ? `${Math.min(100, Math.max(0, (player.progressValue / player.progressMax) * 100))}%`
    : "0%");
  const playerPlaylistLayoutVisible = $derived(
    musicPage === "player" && playlistVisible,
  );
  const topBarMediaTitleMaxLength = 42;
  const volumeShortcutStep = 0.05;
  const topBarMediaTitle = $derived(
    player.currentSource
      ? truncateTopBarMediaTitle(
        mediaTitleWithoutExtension(player.loadedTitle, player.currentSource.kind === "local-file"),
      )
      : "",
  );
  const mediaTitleLeft = $derived(
    playlistVisible && mediaTitleMeasuredCenterPx !== null ? `${mediaTitleMeasuredCenterPx}px` : "50%",
  );
  const musicIconSize = 14;
  const musicIconStrokeWidth = 1.4;
  const panelMaximumHeight = $derived(
    playerPlaylistLayoutVisible && fittedPanelHeightPx !== null
      ? `${fittedPanelHeightPx}px`
      : "680px",
  );
  const mobilePresentation = $derived(presentation === "mobile");
  const mobileBuilderPresentation = $derived(
    mobilePresentation && musicPage === "playlist-builder",
  );
  const mobileBuilderPanelStyle = "left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: var(--safe-area-top) var(--safe-area-right) var(--safe-area-bottom) var(--safe-area-left);";
  const mobilePlayerFallbackStyle = "left:calc(var(--visual-viewport-offset-left) + var(--safe-area-left) + 0.5rem);top:calc(var(--visual-viewport-offset-top) + var(--safe-area-top) + var(--mobile-topbar-h) + 0.25rem);width:calc(var(--visual-viewport-width) - var(--safe-area-left) - var(--safe-area-right) - 1rem);height:calc(var(--visual-viewport-height) - var(--safe-area-top) - var(--safe-area-bottom) - var(--mobile-topbar-h) - 0.75rem);";
  const desktopPanelStyle = $derived(`top: calc(var(--titlebar-h) + 4px); height: min(${panelMaximumHeight}, calc(100dvh - var(--titlebar-h) - 12px));`);
  const renderedPlaylistWindow = $derived(musicPlaylistWindow(
    player.queue.length,
    playlistScrollTop,
    playlistViewportHeight,
  ));
  const renderedPlaylistItems = $derived(
    player.queue.slice(renderedPlaylistWindow.startIndex, renderedPlaylistWindow.endIndex),
  );
  const savedQueueSkippedCount = $derived(Object.values(player.savedQueueSkipBreakdown).reduce((total, count) => total + count, 0));
  const savedQueueUnavailable = $derived(Boolean(
    player.activePlaylistId
    && !player.currentSource
    && savedQueueSkippedCount > 0
    && player.contextPlayback?.state !== "unavailable",
  ));
  const savedQueueOfflineSubset = $derived(Boolean(player.activePlaylistId && !player.online && player.savedQueueSkipBreakdown.offline > 0 && player.currentSource));
  const visibleContext = $derived(player.contextPlayback?.state === "overridden" ? null : player.contextPlayback);
  const contextPhaseLabel = $derived(visibleContext ? t(`music.assignment.phase.${visibleContext.phase}`) : "");
  const contextSummary = $derived(visibleContext
    ? t("music.assignment.context.selectedBy", contextPhaseLabel, visibleContext.eventTitle)
    : "");

  $effect(() => {
    if (!sources.firstUseSession || firstUseRedirectHandled) return;
    firstUseRedirectHandled = true;
    playlistBuilderMounted = true;
    musicPage = "playlist-builder";
    void loadPlaylistBuilder();
  });

  $effect(() => {
    if (musicPage === "playlist-builder") void loadPlaylistBuilder();
  });

  $effect(() => {
    const surface = mediaSurface;
    if (!visible || !surface) return;
    return player.claimSurface("music-panel", surface);
  });

  $effect(() => {
    const header = musicHeader;
    const surface = mediaSurface;
    const playlistLayoutVisible = playlistVisible;

    if (typeof window === "undefined" || typeof ResizeObserver === "undefined" || !header || !surface) {
      mediaTitleMeasuredCenterPx = null;
      return;
    }

    let animationFrameId: number | null = null;

    const updateTitleCenter = () => {
      const headerRect = header.getBoundingClientRect();
      const surfaceRect = surface.getBoundingClientRect();
      if (headerRect.width <= 0 || surfaceRect.width <= 0) {
        mediaTitleMeasuredCenterPx = null;
        return;
      }
      mediaTitleMeasuredCenterPx = Math.round(surfaceRect.left - headerRect.left + surfaceRect.width / 2);
    };

    const requestTitleCenterUpdate = () => {
      if (animationFrameId !== null) {
        window.cancelAnimationFrame(animationFrameId);
      }
      animationFrameId = window.requestAnimationFrame(() => {
        animationFrameId = null;
        updateTitleCenter();
      });
    };

    updateTitleCenter();
    void tick().then(() => {
      if (playlistLayoutVisible === playlistVisible) {
        requestTitleCenterUpdate();
      }
    });

    const observer = new ResizeObserver(requestTitleCenterUpdate);
    observer.observe(header);
    observer.observe(surface);
    window.addEventListener("resize", requestTitleCenterUpdate);

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", requestTitleCenterUpdate);
      if (animationFrameId !== null) {
        window.cancelAnimationFrame(animationFrameId);
      }
    };
  });

  $effect(() => {
    const page = musicPage;
    const panelIsVisible = visible;
    const playlistIsVisible = playlistVisible;
    const header = musicHeader;
    const media = mediaCell;
    const playlist = playlistPanel;
    const controls = playbackControls;
    if (!playlistIsVisible) {
      fittedPanelHeightPx = null;
      return;
    }
    if (!panelIsVisible || page !== "player" || !header || !media || !playlist || !controls) return;

    let animationFrameId: number | null = null;
    const updateHeight = () => {
      const mediaRect = media.getBoundingClientRect();
      const playlistRect = playlist.getBoundingClientRect();
      fittedPanelHeightPx = fittedSidePlaylistPanelHeight({
        mediaWidth: mediaRect.width,
        headerHeight: header.getBoundingClientRect().height,
        controlsHeight: controls.getBoundingClientRect().height,
        sideBySide: Math.abs(mediaRect.top - playlistRect.top) < 1
          && playlistRect.left >= mediaRect.right - 1,
      });
    };
    const requestHeightUpdate = () => {
      if (animationFrameId !== null) window.cancelAnimationFrame(animationFrameId);
      animationFrameId = window.requestAnimationFrame(() => {
        animationFrameId = null;
        updateHeight();
      });
    };

    updateHeight();
    const observer = new ResizeObserver(requestHeightUpdate);
    observer.observe(header);
    observer.observe(media);
    observer.observe(playlist);
    observer.observe(controls);
    window.addEventListener("resize", requestHeightUpdate);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", requestHeightUpdate);
      if (animationFrameId !== null) window.cancelAnimationFrame(animationFrameId);
    };
  });

  onDestroy(() => {
    clearMediaSurfaceClickTimeout();
    clearVolumeFeedbackTimeout();
  });

  $effect(() => {
    if (visible && !previouslyVisible) {
      returnFocus = captureFocusReturn();
      void tick().then(() => {
        if (visible) panel?.focus();
      });
    } else if (!visible && previouslyVisible) {
      returnFocus?.(panel);
      returnFocus = null;
    }
    previouslyVisible = visible;
  });

  $effect(() => {
    if (visible) return;
    volumeMenuOpen = false;
  });

  $effect(() => {
    if (typeof document === "undefined") return;
    const updateFullscreenState = () => {
      mediaSurfaceFullscreen = Boolean(mediaSurface && document.fullscreenElement === mediaSurface);
    };
    updateFullscreenState();
    document.addEventListener("fullscreenchange", updateFullscreenState);
    return () => {
      document.removeEventListener("fullscreenchange", updateFullscreenState);
    };
  });

  $effect(() => {
    const feedbackId = player.volumeFeedbackId;
    if (feedbackId === lastVolumeFeedbackId) return;
    lastVolumeFeedbackId = feedbackId;
    if (!mediaSurfaceFullscreen) return;
    showVolumeFeedback();
  });

  $effect(() => {
    const visible = playlistVisible;
    const container = playlistScrollContainer;
    const index = player.highlightedQueueIndex;
    const identity = index >= 0 ? player.queue[index]?.identity ?? null : null;

    if (!visible) {
      playlistAutoScrollActive = false;
      lastPlaylistAutoScrollIndex = index;
      lastPlaylistAutoScrollIdentity = identity;
      return;
    }

    if (!container) return;

    const opened = !playlistAutoScrollActive;
    playlistAutoScrollActive = true;

    if (index < 0 || identity === null) {
      lastPlaylistAutoScrollIndex = index;
      lastPlaylistAutoScrollIdentity = identity;
      return;
    }

    if (opened || index !== lastPlaylistAutoScrollIndex || identity !== lastPlaylistAutoScrollIdentity) {
      scrollPlaylistItemIntoView(index, opened ? "center" : "nearest");
    }

    lastPlaylistAutoScrollIndex = index;
    lastPlaylistAutoScrollIdentity = identity;
  });

  function togglePlaylist(): void {
    player.setPlaylistVisible(!playlistVisible);
  }

  function queueTitleTooltip(node: HTMLElement, _title: string): {
    update: (title: string) => void;
    destroy: () => void;
  } {
    const button = node.parentElement?.previousElementSibling;
    const measure = () => {
      if (!(button instanceof HTMLButtonElement) || !node.isConnected) return;
      button.dataset.appTooltipDisabled = String(node.scrollWidth <= node.clientWidth + 1);
    };
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    queueMicrotask(measure);
    return {
      update: () => queueMicrotask(measure),
      destroy: () => observer.disconnect(),
    };
  }

  async function removeQueueSnooze(index: number): Promise<void> {
    const itemId = player.activeQueueItemIds[index];
    if (!itemId) return;
    const playlistId = player.activePlaylistId;
    const now = Date.now();
    const activeSnoozes = (await getMusicInspectorDetail(itemId)).snoozes.filter((entry) =>
      entry.startsAtMs <= now && (entry.endsAtMs === null || entry.endsAtMs > now)
      && (playlistId === null || entry.scope === "all-playlists" || entry.playlistId === playlistId));
    await Promise.all(activeSnoozes.map((entry) => removeMusicSnooze(entry.id)));
    if (player.activeQueueItemIds[index] === itemId && player.activePlaylistId === playlistId) {
      player.clearQueueItemSnooze(index);
    }
    notifyMusicLibraryChanged();
  }

  async function loadPlaylistBuilder(): Promise<void> {
    if (playlistBuilderComponent || playlistBuilderLoading) return;
    playlistBuilderLoading = true;
    playlistBuilderLoadError = null;
    try {
      playlistBuilderComponent = await musicBuilderLoader.load();
    } catch (error) {
      playlistBuilderLoadError = error instanceof Error ? error.message : String(error);
    } finally {
      playlistBuilderLoading = false;
    }
  }

  function openPlaylistBuilder(initialAction: MusicBuilderInitialAction | null = null): void {
    closeVolumeMenu();
    playlistBuilderInitialAction = initialAction;
    playlistBuilderMounted = true;
    musicPage = "playlist-builder";
    void loadPlaylistBuilder();
  }

  function closePlaylistBuilder(): void {
    musicPage = "player";
  }

  async function openCurrentLocalFileLocation(): Promise<void> {
    const source = player.currentSource;
    if (source?.kind !== "local-file") return;
    try {
      await revealLocalFile(source.path);
    } catch (error) {
      player.playerError = error instanceof Error ? error.message : String(error);
    }
  }

  function digitSeekShortcut(event: KeyboardEvent): number | null {
    if (event.shiftKey) return null;
    if (/^[0-9]$/.test(event.key)) return Number(event.key);
    if (/^Numpad[0-9]$/.test(event.code)) return Number(event.code.slice("Numpad".length));
    return null;
  }

  function seekToDigitPosition(digit: number): void {
    const durationMs = player.snapshot.durationMs;
    if (!player.currentSource || durationMs === null || durationMs <= 0) return;
    void player.seekToMs(Math.round((durationMs * digit) / 10));
  }

  function snappedVolume(value: number): number {
    if (!Number.isFinite(value)) return player.volumeControlValue;
    return Number((Math.round(value / volumeShortcutStep) * volumeShortcutStep).toFixed(2));
  }

  function setVolumeFromControl(value: number): void {
    void player.setVolume(snappedVolume(value));
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (!visible) return;
    if (event.target instanceof HTMLElement && event.target.closest("[data-music-track-preferences-open]")) return;
    if (event.key === "Escape" && !mediaSurfaceFullscreen) {
      if (musicPage === "playlist-builder") return;
      if (typeof document !== "undefined" && document.querySelector("[data-app-floating-surface], [data-music-track-preferences-open]")) return;
      claimKeyboardShortcut(event);
      onClose();
      return;
    }
    if (event.key === "Tab") {
      trapPanelFocus(event);
      return;
    }
    if (musicPage !== "player") return;
    if (event.altKey) return;
    if (isEditableTarget(event.target)) return;
    const shortcutModifier = hasShortcutModifier(event);
    if ((event.ctrlKey || event.metaKey) && !shortcutModifier) return;
    if (shortcutModifier) {
      if (event.key === "ArrowLeft") {
        claimKeyboardShortcut(event);
        void player.playPreviousTrack();
        return;
      }
      if (event.key === "ArrowRight") {
        claimKeyboardShortcut(event);
        void player.playNextTrack();
        return;
      }
      if (!event.shiftKey && (event.key.toLowerCase() === "l" || event.key.toLowerCase() === "p")) {
        claimKeyboardShortcut(event);
        togglePlaylist();
      }
      return;
    }
    if (event.code === "Space") {
      claimKeyboardShortcut(event);
      void player.togglePlay();
      return;
    }
    const seekDigit = digitSeekShortcut(event);
    if (seekDigit !== null) {
      claimKeyboardShortcut(event);
      seekToDigitPosition(seekDigit);
      return;
    }
    if (event.key.toLowerCase() === "p" || event.key.toLowerCase() === "l") {
      claimKeyboardShortcut(event);
      togglePlaylist();
      return;
    }
    if (event.key.toLowerCase() === "m") {
      claimKeyboardShortcut(event);
      void player.toggleMute();
      return;
    }
    if (event.shiftKey && event.key === "ArrowLeft") {
      claimKeyboardShortcut(event);
      void player.playPreviousTrack();
      return;
    }
    if (event.shiftKey && event.key === "ArrowRight") {
      claimKeyboardShortcut(event);
      void player.playNextTrack();
      return;
    }
    if (event.key === "ArrowLeft") {
      claimKeyboardShortcut(event);
      void player.seekByMs(-10_000);
      return;
    }
    if (event.key === "ArrowRight") {
      claimKeyboardShortcut(event);
      void player.seekByMs(10_000);
      return;
    }
    if (event.key === "ArrowUp") {
      claimKeyboardShortcut(event);
      void player.adjustVolume(volumeShortcutStep);
      return;
    }
    if (event.key === "ArrowDown") {
      claimKeyboardShortcut(event);
      void player.adjustVolume(-volumeShortcutStep);
      return;
    }
    if (event.key.toLowerCase() === "s" || event.key.toLowerCase() === "r") {
      claimKeyboardShortcut(event);
      if (player.queue.length > 0) player.setPlaybackMode(player.playbackMode === "shuffle" ? "in-order" : "shuffle");
      return;
    }
  }

  function claimKeyboardShortcut(event: KeyboardEvent): void {
    event.preventDefault();
    event.stopPropagation();
    event.stopImmediatePropagation();
  }

  function trapPanelFocus(event: KeyboardEvent): void {
    if (!panel) return;
    const focusable = [...panel.querySelectorAll<HTMLElement>(
      "button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex='-1'])",
    )].filter((element) => element.offsetParent !== null);
    const first = focusable[0];
    const last = focusable.at(-1);
    if (!first || !last) return;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function isEditableTarget(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    return Boolean(target.closest("input, textarea, select, [contenteditable='true']"));
  }

  function handleWindowPointerDown(event: PointerEvent): void {
    if (!visible) return;
    if (!(event.target instanceof Node)) return;
    if (volumeMenuOpen && volumeMenuRoot && !volumeMenuRoot.contains(event.target)) {
      closeVolumeMenu();
    }
  }

  function toggleVolumeMenu(): void {
    volumeMenuOpen = !volumeMenuOpen;
  }

  function closeVolumeMenu(): void {
    volumeMenuOpen = false;
  }


  function handleMediaSurfaceClick(event: MouseEvent): void {
    event.preventDefault();
    if (event.button !== 0) return;
    if (event.detail > 1) return;
    clearMediaSurfaceClickTimeout();
    mediaSurfaceClickTimeoutId = window.setTimeout(() => {
      mediaSurfaceClickTimeoutId = null;
      void player.togglePlay();
    }, 250);
  }

  function handleMediaSurfaceDoubleClick(event: MouseEvent): void {
    event.preventDefault();
    if (event.button !== 0) return;
    clearMediaSurfaceClickTimeout();
    window.dispatchEvent(new CustomEvent(mediaSurfaceFullscreenEvent));
  }

  function clearMediaSurfaceClickTimeout(): void {
    if (mediaSurfaceClickTimeoutId === null) return;
    window.clearTimeout(mediaSurfaceClickTimeoutId);
    mediaSurfaceClickTimeoutId = null;
  }

  function clearVolumeFeedbackTimeout(): void {
    if (volumeFeedbackTimeoutId === null) return;
    window.clearTimeout(volumeFeedbackTimeoutId);
    volumeFeedbackTimeoutId = null;
  }

  function releaseRangeFocus(event: Event): void {
    if (event.currentTarget instanceof HTMLElement) {
      event.currentTarget.blur();
    }
  }

  function releaseClickedButtonFocus(event: PointerEvent): void {
    if (!(event.target instanceof HTMLElement)) return;
    const button = event.target.closest("button");
    if (!button) return;
    window.setTimeout(() => button.blur(), 0);
  }

  function releaseClickedButtonFocusAction(node: HTMLElement): { destroy: () => void } {
    node.addEventListener("pointerup", releaseClickedButtonFocus);
    return {
      destroy: () => node.removeEventListener("pointerup", releaseClickedButtonFocus),
    };
  }

  function mediaTitleWithoutExtension(title: string, localFile: boolean): string {
    const trimmed = title.trim();
    if (!localFile) return trimmed;
    return trimmed.replace(/\.[A-Za-z0-9]{1,8}$/, "");
  }

  function truncateTopBarMediaTitle(title: string): string {
    if (title.length <= topBarMediaTitleMaxLength) return title;
    return `${title.slice(0, topBarMediaTitleMaxLength - 3).trimEnd()}...`;
  }

  function showVolumeFeedback(): void {
    clearVolumeFeedbackTimeout();
    volumeFeedbackVisible = true;
    volumeFeedbackTimeoutId = window.setTimeout(() => {
      volumeFeedbackVisible = false;
      volumeFeedbackTimeoutId = null;
    }, 900);
  }

  function scrollPlaylistItemIntoView(index: number, block: "center" | "nearest"): void {
    const container = playlistScrollContainer;
    if (!playlistVisible || !container || index < 0 || index >= player.queue.length) return;
    const itemTop = index * MUSIC_PLAYLIST_ROW_HEIGHT_PX;
    const itemBottom = itemTop + MUSIC_PLAYLIST_ROW_HEIGHT_PX;
    const viewportTop = container.scrollTop;
    const viewportBottom = viewportTop + container.clientHeight;
    let nextScrollTop = viewportTop;
    if (block === "center") {
      nextScrollTop = itemTop - (container.clientHeight - MUSIC_PLAYLIST_ROW_HEIGHT_PX) / 2;
    } else if (itemTop < viewportTop) {
      nextScrollTop = itemTop;
    } else if (itemBottom > viewportBottom) {
      nextScrollTop = itemBottom - container.clientHeight;
    }
    container.scrollTop = Math.max(0, nextScrollTop);
    playlistScrollTop = container.scrollTop;
  }

  function playlistViewportAction(node: HTMLElement): { destroy: () => void } {
    const updateViewport = () => {
      playlistScrollTop = node.scrollTop;
      playlistViewportHeight = node.clientHeight;
    };
    const observer = new ResizeObserver(updateViewport);
    observer.observe(node);
    node.addEventListener("scroll", updateViewport, { passive: true });
    updateViewport();
    return {
      destroy: () => {
        observer.disconnect();
        node.removeEventListener("scroll", updateViewport);
      },
    };
  }

  function handleMediaSurfaceKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter" || event.code === "Space") {
      event.preventDefault();
      event.stopPropagation();
      void player.togglePlay();
      return;
    }
    if (mediaSurfaceFullscreen && event.key.toLowerCase() === "f") {
      event.preventDefault();
      event.stopPropagation();
      window.dispatchEvent(new CustomEvent(mediaSurfaceFullscreenEvent));
    }
  }
</script>

<svelte:window onkeydowncapture={handleKeydown} onpointerdown={handleWindowPointerDown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  hidden={!visible}
  class={cn("fixed z-40", mobileBuilderPresentation ? "bg-background" : !mobilePresentation && "inset-0")}
  style={mobilePresentation ? "left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height);" : undefined}
  onclick={(event) => { if (!mobileBuilderPresentation && event.target === event.currentTarget) onClose(); }}
></div>
{#if !mobilePresentation}
  <div
    hidden={!visible}
    class="pointer-events-none fixed right-2 z-50 w-[min(1000px,calc(100vw-1rem))] overflow-hidden rounded-floating shadow-floating"
    style={desktopPanelStyle}
    aria-hidden="true"
  >
    <div class="h-full w-full" style="background-color: var(--cal-bg);"></div>
  </div>
{/if}
<div
  bind:this={panel}
  hidden={!visible}
  class={cn(
    "music-panel-root fixed z-70 flex flex-col overflow-hidden outline-none",
    mobileBuilderPresentation
      ? "bg-background"
      : mobilePresentation
        ? "rounded-floating border border-border shadow-floating"
        : "right-2 w-[min(1000px,calc(100vw-1rem))] rounded-floating border border-border",
  )}
  style={mobilePresentation
    ? mobileBuilderPresentation
      ? mobileBuilderPanelStyle
      : `${playlistVisible && mobilePlaylistPanelStyle
        ? mobilePlaylistPanelStyle
        : mobilePlayerPanelStyle || mobilePlayerFallbackStyle};background-color:var(--cal-bg);`
    : desktopPanelStyle}
  role="dialog"
  aria-modal="true"
  aria-label={t("music.title")}
  tabindex="-1"
>
  {#if PlaylistBuilder && playlistBuilderMounted}
    <div class:hidden={musicPage !== "playlist-builder"} class="h-full min-h-0" aria-hidden={musicPage !== "playlist-builder"}>
      <PlaylistBuilder
        onOpenPlayer={closePlaylistBuilder}
        presentation={mobilePresentation ? "mobile" : "desktop"}
        active={visible && musicPage === "playlist-builder"}
        initialAction={playlistBuilderInitialAction}
        onInitialActionHandled={() => { playlistBuilderInitialAction = null; }}
      />
    </div>
  {:else if musicPage === "playlist-builder"}
    <section class="relative grid h-full min-h-40 place-items-center overflow-hidden p-5 text-center text-foreground" style="background-color: var(--cal-bg);">
      {#if playlistBuilderLoadError}
        <button type="button" onclick={closePlaylistBuilder} class="absolute left-3 top-3 inline-flex h-8 items-center rounded-full bg-secondary px-3 text-[0.7rem] font-medium text-secondary-foreground hover:bg-accent hover:text-accent-foreground">{t("music.mediaPlayer")}</button>
        <Notice
          class="max-w-sm text-sm"
          title={t("music.builder.loadFailed")}
          message={playlistBuilderLoadError}
          retryLabel={t("music.builder.retry")}
          onRetry={loadPlaylistBuilder}
        />
      {:else}
        <div class="w-full max-w-lg">
          <MusicPreparationActivity />
          <h1 class="mt-3 text-lg font-semibold tracking-tight">{t("music.builder.loading")}</h1>
        </div>
      {/if}
    </section>
  {/if}
  <section
      class:hidden={musicPage === "playlist-builder"}
      aria-hidden={musicPage === "playlist-builder"}
      data-music-player-page
      class="flex h-full min-h-0 select-none flex-col text-foreground"
      use:releaseClickedButtonFocusAction
      onwheel={(event) => player.handleVolumeWheel(event)}
    >
  <div
    bind:this={musicHeader}
    data-music-player-header
    class={cn(
      "relative flex shrink-0 items-stretch",
      mobilePresentation ? "py-2" : "h-(--toolbar-h)",
    )}
    style="background-color: var(--cal-bg);"
  >
    <div class="relative flex min-w-0 flex-1 items-center gap-3 px-2">
      <div class="relative z-10 flex min-w-0 shrink-0 items-center gap-2">
        <MusicPlaylistLauncher
          active={visible}
          onOpenBuilder={() => openPlaylistBuilder()}
          onOpenIssues={() => openPlaylistBuilder({ kind: "open-issues" })}
          mobile={mobilePresentation}
        />
      </div>
      <div
        class="music-header-title absolute top-1/2 z-0 min-w-0 -translate-x-1/2 -translate-y-1/2 text-center text-[0.8rem] font-medium text-foreground"
        style={`left: ${mediaTitleLeft};`}
      >
        {#if topBarMediaTitle}
          {#if player.currentSource?.kind === "local-file" && supportsLocalFileReveal}
            <button
              type="button"
              onclick={() => { void openCurrentLocalFileLocation(); }}
              class="block w-full truncate text-center transition-colors hover:text-accent-foreground"
              title={t("music.showFileLocation", player.loadedTitle)}
              aria-label={t("music.showCurrentFileLocation")}
            >
              {topBarMediaTitle}
            </button>
          {:else}
            <span class="block w-full truncate text-center" title={player.currentSource ? player.loadedTitle : undefined}>
              {topBarMediaTitle}
            </span>
          {/if}
        {/if}
      </div>
      {#if player.parseError || player.playerError}
        <div class="relative z-10 ml-auto flex min-w-0 items-center gap-2">
          <div class="hidden min-w-0 max-w-56 items-center gap-1.5 text-[0.733333rem] text-foreground min-[720px]:flex" role="alert">
            <AlertCircle class="shrink-0 text-muted-foreground" size={musicIconSize} strokeWidth={musicIconStrokeWidth} />
            <span class="truncate">{player.parseError ?? player.playerError}</span>
          </div>
        </div>
      {/if}
    </div>
    {#if playlistVisible && !mobilePresentation}
      <div data-music-desktop-playlist-header class="hidden shrink-0 items-center justify-between gap-2 px-4 min-[861px]:flex min-[861px]:w-80">
        {#if playlistHasContent}
          <div class="flex items-center gap-2 text-[0.8rem] font-medium text-muted-foreground">
            <ListMusic size={musicIconSize} strokeWidth={musicIconStrokeWidth} />
            {t("music.playlist")}
          </div>
          <div class="text-[0.733333rem] text-muted-foreground">{t("music.tracks", player.queue.length)}</div>
        {/if}
      </div>
    {/if}
  </div>

  {#if savedQueueOfflineSubset}
    <div class="flex shrink-0 items-center gap-2 px-3 py-1.5 text-[0.68rem] text-muted-foreground" role="status">
      <AlertCircle size={13} class="shrink-0" />
      <span class="min-w-0 truncate">{t("music.queueState.offlineSubset", player.savedQueueSkipBreakdown.offline)}</span>
    </div>
  {/if}

  {#if visibleContext && (visibleContext.state === "unavailable" || visibleContext.issue)}
    <Notice
      class="shrink-0 border-y border-border/55 px-3 py-2 text-[0.68rem]"
      tone="warning"
      title={contextSummary}
      message={visibleContext.issue
        ? `${t(`music.assignment.context.state.${visibleContext.state}`)} ${t(`music.assignment.context.issue.${visibleContext.issue}`)}`
        : t(`music.assignment.context.state.${visibleContext.state}`)}
      retryLabel={t("music.assignment.context.retry")}
      onRetry={visibleContext.state === "unavailable" ? () => player.requestContextRetry() : undefined}
      copy={false}
    >
      {#snippet actions()}
        {#if visibleContext.state === "prepared"}
          <button type="button" class="notice-action-text" onclick={() => { void player.playPlayback(); }}>{t("music.assignment.context.preparedPlay")}</button>
        {/if}
        {#if visibleContext.state === "unavailable"}
          <button type="button" class="notice-action-text" onclick={() => openPlaylistBuilder({ kind: "open-issues" })}>{t("music.assignment.context.openIssues")}</button>
        {/if}
        <button type="button" class="notice-action-text" onclick={() => player.inspectContextAssignment()}>{t("music.assignment.context.inspect")}</button>
      {/snippet}
    </Notice>
  {:else if visibleContext}
    <div class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1.5 border-y border-border/55 bg-primary/6 px-3 py-2 text-[0.68rem]" role="status">
      <CalendarClock size={14} class="shrink-0 text-primary" />
      <div class="min-w-40 flex-1 leading-relaxed">
        <p class="font-semibold text-foreground">{contextSummary}</p>
        <p class="text-muted-foreground">{t(`music.assignment.context.state.${visibleContext.state}`)}</p>
      </div>
      {#if visibleContext.state === "prepared"}
        <button type="button" onclick={() => { void player.playPlayback(); }} class="rounded-md bg-primary px-2.5 py-1 font-semibold text-primary-foreground hover:bg-primary/90">{t("music.assignment.context.preparedPlay")}</button>
      {/if}
      <button type="button" onclick={() => player.inspectContextAssignment()} class="rounded-md px-2.5 py-1 font-semibold text-primary hover:bg-primary/10">{t("music.assignment.context.inspect")}</button>
    </div>
  {/if}

  <div
    class={cn(
      "music-player-grid grid min-h-0 flex-1",
      playlistVisible
        ? "grid-cols-[minmax(0,1fr)_minmax(16rem,20rem)] grid-rows-[minmax(0,1fr)_auto] max-[860px]:grid-cols-1 max-[860px]:grid-rows-[minmax(0,1fr)_minmax(0,35%)_auto]"
        : "grid-cols-1 grid-rows-[minmax(0,1fr)_auto]",
    )}
  >
    <div bind:this={mediaCell} class="music-media-cell relative flex min-h-0 items-center justify-center overflow-hidden">
      {#if savedQueueUnavailable}
        <div class="absolute inset-0 z-10 flex flex-col items-center justify-center gap-2 px-6 text-center" role="status" data-music-playlist-unavailable>
          <AlertCircle size={18} strokeWidth={1.4} class="text-muted-foreground" />
          <p class="text-[0.8rem] font-medium">{t("music.queueState.unavailable")}</p>
          <button type="button" onclick={() => openPlaylistBuilder({ kind: "open-issues" })} class="text-[0.68rem] font-medium text-primary hover:underline">{t("music.queueState.review")}</button>
        </div>
      {/if}
      <div
        bind:this={mediaSurface}
        class="music-media-surface relative cursor-default overflow-hidden"
        role="button"
        tabindex={player.currentSource ? 0 : -1}
        aria-disabled={!player.currentSource}
        aria-label={player.isPlaying ? t("music.pause") : t("music.play")}
        data-app-tooltip-disabled="true"
        onclick={handleMediaSurfaceClick}
        ondblclick={handleMediaSurfaceDoubleClick}
        onkeydown={handleMediaSurfaceKeydown}
        onwheel={(event) => player.handleVolumeWheel(event)}
      >
        {#if mediaSurfaceFullscreen && volumeFeedbackVisible}
          <div class="music-volume-feedback pointer-events-none absolute bottom-4 right-4 z-20 select-none text-[0.866667rem] font-medium text-white">
            {player.volumeFeedbackLabel}
          </div>
        {/if}
        {#if player.currentSource?.kind === "local-file" && (!player.localHasVideo || player.snapshot.error)}
          <div class="absolute inset-0 flex items-center justify-center text-muted-foreground" style="background-color: var(--cal-bg);">
            {#if player.currentArtworkUrl && !player.snapshot.error}
              <img
                src={player.currentArtworkUrl}
                alt=""
                class="absolute inset-0 h-full w-full object-contain"
                draggable="false"
                onload={() => player.handleArtworkLoaded()}
                onerror={() => player.handleArtworkError()}
              />
            {/if}
            {#if player.snapshot.error}
              <div class="flex max-w-[80%] flex-col items-center gap-2 text-center text-[0.8rem]">
                <AlertCircle size={musicIconSize} strokeWidth={musicIconStrokeWidth} />
                <span class="max-w-full truncate">{player.snapshot.error ?? player.loadedTitle}</span>
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    {#if playlistVisible}
      <aside bind:this={playlistPanel} id="music-playlist" class="min-h-0" style="background-color: var(--cal-bg);">
        <div class="flex h-full min-h-0 flex-col">
          {#if playlistHasContent}
            <div
              data-music-stacked-playlist-header
              class={cn(
                "flex items-center justify-between gap-2 px-4 py-3",
                !mobilePresentation && "min-[861px]:hidden",
              )}
            >
              <div class="flex items-center gap-2 text-[0.8rem] font-medium text-muted-foreground">
                <ListMusic size={musicIconSize} strokeWidth={musicIconStrokeWidth} />
                {t("music.playlist")}
              </div>
              <div class="text-[0.733333rem] text-muted-foreground">{t("music.tracks", player.queue.length)}</div>
            </div>

            {#if player.folderScanTruncated}
              <div class="mx-4 mt-3 rounded-md border border-warning/40 bg-warning/10 px-2 py-1.5 text-[0.733333rem] text-warning">
                {t("music.scanTruncated")}
              </div>
            {/if}

            <div class="relative min-h-0 flex-1">
              <div
                bind:this={playlistScrollContainer}
                use:playlistViewportAction
                class="hide-scrollbar h-full min-h-0 overflow-y-auto overflow-x-hidden px-3 pb-3 pt-0"
                data-music-scrollable="true"
              >
                <div class="flex flex-col">
                  <div class="shrink-0" aria-hidden="true" style={`height: ${renderedPlaylistWindow.topSpacerHeight}px;`}></div>
                  {#each renderedPlaylistItems as item, offset}
                    {@const index = renderedPlaylistWindow.startIndex + offset}
                    {@const queueEntry = player.savedQueueEntries[index]}
                    {@const snoozed = queueEntry
                      ? queueEntry.snoozedIndefinitely || (queueEntry.snoozedUntil !== null && queueEntry.snoozedUntil > Date.now())
                      : player.sourceQueueSnoozedItemIds.includes(player.activeQueueItemIds[index] ?? "")}
                    <div
                      class={cn(
                        "relative flex h-9 w-full min-w-0 shrink-0 items-center px-2 text-left text-[0.8rem]",
                        index === 0 && "rounded-t-md",
                        index === player.queue.length - 1 && "rounded-b-md",
                        player.highlightedQueueIndex === index && "bg-accent text-accent-foreground",
                      )}
                    >
                      <button
                        type="button"
                        data-playlist-index={index}
                        data-app-tooltip-disabled="true"
                        onclick={() => { void player.playQueueItem(index); }}
                        class="absolute inset-0 rounded-md focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring"
                        aria-label={item.title}
                      ></button>
                      <span class="pointer-events-none relative z-1 flex max-w-full min-w-0 items-center gap-1.5">
                        <span data-music-queue-title use:queueTitleTooltip={item.title} class="min-w-0 truncate">{item.title}</span>
                        {#if snoozed}<MusicSnoozeButton onRemove={() => removeQueueSnooze(index)} />{/if}
                      </span>
                    </div>
                  {/each}
                  <div class="shrink-0" aria-hidden="true" style={`height: ${renderedPlaylistWindow.bottomSpacerHeight}px;`}></div>
                </div>
              </div>
              <CalendarScrollbar scrollContainer={playlistScrollContainer} wheelPassthrough />
            </div>
          {/if}
        </div>
      </aside>
    {/if}

    <div
      bind:this={playbackControls}
      class={cn("px-2 py-2", playlistVisible && "col-span-2 max-[860px]:col-span-1")}
      style="background-color: var(--cal-bg);"
    >
      {#key player.currentSource?.identity ?? "empty"}
        <div class="flex items-center gap-2 text-[0.733333rem] tabular-nums text-muted-foreground">
          <span class="shrink-0 text-left">{formatPlaybackTime(player.snapshot.positionMs)}</span>
          <input
            type="range"
            min="0"
            max={player.progressMax}
            value={player.progressValue}
            disabled={!player.currentSource}
            class="music-seek-slider min-w-0 flex-1 disabled:opacity-50"
            style={`--music-seek-progress: ${seekSliderProgress};`}
            aria-label={t("music.seek")}
            oninput={(event) => { void player.seekToMs(Number(event.currentTarget.value)); }}
            onpointerup={releaseRangeFocus}
            onpointercancel={releaseRangeFocus}
          />
          <span class="shrink-0 text-right">{formatPlaybackTime(player.snapshot.durationMs)}</span>
        </div>
      {/key}

      <div class="music-control-row mt-2 flex flex-wrap items-center justify-between gap-3">
        <div class="music-control-group flex items-center gap-2">
          <button
            type="button"
            onclick={() => { void player.playPreviousTrack(); }}
            disabled={!player.canPlayPreviousTrack}
            class="inline-flex h-9 w-9 items-center justify-center rounded-md bg-secondary text-secondary-foreground transition-colors disabled:pointer-events-none disabled:opacity-50"
            title={t("music.lastTrackTitle", formatShortcut("Mod + ←"))}
            aria-label={t("music.lastTrack")}
          >
            <SkipBack size={musicIconSize} strokeWidth={musicIconStrokeWidth} />
          </button>
          <button
            type="button"
            onclick={() => { void player.togglePlay(); }}
            disabled={!player.currentSource || player.youtubePlaybackStarting}
            class="inline-flex h-9 w-9 items-center justify-center rounded-md bg-secondary text-secondary-foreground transition-colors disabled:pointer-events-none disabled:opacity-50"
            title={player.youtubePlaybackStarting ? t("music.builder.loading") : player.isPlaying ? t("music.pauseShortcut") : t("music.playShortcut")}
            aria-label={player.youtubePlaybackStarting ? t("music.builder.loading") : player.isPlaying ? t("music.pause") : t("music.play")}
          >
            {#if player.youtubePlaybackStarting}
              <LoaderCircle size={musicIconSize} strokeWidth={3} class="animate-spin motion-reduce:animate-none" />
            {:else if player.isPlaying}
              <Pause size={musicIconSize} strokeWidth={musicIconStrokeWidth} />
            {:else}
              <Play size={musicIconSize} strokeWidth={musicIconStrokeWidth} />
            {/if}
          </button>
          <button
            type="button"
            onclick={() => { void player.playNextTrack(); }}
            disabled={!player.canPlayNextTrack}
            class="inline-flex h-9 w-9 items-center justify-center rounded-md bg-secondary text-secondary-foreground transition-colors disabled:pointer-events-none disabled:opacity-50"
            title={t("music.nextTrackTitle", formatShortcut("Mod + →"))}
            aria-label={t("music.nextTrack")}
          >
            <SkipForward size={musicIconSize} strokeWidth={musicIconStrokeWidth} />
          </button>
          <MusicPlaybackModeControl className="music-transport-shuffle" />
        </div>

        <div class="music-control-group flex items-center gap-2">
          <MusicPlaybackModeControl className="music-utility-shuffle" align="end" />
          <div
            class="music-expanded-volume-control flex items-center gap-2 text-[0.8rem] text-muted-foreground"
            data-music-volume-control="true"
            onwheel={(event) => player.handleVolumeWheel(event)}
          >
            <input
              type="range"
              min="0"
              max={volumeMax}
              step={volumeShortcutStep}
              value={player.volumeControlValue}
              class="music-volume-slider block w-28"
              style={`--music-volume-progress: ${volumeSliderProgress};`}
              aria-label={t("music.volume")}
              data-app-tooltip={t("music.volumeTooltip")}
              oninput={(event) => { setVolumeFromControl(Number(event.currentTarget.value)); }}
              onpointerup={releaseRangeFocus}
              onpointercancel={releaseRangeFocus}
            />
            <button
              type="button"
              onclick={() => { void player.toggleMute(); }}
              class={cn(
                "inline-flex h-8 w-10 items-center justify-center tabular-nums",
                player.muted && "line-through opacity-60",
              )}
              title={player.muted ? t("music.unmuteTitle") : t("music.muteTitle")}
              aria-label={player.muted ? t("music.unmuteVolume") : t("music.muteVolume")}
              aria-pressed={player.muted}
            >
              {player.volumePercentLabel}
            </button>
          </div>
          {#if supportsSoundscapes}
            <MusicSoundscapeControl onOpenSoundscapes={() => openPlaylistBuilder({ kind: "open-soundscapes" })} />
          {/if}
          <div
            bind:this={volumeMenuRoot}
            class="music-compact-volume-control relative"
            data-music-volume-control="true"
            onwheel={(event) => player.handleVolumeWheel(event)}
          >
            <button
              type="button"
              onclick={toggleVolumeMenu}
              class={cn(
                "inline-flex h-9 w-9 items-center justify-center rounded-md bg-secondary text-secondary-foreground transition-colors hover:bg-accent hover:text-accent-foreground",
                player.muted && "text-muted-foreground opacity-70",
              )}
              title={t("music.volumeTooltip")}
              aria-label={t("music.volumeControls")}
              aria-haspopup="dialog"
              aria-expanded={volumeMenuOpen}
              data-music-volume-control="true"
              onwheel={(event) => player.handleVolumeWheel(event)}
            >
              {#if player.muted || player.volumeControlValue === 0}
                <VolumeX size={musicIconSize} strokeWidth={musicIconStrokeWidth} />
              {:else}
                <Volume2 size={musicIconSize} strokeWidth={musicIconStrokeWidth} />
              {/if}
            </button>
            {#if volumeMenuOpen}
              <div class="music-volume-panel absolute bottom-full left-1/2 z-30 mb-2 flex -translate-x-1/2 flex-col items-center gap-2 surface-floating p-2 text-foreground">
                <button
                  type="button"
                  onclick={() => { void player.toggleMute(); }}
                  class={cn(
                    "inline-flex h-7 w-full items-center justify-center rounded-sm px-1.5 text-[0.733333rem] tabular-nums text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground",
                    player.muted && "line-through opacity-60",
                  )}
                  title={player.muted ? t("music.unmuteTitle") : t("music.muteTitle")}
                  aria-label={player.muted ? t("music.unmuteVolume") : t("music.muteVolume")}
                  aria-pressed={player.muted}
                >
                  {player.volumePercentLabel}
                </button>
                <div class="music-volume-slider-vertical-frame flex items-center justify-center">
                  <input
                    type="range"
                    min="0"
                    max={volumeMax}
                    step={volumeShortcutStep}
                    value={player.volumeControlValue}
                    class="music-volume-slider music-volume-slider-vertical"
                    style={`--music-volume-progress: ${volumeSliderProgress};`}
                    aria-label={t("music.volume")}
                    oninput={(event) => { setVolumeFromControl(Number(event.currentTarget.value)); }}
                    onpointerup={releaseRangeFocus}
                    onpointercancel={releaseRangeFocus}
                  />
                </div>
              </div>
            {/if}
          </div>

          <MusicTrackPreferences
            active={visible && musicPage === "player"}
            {volumeMenuOpen}
            onOpen={closeVolumeMenu}
          />
          <MusicCurrentItemMenu active={visible} onOpenBuilder={(itemId) => openPlaylistBuilder(itemId ? { kind: "open-review-item", itemId } : "open-review")} />
          <button
            type="button"
            onclick={togglePlaylist}
            class="inline-flex h-9 w-9 items-center justify-center rounded-md bg-secondary text-secondary-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
            title={playlistVisible ? t("music.hidePlaylistTitle") : t("music.showPlaylistTitle")}
            aria-label={playlistVisible ? t("music.hidePlaylist") : t("music.showPlaylist")}
            aria-controls="music-playlist"
            aria-expanded={playlistVisible}
          >
            <ListMusic size={musicIconSize} strokeWidth={musicIconStrokeWidth} />
          </button>
        </div>
      </div>
    </div>
  </div>
</section>
</div>

<style>
  .music-header-title {
    width: clamp(3rem, 22vw, 16rem);
  }

  .music-media-cell {
    container-type: size;
  }

  .music-media-surface {
    aspect-ratio: 16 / 9;
    width: min(100%, calc(100cqh * 16 / 9));
  }

  .music-compact-volume-control {
    display: none;
  }

  .music-volume-panel {
    width: 3.5rem;
    background-color: var(--cal-bg);
  }

  :global(.music-utility-shuffle) {
    display: none;
  }

  @media (max-width: 440px) {
    .music-control-row {
      justify-content: center;
      gap: 0.5rem;
    }

    .music-control-group {
      display: contents;
      justify-content: center;
    }

    :global(.music-transport-shuffle) {
      display: none;
    }

    :global(.music-utility-shuffle) {
      display: inline-flex;
    }

    .music-expanded-volume-control {
      display: none;
    }

    .music-compact-volume-control {
      display: block;
    }
  }

  @media (max-width: 320px) {
    .music-control-row {
      flex-direction: column;
      gap: 0.5rem;
    }

    .music-control-group {
      display: flex;
      width: 100%;
    }
  }

  @media (max-height: 300px) {
    .music-player-grid {
      grid-template-rows: minmax(0, 1fr) auto;
    }

    .music-player-grid :global(#music-playlist) {
      display: none;
    }
  }

  .music-volume-slider {
    --music-volume-thumb-size: 0.875rem;
    --music-volume-track-height: 0.25rem;
    --music-volume-track-color: color-mix(in srgb, var(--foreground) 22%, transparent);
    --music-volume-thumb-border: color-mix(in srgb, var(--foreground) 18%, transparent);

    height: var(--music-volume-thumb-size);
    appearance: none;
    cursor: pointer;
    background:
      linear-gradient(
        to right,
        var(--primary) 0%,
        var(--primary) var(--music-volume-progress),
        var(--music-volume-track-color) var(--music-volume-progress),
        var(--music-volume-track-color) 100%
      )
      center / calc(100% - var(--music-volume-thumb-size)) var(--music-volume-track-height) no-repeat;
  }

  .music-volume-slider::-webkit-slider-runnable-track {
    height: var(--music-volume-track-height);
    border-radius: 999px;
    background: transparent;
  }

  .music-volume-slider::-webkit-slider-thumb {
    width: var(--music-volume-thumb-size);
    height: var(--music-volume-thumb-size);
    margin-top: calc((var(--music-volume-track-height) - var(--music-volume-thumb-size)) / 2);
    appearance: none;
    border: 1px solid var(--music-volume-thumb-border);
    border-radius: 999px;
    background: var(--card);
  }

  .music-volume-slider::-moz-range-track,
  .music-volume-slider::-moz-range-progress {
    height: var(--music-volume-track-height);
    border-radius: 999px;
    background: transparent;
  }

  .music-volume-slider::-moz-range-thumb {
    width: var(--music-volume-thumb-size);
    height: var(--music-volume-thumb-size);
    border: 1px solid var(--music-volume-thumb-border);
    border-radius: 999px;
    background: var(--card);
  }

  .music-volume-slider-vertical {
    width: 8rem;
    height: var(--music-volume-thumb-size);
    transform: rotate(-90deg);
    background:
      linear-gradient(
        to right,
        var(--primary) 0%,
        var(--primary) var(--music-volume-progress),
        var(--music-volume-track-color) var(--music-volume-progress),
        var(--music-volume-track-color) 100%
      )
      center / calc(100% - var(--music-volume-thumb-size)) var(--music-volume-track-height) no-repeat;
  }

  .music-volume-slider-vertical-frame {
    width: var(--music-volume-thumb-size);
    height: 8rem;
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.music-panel-root *) {
      scroll-behavior: auto !important;
      transition-duration: 0.01ms !important;
      animation-duration: 0.01ms !important;
      animation-iteration-count: 1 !important;
    }
  }
</style>
