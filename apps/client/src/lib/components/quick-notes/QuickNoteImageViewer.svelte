<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import CloudDownload from "@lucide/svelte/icons/cloud-download";
  import Copy from "@lucide/svelte/icons/copy";
  import Download from "@lucide/svelte/icons/download";
  import ImageOff from "@lucide/svelte/icons/image-off";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import X from "@lucide/svelte/icons/x";
  import ZoomIn from "@lucide/svelte/icons/zoom-in";
  import ZoomOut from "@lucide/svelte/icons/zoom-out";
  import { downloadQuickNoteImage } from "$lib/api/quick-notes";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import { quickNoteImageCache, type QuickNoteImageState } from "$lib/quick-notes/image-cache";
  import {
    QUICK_NOTE_VIEWER_ZOOM_STEP,
    clampViewerTransform,
    panViewer,
    pinchViewer,
    stepViewerIndex,
    toggleViewerZoom,
    viewerFit,
    viewerIndexAfterRemoval,
    viewerIsZoomed,
    viewerMaxScale,
    viewerPointOnImage,
    viewerSwipeAction,
    viewerWheelFactor,
    zoomViewerBy,
    type ViewerPoint,
    type ViewerSize,
    type ViewerTransform,
  } from "$lib/quick-notes/image-viewer";
  import type { QuickNoteImage } from "$lib/quick-notes/types";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { imageBlobToPng, imageClipboardSupported, writePngToClipboard } from "$lib/utils/clipboard-images";
  import { portal } from "$lib/utils/portal";

  let {
    images,
    startIndex,
    readOnly = false,
    onRemove,
    onClose,
  }: {
    images: readonly QuickNoteImage[];
    startIndex: number;
    readOnly?: boolean;
    /** Removes the image at an index from the note; the viewer then shows a neighbor or closes. */
    onRemove?: (index: number) => void;
    onClose: () => void;
  } = $props();

  /** Pointer travel below which a press counts as a tap, in pixels. */
  const TAP_SLOP = 8;
  /** Longest gap and distance between the taps of a double tap. */
  const DOUBLE_TAP_MS = 280;
  const DOUBLE_TAP_SLOP = 32;
  /** Distance an arrow key moves a zoomed image, in pixels. */
  const KEY_PAN = 80;
  const NOTICE_MS = 2600;

  interface Gesture {
    pointerType: string;
    start: ViewerPoint;
    moved: boolean;
    /** A second finger touched down, so the gesture is a pinch rather than a tap or swipe. */
    multi: boolean;
    onImage: boolean;
  }

  const localization = getLocalization();
  const { t } = localization;
  const mobileBackStack = getMobileBackStack();
  const canCopy = imageClipboardSupported();
  const canDownload = BUILD_PLATFORM_PROFILE.shell === "desktop" || BUILD_PLATFORM_PROFILE.platform === "android";
  let root = $state<HTMLDivElement | null>(null);
  let stage = $state<HTMLDivElement | null>(null);
  let index = $state(untrack(() => startIndex));
  let viewport = $state.raw<ViewerSize>({ width: 0, height: 0 });
  let transform = $state.raw<ViewerTransform>({ scale: 1, x: 0, y: 0 });
  let animate = $state(false);
  let dragging = $state(false);
  let swipe = $state.raw<ViewerPoint>({ x: 0, y: 0 });
  let thumbnailState = $state.raw<QuickNoteImageState>({ status: "loading" });
  let fullState = $state.raw<QuickNoteImageState>({ status: "loading" });
  let fullDecoded = $state(false);
  let downloading = $state(false);
  let notice = $state<string | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout> | null = null;
  let tapTimer: ReturnType<typeof setTimeout> | null = null;
  let lastTap: { time: number; point: ViewerPoint } | null = null;
  let gesture: Gesture | null = null;
  const pointers = new Map<number, ViewerPoint>();

  const count = $derived(images.length);
  const shownIndex = $derived(Math.min(index, Math.max(0, count - 1)));
  const image = $derived(images[shownIndex]);
  const sha256 = $derived(image?.sha256 ?? "");
  const imageSize = $derived<ViewerSize>(image ? { width: image.width, height: image.height } : { width: 1, height: 1 });
  const zoomed = $derived(viewerIsZoomed(transform, imageSize, viewport));
  const atMaxZoom = $derived(transform.scale >= viewerMaxScale(imageSize, viewport) * 0.999);
  const thumbnailUrl = $derived(thumbnailState.status === "ready" ? thumbnailState.url : null);
  const fullUrl = $derived(fullState.status === "ready" ? fullState.url : null);
  const failure = $derived(fullState.status === "failed" ? fullState.code : null);
  const canRemove = $derived(!readOnly && onRemove !== undefined);
  const zoomLabel = $derived(formatNumber(localization.locale, transform.scale, { style: "percent", maximumFractionDigits: 0 }));
  /** The page behind fades as a downward swipe gets closer to closing. */
  const backdropAlpha = $derived(0.92 * (1 - Math.min(0.6, swipe.y / Math.max(1, viewport.height))));
  const frameStyle = $derived([
    `width: ${imageSize.width * transform.scale}px`,
    `height: ${imageSize.height * transform.scale}px`,
    `transform: translate(-50%, -50%) translate(${transform.x + swipe.x}px, ${transform.y + swipe.y}px)`,
  ].join("; "));

  $effect(() => {
    if (count === 0) untrack(onClose);
  });

  // Shows the shared thumbnail until the original is decoded, and warms the neighbors' originals.
  $effect(() => {
    void sha256;
    const current = untrack(() => image);
    if (!current) return;
    const cache = quickNoteImageCache();
    untrack(() => {
      thumbnailState = { status: "loading" };
      fullState = { status: "loading" };
      fullDecoded = false;
    });
    const stops = [
      cache.subscribe(current, "thumbnail", (next) => untrack(() => { thumbnailState = next; })),
      cache.subscribe(current, "full", (next) => untrack(() => {
        if (next.status !== "ready" || fullState.status !== "ready" || next.url !== fullState.url) fullDecoded = false;
        fullState = next;
      })),
    ];
    const all = untrack(() => images);
    const position = untrack(() => shownIndex);
    if (all.length > 1) {
      for (const direction of [-1, 1] as const) {
        const neighbor = all[stepViewerIndex(position, all.length, direction)];
        if (neighbor && neighbor.sha256 !== current.sha256) stops.push(cache.subscribe(neighbor, "full", () => undefined));
      }
    }
    return () => {
      for (const stop of stops) stop();
    };
  });

  // A new image starts fitted; a resized viewport keeps a zoomed image in bounds.
  let fittedSha = "";
  $effect(() => {
    const key = sha256;
    const size = viewport;
    const shape = imageSize;
    untrack(() => {
      if (key !== fittedSha || !viewerIsZoomed(transform, shape, size)) {
        fittedSha = key;
        animate = false;
        transform = viewerFit(shape, size);
      } else {
        transform = clampViewerTransform(transform, shape, size);
      }
    });
  });

  function setTransform(next: ViewerTransform, animated: boolean): void {
    animate = animated;
    transform = next;
  }

  function center(): ViewerPoint {
    return { x: viewport.width / 2, y: viewport.height / 2 };
  }

  function zoomBy(factor: number): void {
    setTransform(zoomViewerBy(transform, factor, imageSize, viewport, center()), true);
  }

  function fit(): void {
    setTransform(viewerFit(imageSize, viewport), true);
  }

  function show(next: number): void {
    if (count === 0) return;
    index = Math.min(Math.max(0, next), count - 1);
  }

  function step(direction: -1 | 1): void {
    if (count > 1) show(stepViewerIndex(shownIndex, count, direction));
  }

  function announce(message: string): void {
    if (noticeTimer) clearTimeout(noticeTimer);
    notice = message;
    noticeTimer = setTimeout(() => {
      noticeTimer = null;
      notice = null;
    }, NOTICE_MS);
  }

  function copy(): void {
    const current = image;
    if (!current || !canCopy) return;
    // The clipboard write starts inside the gesture and waits for the converted image.
    writePngToClipboard(quickNoteImageCache().blob(current).then(imageBlobToPng))
      .then(() => announce(t("quickNotes.images.viewer.copied")))
      .catch((error: unknown) => {
        console.warn("quick note image could not be copied", error);
        announce(t("quickNotes.images.viewer.copyFailed"));
      });
  }

  async function download(): Promise<void> {
    const current = image;
    if (!current || !canDownload || downloading) return;
    downloading = true;
    try {
      if (await downloadQuickNoteImage(current)) {
        announce(BUILD_PLATFORM_PROFILE.platform === "android"
          ? t("quickNotes.images.viewer.downloadedAndroid")
          : t("quickNotes.images.viewer.downloaded"));
      }
    } catch (error: unknown) {
      console.warn("quick note image could not be saved", error);
      announce(t("quickNotes.images.viewer.downloadFailed"));
    } finally {
      downloading = false;
    }
  }

  function remove(): void {
    if (!canRemove || !image) return;
    const removed = shownIndex;
    const next = viewerIndexAfterRemoval(removed, count);
    onRemove?.(removed);
    if (next === null) onClose();
    else index = next;
  }

  function localPoint(event: MouseEvent): ViewerPoint {
    const rect = stage?.getBoundingClientRect();
    return { x: event.clientX - (rect?.left ?? 0), y: event.clientY - (rect?.top ?? 0) };
  }

  function clearTapTimer(): void {
    if (tapTimer) clearTimeout(tapTimer);
    tapTimer = null;
  }

  function handlePointerDown(event: PointerEvent): void {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    if (!stage) return;
    stage.setPointerCapture(event.pointerId);
    const point = localPoint(event);
    pointers.set(event.pointerId, point);
    if (pointers.size === 1) {
      gesture = {
        pointerType: event.pointerType,
        start: point,
        moved: false,
        multi: false,
        onImage: viewerPointOnImage(point, transform, imageSize, viewport),
      };
      dragging = true;
      animate = false;
    } else if (gesture) {
      gesture.multi = true;
      swipe = { x: 0, y: 0 };
    }
  }

  function handlePointerMove(event: PointerEvent): void {
    const previous = pointers.get(event.pointerId);
    if (!previous) return;
    const point = localPoint(event);
    if (pointers.size >= 2) {
      const [firstId, secondId] = [...pointers.keys()];
      if (firstId === undefined || secondId === undefined || (event.pointerId !== firstId && event.pointerId !== secondId)) {
        pointers.set(event.pointerId, point);
        return;
      }
      const before = [pointers.get(firstId) ?? point, pointers.get(secondId) ?? point] as const;
      pointers.set(event.pointerId, point);
      const after = [pointers.get(firstId) ?? point, pointers.get(secondId) ?? point] as const;
      setTransform(pinchViewer(transform, before, after, imageSize, viewport), false);
      return;
    }
    pointers.set(event.pointerId, point);
    if (!gesture) return;
    const dx = point.x - gesture.start.x;
    const dy = point.y - gesture.start.y;
    if (!gesture.moved && Math.hypot(dx, dy) < TAP_SLOP) return;
    gesture.moved = true;
    if (zoomed) {
      setTransform(panViewer(transform, { x: point.x - previous.x, y: point.y - previous.y }, imageSize, viewport), false);
    } else if (!gesture.multi) {
      swipe = Math.abs(dx) > Math.abs(dy) ? { x: count > 1 ? dx : dx / 4, y: 0 } : { x: 0, y: Math.max(0, dy) };
    }
  }

  function handlePointerEnd(event: PointerEvent): void {
    if (!pointers.delete(event.pointerId) || pointers.size > 0) return;
    const ended = gesture;
    const offset = swipe;
    gesture = null;
    dragging = false;
    animate = true;
    swipe = { x: 0, y: 0 };
    if (!ended || ended.multi || event.type === "pointercancel") return;
    if (ended.moved) {
      const action = viewerSwipeAction(offset, viewport);
      if (action === "close") onClose();
      else if (action === "next") step(1);
      else if (action === "previous") step(-1);
      return;
    }
    const point = localPoint(event);
    if (ended.pointerType === "mouse") {
      if (ended.onImage) setTransform(toggleViewerZoom(transform, point, imageSize, viewport), true);
      else if (!zoomed) onClose();
      return;
    }
    const now = performance.now();
    if (lastTap && now - lastTap.time < DOUBLE_TAP_MS
      && Math.hypot(point.x - lastTap.point.x, point.y - lastTap.point.y) < DOUBLE_TAP_SLOP) {
      lastTap = null;
      clearTapTimer();
      setTransform(toggleViewerZoom(transform, point, imageSize, viewport), true);
      return;
    }
    lastTap = { time: now, point };
    clearTapTimer();
    if (!ended.onImage && !zoomed) {
      tapTimer = setTimeout(() => {
        tapTimer = null;
        lastTap = null;
        onClose();
      }, DOUBLE_TAP_MS);
    }
  }

  /** Wheel zoom needs a listener that can cancel page scrolling. */
  function wheelZoom(node: HTMLElement) {
    const handle = (event: WheelEvent): void => {
      event.preventDefault();
      const factor = viewerWheelFactor(event.deltaY, event.deltaMode, viewport.height, event.ctrlKey);
      setTransform(zoomViewerBy(transform, factor, imageSize, viewport, localPoint(event)), false);
    };
    node.addEventListener("wheel", handle, { passive: false });
    return { destroy: () => node.removeEventListener("wheel", handle) };
  }

  function trapTab(event: KeyboardEvent): void {
    if (!root) return;
    const focusable = [...root.querySelectorAll<HTMLElement>("button:not([disabled])")]
      .filter((element) => element.offsetParent !== null);
    const first = focusable[0];
    const last = focusable.at(-1);
    if (!first || !last) {
      event.preventDefault();
      root.focus();
      return;
    }
    const active = document.activeElement;
    if (!root.contains(active) || active === root) {
      event.preventDefault();
      (event.shiftKey ? last : first).focus();
    } else if (event.shiftKey && active === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  }

  /** Runs the viewer shortcut for a key, returning whether it handled the key. */
  function runShortcut(event: KeyboardEvent): boolean {
    const modifier = event.ctrlKey || event.metaKey;
    if (modifier && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "c") {
      if (!canCopy) return false;
      copy();
      return true;
    }
    if (modifier || event.altKey) return false;
    switch (event.key) {
      case "Escape":
        onClose();
        return true;
      case "ArrowLeft":
        if (zoomed) setTransform(panViewer(transform, { x: KEY_PAN, y: 0 }, imageSize, viewport), true);
        else step(-1);
        return true;
      case "ArrowRight":
        if (zoomed) setTransform(panViewer(transform, { x: -KEY_PAN, y: 0 }, imageSize, viewport), true);
        else step(1);
        return true;
      case "ArrowUp":
      case "ArrowDown":
        if (!zoomed) return false;
        setTransform(panViewer(transform, { x: 0, y: event.key === "ArrowUp" ? KEY_PAN : -KEY_PAN }, imageSize, viewport), true);
        return true;
      case "Home":
        show(0);
        return true;
      case "End":
        show(count - 1);
        return true;
      case "+":
      case "=":
        zoomBy(QUICK_NOTE_VIEWER_ZOOM_STEP);
        return true;
      case "-":
      case "_":
        zoomBy(1 / QUICK_NOTE_VIEWER_ZOOM_STEP);
        return true;
      case "0":
        fit();
        return true;
      case "Delete":
      case "Backspace":
        if (!canRemove) return false;
        remove();
        return true;
      default:
        return false;
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab") {
      trapTab(event);
      return;
    }
    if (event.isComposing || !runShortcut(event)) return;
    event.preventDefault();
    event.stopPropagation();
  }

  onMount(() => {
    const returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const returnContainer = returnFocus?.closest<HTMLElement>("[role='dialog']") ?? null;
    const deactivateMobileBack = mobileBackStack.activate({ handle: onClose });
    window.addEventListener("keydown", handleKeydown, true);
    const observer = new ResizeObserver(() => {
      if (stage) viewport = { width: stage.clientWidth, height: stage.clientHeight };
    });
    if (stage) {
      viewport = { width: stage.clientWidth, height: stage.clientHeight };
      observer.observe(stage);
    }
    void tick().then(() => root?.focus());
    return () => {
      observer.disconnect();
      deactivateMobileBack();
      window.removeEventListener("keydown", handleKeydown, true);
      clearTapTimer();
      if (noticeTimer) clearTimeout(noticeTimer);
      // The editor stays inert until this viewer is gone, so focus returns once both have updated.
      queueMicrotask(() => {
        if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
        else returnContainer?.focus({ preventScroll: true });
      });
    };
  });

  const controlClass = "flex size-10 shrink-0 items-center justify-center rounded-full transition-colors hover:bg-white/12 focus-visible:outline-2 focus-visible:outline-white disabled:opacity-40 disabled:hover:bg-transparent";
  const sideClass = "absolute top-1/2 flex size-11 -translate-y-1/2 items-center justify-center rounded-full bg-black/45 transition-colors hover:bg-black/70 focus-visible:outline-2 focus-visible:outline-white";
</script>

<div
  use:portal
  bind:this={root}
  class="quick-note-image-viewer fixed inset-0 z-90 flex flex-col text-white outline-none"
  style:background-color={`rgb(0 0 0 / ${backdropAlpha})`}
  role="dialog"
  aria-modal="true"
  aria-label={t("quickNotes.images.viewer.title")}
  tabindex="-1"
  data-app-shortcuts="ignore"
>
  <header class="@container relative z-10 flex min-h-12 shrink-0 items-center gap-0.5 px-1.5">
    <span class="min-w-0 flex-1 truncate px-2 text-sm tabular-nums opacity-80">
      {#if count > 1}{t("quickNotes.images.viewer.counter", shownIndex + 1, count)}{/if}
    </span>
    <div class="flex items-center gap-0.5 @max-sm:hidden">
      <button class={controlClass} type="button" disabled={!zoomed} aria-label={t("quickNotes.images.viewer.zoomOut")} title={t("quickNotes.images.viewer.zoomOut")} onclick={() => zoomBy(1 / QUICK_NOTE_VIEWER_ZOOM_STEP)}>
        <ZoomOut class="size-5" strokeWidth={1.5} aria-hidden="true" />
      </button>
      <button class="min-h-10 min-w-14 rounded-full px-2 text-xs tabular-nums transition-colors hover:bg-white/12 focus-visible:outline-2 focus-visible:outline-white" type="button" aria-label={t("quickNotes.images.viewer.fit")} title={t("quickNotes.images.viewer.fit")} onclick={fit}>
        {zoomLabel}
      </button>
      <button class={controlClass} type="button" disabled={atMaxZoom} aria-label={t("quickNotes.images.viewer.zoomIn")} title={t("quickNotes.images.viewer.zoomIn")} onclick={() => zoomBy(QUICK_NOTE_VIEWER_ZOOM_STEP)}>
        <ZoomIn class="size-5" strokeWidth={1.5} aria-hidden="true" />
      </button>
      <span class="mx-1 h-5 w-px bg-white/20"></span>
    </div>
    {#if canCopy}
      <button class={controlClass} type="button" disabled={!image} aria-label={t("quickNotes.images.viewer.copy")} title={t("quickNotes.images.viewer.copy")} onclick={copy}>
        <Copy class="size-5" strokeWidth={1.5} aria-hidden="true" />
      </button>
    {/if}
    {#if canDownload}
      <button class={controlClass} type="button" disabled={!image || downloading} aria-label={t("quickNotes.images.viewer.download")} title={t("quickNotes.images.viewer.download")} onclick={() => void download()}>
        <Download class="size-5" strokeWidth={1.5} aria-hidden="true" />
      </button>
    {/if}
    {#if canRemove}
      <button class={controlClass} type="button" aria-label={t("quickNotes.images.viewer.remove")} title={t("quickNotes.images.viewer.remove")} onclick={remove}>
        <Trash2 class="size-5" strokeWidth={1.5} aria-hidden="true" />
      </button>
    {/if}
    <button class={controlClass} type="button" aria-label={t("quickNotes.images.viewer.close")} title={t("quickNotes.images.viewer.close")} onclick={onClose}>
      <X class="size-5" strokeWidth={1.5} aria-hidden="true" />
    </button>
  </header>

  <div class="relative min-h-0 flex-1">
    <div
      bind:this={stage}
      use:wheelZoom
      class="absolute inset-0 touch-none overflow-hidden select-none"
      class:cursor-grab={zoomed && !dragging}
      class:cursor-grabbing={zoomed && dragging}
      role="presentation"
      onpointerdown={handlePointerDown}
      onpointermove={handlePointerMove}
      onpointerup={handlePointerEnd}
      onpointercancel={handlePointerEnd}
    >
      {#if image}
        <div
          class="quick-note-viewer-frame absolute top-1/2 left-1/2"
          class:cursor-zoom-in={!zoomed}
          class:quick-note-viewer-animate={animate && !dragging}
          style={frameStyle}
        >
          {#if thumbnailUrl && !fullDecoded}
            <img src={thumbnailUrl} alt="" draggable="false" class="absolute inset-0 size-full" />
          {/if}
          {#if fullUrl}
            <img
              src={fullUrl}
              alt={t("quickNotes.images.image", shownIndex + 1, count)}
              draggable="false"
              decoding="async"
              class="absolute inset-0 size-full"
              class:opacity-0={!fullDecoded}
              onload={() => { fullDecoded = true; }}
            />
          {/if}
        </div>
        {#if failure}
          <div class="pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-2 px-6 text-center text-sm">
            {#if failure === "unavailable"}
              <CloudDownload class="size-8 opacity-70" strokeWidth={1.25} aria-hidden="true" />
              <p class="max-w-xs opacity-85">{t("quickNotes.images.unavailable")}</p>
            {:else}
              <ImageOff class="size-8 opacity-70" strokeWidth={1.25} aria-hidden="true" />
              <p class="max-w-xs opacity-85">{t("quickNotes.images.loadFailed")}</p>
            {/if}
          </div>
        {:else if !fullDecoded && !thumbnailUrl}
          <div class="pointer-events-none absolute inset-0 flex items-center justify-center" role="status" aria-label={t("quickNotes.images.loading")}>
            <LoaderCircle class="size-7 animate-spin opacity-75" strokeWidth={1.5} aria-hidden="true" />
          </div>
        {/if}
      {/if}
    </div>

    {#if count > 1}
      <button class={`${sideClass} left-2`} type="button" aria-label={t("quickNotes.images.viewer.previous")} title={t("quickNotes.images.viewer.previous")} onclick={() => step(-1)}>
        <ChevronLeft class="size-6" strokeWidth={1.5} aria-hidden="true" />
      </button>
      <button class={`${sideClass} right-2`} type="button" aria-label={t("quickNotes.images.viewer.next")} title={t("quickNotes.images.viewer.next")} onclick={() => step(1)}>
        <ChevronRight class="size-6" strokeWidth={1.5} aria-hidden="true" />
      </button>
    {/if}

    <div class="pointer-events-none absolute inset-x-0 bottom-4 flex justify-center px-4" role="status" aria-live="polite">
      {#if notice}
        <span class="rounded-full bg-black/75 px-4 py-2 text-sm">{notice}</span>
      {/if}
    </div>
  </div>
</div>

<style>
  .quick-note-image-viewer {
    padding: var(--safe-area-top, 0px) var(--safe-area-right, 0px) var(--safe-area-bottom, 0px) var(--safe-area-left, 0px);
  }

  .quick-note-viewer-animate {
    transition: width 160ms ease-out, height 160ms ease-out, transform 160ms ease-out;
  }

  @media (prefers-reduced-motion: reduce) {
    .quick-note-viewer-animate {
      transition: none;
    }
  }
</style>
