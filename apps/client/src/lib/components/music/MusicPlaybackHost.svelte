<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { getMusicPlayer } from "$lib/stores/music-player.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { coverPlaybackHostRect } from "$lib/music/session/playback-host-layout";
  import {
    createNativeYouTubeHost,
    HIDDEN_NATIVE_YOUTUBE_BOUNDS,
    usesNativeYouTubeHost,
  } from "$lib/music/youtube-native-host";
  import type { NativeYouTubeHostBounds } from "$lib/api/music";

  let {
    decodes = true,
  }: {
    /** Whether this window runs the browser decoders; a window that does not shows no media surface. */
    decodes?: boolean;
  } = $props();

  const player = getMusicPlayer();
  const { t } = getLocalization();

  let hostElement = $state<HTMLDivElement | null>(null);
  let youtubeFrame = $state<HTMLIFrameElement | null>(null);
  let localMediaElement = $state<HTMLMediaElement | null>(null);
  let surfaceRect = $state<DOMRect | null>(null);
  let surfaceFullscreen = $state(false);
  let volumeFeedbackVisible = $state(false);
  let surfaceClickTimeoutId: number | null = null;
  let volumeFeedbackTimeoutId: number | null = null;
  let lastVolumeFeedbackId = 0;

  const mediaSurfaceFullscreenEvent = "ganbaru-music-media-surface-fullscreen";
  const NATIVE_YOUTUBE_OCCLUSION_INTERVAL_MS = 250;
  const hasVisualSurface = $derived(Boolean(
    decodes
      && player.surfaceElement
      && player.currentSource
      && (player.isYouTubeActive || (player.currentSource.kind === "local-file" && player.localHasVideo)),
  ));
  const hasStaleVisual = $derived(Boolean(player.surfaceElement && player.staleVisual));
  const hasHostSurface = $derived(hasVisualSurface || hasStaleVisual);
  const hasFullscreenSurface = $derived(Boolean(
    player.surfaceElement && player.currentSource && !player.playsInOtherWindow,
  ));
  const hostIsFullscreen = $derived(Boolean(
    surfaceFullscreen
      && hostElement
      && typeof document !== "undefined"
      && document.fullscreenElement === hostElement,
  ));
  const hasFullscreenAudioArtwork = $derived(Boolean(
    hostIsFullscreen
      && player.currentSource?.kind === "local-file"
      && !player.localHasVideo
      && player.currentArtworkUrl
      && !player.snapshot.error,
  ));
  const hostStyle = $derived.by(() => {
    if (hostIsFullscreen) {
      return "left: 0; top: 0; width: 100vw; height: 100vh; background-color: #000;";
    }
    if (surfaceRect && hasHostSurface) {
      const alignedRect = coverPlaybackHostRect(
        surfaceRect,
        typeof window === "undefined" ? 1 : window.devicePixelRatio,
      );
      return `left: ${alignedRect.left}px; top: ${alignedRect.top}px; width: ${alignedRect.width}px; height: ${alignedRect.height}px; background-color: var(--cal-bg);`;
    }
    return "left: -10000px; top: -10000px; width: 1px; height: 1px; background-color: var(--cal-bg);";
  });
  const hostVisualFrameStyle = $derived(hostIsFullscreen
    ? "background-color: #000;"
    : "background-color: var(--cal-bg);");
  const hostHasHitTarget = $derived(hasVisualSurface || (hostIsFullscreen && hasFullscreenSurface));

  const nativeYouTubeHost = usesNativeYouTubeHost
    ? createNativeYouTubeHost("playback", (data) => player.handleYouTubeNativeData(data))
    : null;
  const nativeYouTubeUrl = $derived(
    nativeYouTubeHost && decodes && player.currentSource && player.isYouTubeActive
      ? player.youtubeHostUrl
      : null,
  );
  let nativeYouTubeUnobscured = $state(false);
  const nativeYouTubeVisible = $derived(Boolean(
    nativeYouTubeUrl && hostHasHitTarget && !player.staleVisual && nativeYouTubeUnobscured,
  ));

  onMount(() => {
    player.registerYouTubeNativeHost(nativeYouTubeHost);
    player.init({ decodesBrowserMedia: decodes });
  });

  onDestroy(() => {
    clearSurfaceClickTimeout();
    clearVolumeFeedbackTimeout();
    player.destroy();
    player.registerYouTubeNativeHost(null);
    if (nativeYouTubeHost) void nativeYouTubeHost.close().catch(reportNativeYouTubeHostError);
  });

  $effect(() => {
    player.registerYouTubeFrame(youtubeFrame);
  });

  function reportNativeYouTubeHostError(error: unknown): void {
    console.warn("Unable to update the YouTube host.", error);
  }

  /** Current host placement; the native view sits above the app WebView, so it shows only while nothing covers it. */
  function nativeYouTubeBounds(visible: boolean): NativeYouTubeHostBounds {
    const rect = hostElement?.getBoundingClientRect();
    if (!visible || !rect || rect.width < 1 || rect.height < 1) return HIDDEN_NATIVE_YOUTUBE_BOUNDS;
    return {
      x: rect.left,
      y: rect.top,
      width: rect.width,
      height: rect.height,
      scale: window.devicePixelRatio || 1,
      visible: true,
    };
  }

  function hostSurfaceUnobscured(): boolean {
    if (!hostElement || document.visibilityState !== "visible") return false;
    const rect = hostElement.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) return false;
    const inset = Math.min(4, rect.width / 4, rect.height / 4);
    const points: Array<[number, number]> = [
      [rect.left + rect.width / 2, rect.top + rect.height / 2],
      [rect.left + inset, rect.top + inset],
      [rect.right - inset, rect.top + inset],
      [rect.left + inset, rect.bottom - inset],
      [rect.right - inset, rect.bottom - inset],
    ];
    const host = hostElement;
    return points.every(([x, y]) => {
      const element = document.elementFromPoint(x, y);
      return element !== null && host.contains(element);
    });
  }

  $effect(() => {
    if (!nativeYouTubeHost) return;
    const url = nativeYouTubeUrl;
    if (!url) {
      void nativeYouTubeHost.close().catch(reportNativeYouTubeHostError);
      return;
    }
    const bounds = untrack(() => nativeYouTubeBounds(nativeYouTubeVisible));
    void nativeYouTubeHost.open(url, bounds).catch(reportNativeYouTubeHostError);
  });

  $effect(() => {
    if (!nativeYouTubeHost || !nativeYouTubeUrl || !hostHasHitTarget) {
      nativeYouTubeUnobscured = false;
      return;
    }
    const update = () => {
      nativeYouTubeUnobscured = hostSurfaceUnobscured();
    };
    update();
    const intervalId = window.setInterval(update, NATIVE_YOUTUBE_OCCLUSION_INTERVAL_MS);
    document.addEventListener("visibilitychange", update);
    return () => {
      window.clearInterval(intervalId);
      document.removeEventListener("visibilitychange", update);
    };
  });

  $effect(() => {
    if (!nativeYouTubeHost || !nativeYouTubeUrl) return;
    void hostStyle;
    void surfaceRect;
    const bounds = nativeYouTubeBounds(nativeYouTubeVisible);
    void nativeYouTubeHost.setBounds(bounds).catch(reportNativeYouTubeHostError);
  });

  $effect(() => {
    player.registerLocalMedia(localMediaElement);
  });

  function handleSurfacePointerDown(event: PointerEvent): void {
    event.preventDefault();
  }

  function handleSurfaceClick(event: MouseEvent): void {
    event.preventDefault();
    if (event.button !== 0) return;
    if (event.detail > 1) return;
    clearSurfaceClickTimeout();
    surfaceClickTimeoutId = window.setTimeout(() => {
      surfaceClickTimeoutId = null;
      void player.togglePlay();
    }, 250);
  }

  function handleSurfaceDoubleClick(event: MouseEvent): void {
    event.preventDefault();
    if (event.button !== 0) return;
    clearSurfaceClickTimeout();
    void toggleSurfaceFullscreen();
  }

  function clearSurfaceClickTimeout(): void {
    if (surfaceClickTimeoutId === null) return;
    window.clearTimeout(surfaceClickTimeoutId);
    surfaceClickTimeoutId = null;
  }

  function clearVolumeFeedbackTimeout(): void {
    if (volumeFeedbackTimeoutId === null) return;
    window.clearTimeout(volumeFeedbackTimeoutId);
    volumeFeedbackTimeoutId = null;
  }

  function showVolumeFeedback(): void {
    clearVolumeFeedbackTimeout();
    volumeFeedbackVisible = true;
    volumeFeedbackTimeoutId = window.setTimeout(() => {
      volumeFeedbackVisible = false;
      volumeFeedbackTimeoutId = null;
    }, 900);
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    if (event.key.toLowerCase() !== "f") return;
    if (!hasFullscreenSurface || isEditableTarget(event.target)) return;
    event.preventDefault();
    void toggleSurfaceFullscreen();
  }

  function isEditableTarget(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    return Boolean(target.closest("input, textarea, select, [contenteditable='true']"));
  }

  function handleFullscreenSurfaceKeydown(event: KeyboardEvent): void {
    if (!surfaceFullscreen || event.altKey || event.ctrlKey || event.metaKey) return;
    if (event.code === "Space" || event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      void player.togglePlay();
      return;
    }
    if (event.key.toLowerCase() === "f") {
      event.preventDefault();
      event.stopPropagation();
      void toggleSurfaceFullscreen();
    }
  }

  async function toggleSurfaceFullscreen(): Promise<void> {
    if (typeof document === "undefined" || !document.fullscreenEnabled) return;
    const target = fullscreenTargetElement();
    if (!target) return;
    try {
      if (surfaceFullscreen) {
        await document.exitFullscreen();
      } else {
        await target.requestFullscreen({ navigationUI: "hide" });
        target.focus({ preventScroll: true });
      }
    } catch (error) {
      console.warn("Unable to toggle music fullscreen.", error);
    }
  }

  function fullscreenTargetElement(): HTMLElement | null {
    if (!hasFullscreenSurface) return null;
    return hostElement ?? player.surfaceElement;
  }

  $effect(() => {
    const element = player.surfaceElement;
    if (!element) {
      surfaceRect = null;
      return;
    }

    const updateRect = () => {
      const rectElement = hostIsFullscreen && hostElement ? hostElement : element;
      surfaceRect = rectElement.getBoundingClientRect();
    };
    updateRect();

    const observer = new ResizeObserver(updateRect);
    observer.observe(element);
    if (hostElement) {
      observer.observe(hostElement);
    }
    window.addEventListener("resize", updateRect);
    window.addEventListener("scroll", updateRect, true);

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", updateRect);
      window.removeEventListener("scroll", updateRect, true);
    };
  });

  $effect(() => {
    if (typeof document === "undefined") return;
    const updateFullscreenState = () => {
      surfaceFullscreen = Boolean(
        document.fullscreenElement
          && (document.fullscreenElement === hostElement || document.fullscreenElement === player.surfaceElement),
      );
      if (surfaceFullscreen && document.fullscreenElement instanceof HTMLElement) {
        document.fullscreenElement.focus({ preventScroll: true });
      }
    };
    updateFullscreenState();
    document.addEventListener("fullscreenchange", updateFullscreenState);
    return () => {
      document.removeEventListener("fullscreenchange", updateFullscreenState);
    };
  });

  $effect(() => {
    if (!surfaceFullscreen || hasFullscreenSurface || typeof document === "undefined") return;
    if (document.fullscreenElement === hostElement || document.fullscreenElement === player.surfaceElement) {
      void document.exitFullscreen();
    }
  });

  $effect(() => {
    if (typeof window === "undefined") return;
    const handleMediaSurfaceFullscreenRequest = () => {
      void toggleSurfaceFullscreen();
    };
    window.addEventListener(mediaSurfaceFullscreenEvent, handleMediaSurfaceFullscreenRequest);
    return () => {
      window.removeEventListener(mediaSurfaceFullscreenEvent, handleMediaSurfaceFullscreenRequest);
    };
  });

  $effect(() => {
    const feedbackId = player.volumeFeedbackId;
    if (feedbackId === lastVolumeFeedbackId) return;
    lastVolumeFeedbackId = feedbackId;
    if (!hostIsFullscreen) return;
    showVolumeFeedback();
  });
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<div
  bind:this={hostElement}
  class="music-playback-host pointer-events-none fixed overflow-hidden"
  style={`z-index: 60; ${hostStyle}`}
  tabindex="-1"
  aria-hidden={!hasHostSurface && !hostIsFullscreen}
  onkeydown={handleFullscreenSurfaceKeydown}
>
  {#if decodes && player.currentSource && player.isYouTubeActive && !usesNativeYouTubeHost}
    {#key player.youtubeHostUrl}
      <iframe
        bind:this={youtubeFrame}
        class="pointer-events-none h-full w-full border-0 outline-none"
        src={player.youtubeHostUrl ?? "about:blank"}
        title={player.loadedTitle}
        allow="autoplay; encrypted-media"
        referrerpolicy="strict-origin-when-cross-origin"
      ></iframe>
    {/key}
  {:else if player.currentSource?.kind === "local-file"}
    {#if player.localMediaSrc && player.localHasVideo}
      {#key player.localMediaSrc}
        <video
          bind:this={localMediaElement}
          class="music-local-video pointer-events-none h-full w-full border-0 object-contain outline-none transition-opacity duration-75"
          style="background-color: var(--cal-bg);"
          class:opacity-0={!player.localVideoReady}
          class:opacity-100={player.localVideoReady}
          src={player.localMediaSrc}
          crossorigin="anonymous"
          playsinline
          onloadedmetadata={(event) => player.handleLocalLoadedMetadata(event)}
          onloadeddata={(event) => player.handleLocalLoadedData(event)}
          ontimeupdate={(event) => player.handleLocalTimeUpdate(event)}
          onplay={(event) => player.handleLocalPlay(event)}
          onpause={(event) => player.handleLocalPause(event)}
          onended={(event) => { void player.handleLocalEnded(event); }}
          onerror={(event) => player.handleLocalError(event)}
        >
          <track kind="captions" />
        </video>
      {/key}
    {/if}
  {/if}
  {#if hasFullscreenAudioArtwork}
    <div
      class="pointer-events-none absolute inset-0 flex items-center justify-center overflow-hidden"
      style={hostVisualFrameStyle}
    >
      <img
        src={player.currentArtworkUrl ?? ""}
        alt=""
        class="h-full w-full object-contain"
        draggable="false"
        onload={() => player.handleArtworkLoaded()}
        onerror={() => player.handleArtworkError()}
      />
    </div>
  {/if}
  {#if player.staleVisual}
    <div
      class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center overflow-hidden"
      style={hostVisualFrameStyle}
    >
      <img
        src={player.staleVisual.url}
        alt=""
        class="h-full w-full object-contain"
        draggable="false"
      />
    </div>
  {/if}
  {#if hostHasHitTarget}
    {#if hostIsFullscreen && volumeFeedbackVisible}
      <div class="music-volume-feedback pointer-events-none absolute bottom-4 right-4 z-20 select-none text-[0.866667rem] font-medium text-white">
        {player.volumeFeedbackLabel}
      </div>
    {/if}
    <button
      type="button"
      tabindex="-1"
      class="pointer-events-auto absolute inset-0 z-20 cursor-default border-0 bg-transparent p-0 text-transparent outline-none focus:outline-none"
      aria-label={player.isPlaying ? t("music.pause") : t("music.play")}
      data-app-tooltip-disabled="true"
      onpointerdown={handleSurfacePointerDown}
      onclick={handleSurfaceClick}
      ondblclick={handleSurfaceDoubleClick}
      onkeydown={handleFullscreenSurfaceKeydown}
      onwheel={(event) => player.handleVolumeWheel(event)}
    ></button>
  {/if}
</div>

<style>
  .music-local-video {
    transform: scale(1.004);
    transform-origin: center;
  }

  :global(.music-playback-host:fullscreen .music-local-video) {
    transform: none;
  }

  :global(.music-media-surface:fullscreen) {
    width: 100vw;
    height: 100vh;
    background-color: #000 !important;
  }

  :global(.music-playback-host:fullscreen) {
    width: 100vw;
    height: 100vh;
    background-color: #000 !important;
  }

  :global(.music-media-surface:fullscreen > div),
  :global(.music-playback-host:fullscreen),
  :global(.music-playback-host:fullscreen video) {
    background-color: #000 !important;
  }

  :global(.music-volume-feedback) {
    text-shadow:
      0 0 1px #000,
      0 1px 2px rgb(0 0 0 / 0.95),
      1px 0 2px rgb(0 0 0 / 0.95),
      0 -1px 2px rgb(0 0 0 / 0.95),
      -1px 0 2px rgb(0 0 0 / 0.95);
  }

  :global(.music-playback-host:fullscreen *),
  :global(.music-media-surface:fullscreen),
  :global(.music-media-surface:fullscreen *) {
    cursor: none;
  }
</style>
