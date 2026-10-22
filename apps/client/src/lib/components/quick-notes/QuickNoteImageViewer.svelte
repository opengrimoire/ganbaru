<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import CloudDownload from "@lucide/svelte/icons/cloud-download";
  import Copy from "@lucide/svelte/icons/copy";
  import Download from "@lucide/svelte/icons/download";
  import ImageOff from "@lucide/svelte/icons/image-off";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Trash2 from "@lucide/svelte/icons/trash";
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
    panViewer,
    pinchViewer,
    stepViewerIndex,
    toggleViewerZoom,
    viewerFit,
    viewerFitPadding,
    viewerFlight,
    viewerFrameClip,
    viewerIndexAfterRemoval,
    viewerIsZoomed,
    viewerLayoutTransform,
    viewerMaxScale,
    viewerPointOnImage,
    viewerRelativeZoom,
    viewerSwipeAction,
    viewerTargetVisible,
    viewerWheelFactor,
    zoomViewerBy,
    type ViewerInsets,
    type ViewerPoint,
    type ViewerRect,
    type ViewerSize,
    type ViewerTransform,
    type ViewerViewport,
  } from "$lib/quick-notes/image-viewer";
  import type { QuickNoteImage } from "$lib/quick-notes/types";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { imageBlobToPng, imageClipboardSupported, writePngToClipboard } from "$lib/utils/clipboard-images";
  import { portal } from "$lib/utils/portal";

  let {
    images,
    startIndex,
    readOnly = false,
    originRect,
    onRemove,
    onClose,
  }: {
    images: readonly QuickNoteImage[];
    startIndex: number;
    readOnly?: boolean;
    /** Screen rectangle of the tile that shows an image, so opening and closing fly from and back to it. */
    originRect?: (index: number) => DOMRect | null;
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
  const OPEN_MS = 220;
  const CLOSE_MS = 180;
  const FLIGHT_EASING = "cubic-bezier(0.2, 0, 0, 1)";
  /** Scale the image grows from or shrinks to when there is no tile to fly from or back to. */
  const FADE_SCALE = 0.96;

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
  let backdrop = $state<HTMLDivElement | null>(null);
  let stage = $state<HTMLDivElement | null>(null);
  let layer = $state<HTMLDivElement | null>(null);
  let frame = $state<HTMLDivElement | null>(null);
  let chrome = $state<HTMLDivElement | null>(null);
  let index = $state(untrack(() => startIndex));
  let stageSize = $state.raw<ViewerSize>({ width: 0, height: 0 });
  let transform = $state.raw<ViewerTransform>({ scale: 1, x: 0, y: 0 });
  let placed = $state(false);
  let animate = $state(false);
  let dragging = $state(false);
  let closing = $state(false);
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
  let flights: Animation[] = [];
  let closed = false;
  const pointers = new Map<number, ViewerPoint>();

  const count = $derived(images.length);
  const shownIndex = $derived(Math.min(index, Math.max(0, count - 1)));
  const image = $derived(images[shownIndex]);
  const sha256 = $derived(image?.sha256 ?? "");
  const imageSize = $derived<ViewerSize>(image ? { width: image.width, height: image.height } : { width: 1, height: 1 });
  /** The stage, with room kept clear of the toolbar and arrows while the image is fitted. */
  const viewport = $derived<ViewerViewport>({ ...stageSize, padding: viewerFitPadding(stageSize, count > 1) });
  const zoomed = $derived(viewerIsZoomed(transform, imageSize, viewport));
  const atMaxZoom = $derived(transform.scale >= viewerMaxScale(imageSize, viewport) * 0.999);
  const thumbnailUrl = $derived(thumbnailState.status === "ready" ? thumbnailState.url : null);
  const fullUrl = $derived(fullState.status === "ready" ? fullState.url : null);
  const failure = $derived(fullState.status === "failed" ? fullState.code : null);
  const canRemove = $derived(!readOnly && onRemove !== undefined);
  const zoomLabel = $derived(formatNumber(
    localization.locale,
    viewerRelativeZoom(transform, imageSize, viewport),
    { style: "percent", maximumFractionDigits: 0 },
  ));
  /** The page behind shows through as a downward swipe gets closer to closing. */
  const backdropOpacity = $derived(1 - Math.min(0.6, swipe.y / Math.max(1, stageSize.height)));
  const frameStyle = $derived([
    `width: ${imageSize.width * transform.scale}px`,
    `height: ${imageSize.height * transform.scale}px`,
    `transform: translate(-50%, -50%) translate(${transform.x + swipe.x}px, ${transform.y + swipe.y}px)`,
  ].join("; "));

  $effect(() => {
    if (count === 0) untrack(finishClose);
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

  // A new image starts fitted once the stage is measured; a resize refits a fitted image and keeps a zoomed one in bounds.
  let placedSha = "";
  let placedViewport: ViewerViewport | null = null;
  $effect(() => {
    const key = sha256;
    const size = viewport;
    const shape = imageSize;
    untrack(() => {
      const current = key === placedSha && placedViewport ? { transform, viewport: placedViewport } : null;
      const next = viewerLayoutTransform(current, shape, size);
      if (!next) return;
      if (key !== placedSha) animate = false;
      placedSha = key;
      placedViewport = size;
      transform = next;
      placed = true;
    });
  });

  function setTransform(next: ViewerTransform, animated: boolean): void {
    animate = animated;
    transform = next;
  }

  function center(): ViewerPoint {
    return { x: stageSize.width / 2, y: stageSize.height / 2 };
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
    if (next === null) finishClose();
    else index = next;
  }

  /** Where the shown image sits on the stage, including an unfinished swipe. */
  function frameRect(): ViewerRect {
    const width = imageSize.width * transform.scale;
    const height = imageSize.height * transform.scale;
    return {
      x: stageSize.width / 2 + transform.x + swipe.x - width / 2,
      y: stageSize.height / 2 + transform.y + swipe.y - height / 2,
      width,
      height,
    };
  }

  /** The shown image's tile on the stage, when it is on screen. */
  function tileRect(): ViewerRect | null {
    const rect = originRect?.(shownIndex);
    const box = stage?.getBoundingClientRect();
    if (!rect || !box) return null;
    const target = { x: rect.left - box.left, y: rect.top - box.top, width: rect.width, height: rect.height };
    return viewerTargetVisible(target, stageSize) ? target : null;
  }

  function clipInset(insets: ViewerInsets, radius: string): string {
    return `inset(${insets.top}px ${insets.right}px ${insets.bottom}px ${insets.left}px round ${radius})`;
  }

  /**
   * Moves the image between its tile and the stage, fading the backdrop and controls with it. Without a visible
   * tile the image grows from or shrinks toward the stage center instead.
   */
  function fly(direction: "open" | "close"): Animation[] {
    for (const flight of flights) flight.cancel();
    flights = [];
    if (!layer || !backdrop || !chrome || !placed) return flights;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return flights;
    const resting = frameRect();
    const target = tileRect();
    const flight = target ? viewerFlight(resting, target, stageSize) : null;
    const identity = "translate(0px, 0px) scale(1)";
    let rest: Keyframe;
    let away: Keyframe;
    if (flight) {
      const radius = frame ? getComputedStyle(frame).borderTopLeftRadius : "0px";
      rest = { transform: identity, clipPath: clipInset(viewerFrameClip(resting, stageSize), radius) };
      away = {
        transform: `translate(${flight.x}px, ${flight.y}px) scale(${flight.scale})`,
        clipPath: clipInset(flight.clip, "0px"),
      };
    } else {
      const shiftX = ((1 - FADE_SCALE) * stageSize.width) / 2;
      const shiftY = ((1 - FADE_SCALE) * stageSize.height) / 2;
      rest = { transform: identity, opacity: 1 };
      away = { transform: `translate(${shiftX}px, ${shiftY}px) scale(${FADE_SCALE})`, opacity: 0 };
    }
    const opening = direction === "open";
    const options: KeyframeAnimationOptions = {
      duration: opening ? OPEN_MS : CLOSE_MS,
      easing: FLIGHT_EASING,
      fill: opening ? "none" : "forwards",
    };
    const backdropFrom = opening ? 0 : Number(getComputedStyle(backdrop).opacity);
    flights = [
      layer.animate(opening ? [away, rest] : [rest, away], options),
      backdrop.animate([{ opacity: backdropFrom }, { opacity: opening ? 1 : 0 }], options),
      chrome.animate([{ opacity: opening ? 0 : 1 }, { opacity: opening ? 1 : 0 }], options),
    ];
    return flights;
  }

  function finishClose(): void {
    if (closed) return;
    closed = true;
    onClose();
  }

  /** Closes after flying the image back to its tile. */
  function requestClose(): void {
    if (closing || closed) return;
    closing = true;
    clearTapTimer();
    const running = fly("close");
    if (running.length === 0) finishClose();
    else void Promise.allSettled(running.map((flight) => flight.finished)).then(finishClose);
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
    if (closing || (event.pointerType === "mouse" && event.button !== 0)) return;
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
    gesture = null;
    dragging = false;
    const completed = ended !== null && !ended.multi && event.type !== "pointercancel";
    const action = completed && ended.moved ? viewerSwipeAction(swipe, stageSize) : null;
    if (action === "close") {
      // The swipe offset stays so the image flies back from where the finger left it.
      requestClose();
      return;
    }
    animate = true;
    swipe = { x: 0, y: 0 };
    if (!completed) return;
    if (ended.moved) {
      if (action === "next") step(1);
      else if (action === "previous") step(-1);
      return;
    }
    const point = localPoint(event);
    if (ended.pointerType === "mouse") {
      if (ended.onImage) setTransform(toggleViewerZoom(transform, point, imageSize, viewport), true);
      else if (!zoomed) requestClose();
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
        requestClose();
      }, DOUBLE_TAP_MS);
    }
  }

  /** Wheel zoom needs a listener that can cancel page scrolling. */
  function wheelZoom(node: HTMLElement) {
    const handle = (event: WheelEvent): void => {
      event.preventDefault();
      if (closing) return;
      const factor = viewerWheelFactor(event.deltaY, event.deltaMode, stageSize.height, event.ctrlKey);
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
        requestClose();
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
    // Keys stay with the viewer while it closes, so Escape does not also reach the editor.
    if (event.isComposing || (!closing && !runShortcut(event))) return;
    event.preventDefault();
    event.stopPropagation();
  }

  onMount(() => {
    const returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const returnContainer = returnFocus?.closest<HTMLElement>("[role='dialog']") ?? null;
    const deactivateMobileBack = mobileBackStack.activate({ handle: requestClose });
    window.addEventListener("keydown", handleKeydown, true);
    const observer = new ResizeObserver(() => {
      if (stage) stageSize = { width: stage.clientWidth, height: stage.clientHeight };
    });
    if (stage) {
      stageSize = { width: stage.clientWidth, height: stage.clientHeight };
      observer.observe(stage);
    }
    // The fit runs in this update, so the flight starts from the image's resting place before the first paint.
    void tick().then(() => {
      if (!closing) fly("open");
      root?.focus();
    });
    return () => {
      closed = true;
      for (const flight of flights) flight.cancel();
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

  const controlClass = "flex size-8 shrink-0 items-center justify-center rounded-floating-item text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-muted-foreground pointer-coarse:size-11";
  const sideClass = "surface-floating pointer-events-auto absolute top-1/2 flex size-10 -translate-y-1/2 items-center justify-center text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:outline-2 focus-visible:outline-ring pointer-coarse:size-11";
</script>

<div
  use:portal
  bind:this={root}
  class="quick-note-image-viewer @container fixed inset-0 z-90 outline-none"
  class:pointer-events-none={closing}
  role="dialog"
  aria-modal="true"
  aria-label={t("quickNotes.images.viewer.title")}
  tabindex="-1"
  data-app-shortcuts="ignore"
>
  <div bind:this={backdrop} class="surface-backdrop absolute inset-0" style:opacity={backdropOpacity}></div>

  <div class="relative size-full">
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
        <div bind:this={layer} class="absolute inset-0 origin-top-left">
          <div
            bind:this={frame}
            class="absolute top-1/2 left-1/2 overflow-hidden rounded-floating bg-card shadow-floating"
            class:invisible={!placed}
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
        </div>
        {#if failure}
          <div class="pointer-events-none absolute inset-0 flex items-center justify-center px-6">
            <div class="surface-floating flex max-w-xs flex-col items-center gap-2 px-4 py-3 text-center">
              {#if failure === "unavailable"}
                <CloudDownload class="size-7 text-muted-foreground" strokeWidth={1.25} aria-hidden="true" />
                <p>{t("quickNotes.images.unavailable")}</p>
              {:else}
                <ImageOff class="size-7 text-muted-foreground" strokeWidth={1.25} aria-hidden="true" />
                <p>{t("quickNotes.images.loadFailed")}</p>
              {/if}
            </div>
          </div>
        {:else if !fullDecoded && !thumbnailUrl}
          <div class="pointer-events-none absolute inset-0 flex items-center justify-center" role="status" aria-label={t("quickNotes.images.loading")}>
            <span class="surface-floating flex p-2">
              <LoaderCircle class="size-6 animate-spin text-muted-foreground" strokeWidth={1.5} aria-hidden="true" />
            </span>
          </div>
        {/if}
      {/if}
    </div>

    <div bind:this={chrome} class="pointer-events-none absolute inset-0">
      <div class="surface-floating surface-floating-body pointer-events-auto absolute top-3 right-3 flex items-center gap-0.5">
        {#if count > 1}
          <span class="px-1.5 text-xs text-muted-foreground tabular-nums">{t("quickNotes.images.viewer.counter", shownIndex + 1, count)}</span>
        {/if}
        <div class="flex items-center gap-0.5 @max-sm:hidden">
          <button class={controlClass} type="button" disabled={!zoomed} aria-label={t("quickNotes.images.viewer.zoomOut")} title={t("quickNotes.images.viewer.zoomOut")} onclick={() => zoomBy(1 / QUICK_NOTE_VIEWER_ZOOM_STEP)}>
            <ZoomOut class="size-4" strokeWidth={1.75} aria-hidden="true" />
          </button>
          <button class="h-8 min-w-12 rounded-floating-item px-1.5 text-xs text-muted-foreground tabular-nums transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:outline-2 focus-visible:outline-ring pointer-coarse:h-11" type="button" aria-label={t("quickNotes.images.viewer.fit")} title={t("quickNotes.images.viewer.fit")} onclick={fit}>
            {zoomLabel}
          </button>
          <button class={controlClass} type="button" disabled={atMaxZoom} aria-label={t("quickNotes.images.viewer.zoomIn")} title={t("quickNotes.images.viewer.zoomIn")} onclick={() => zoomBy(QUICK_NOTE_VIEWER_ZOOM_STEP)}>
            <ZoomIn class="size-4" strokeWidth={1.75} aria-hidden="true" />
          </button>
          <span class="mx-1 h-4 w-px bg-border"></span>
        </div>
        {#if canCopy}
          <button class={controlClass} type="button" disabled={!image} aria-label={t("quickNotes.images.viewer.copy")} title={t("quickNotes.images.viewer.copy")} onclick={copy}>
            <Copy class="size-4" strokeWidth={1.75} aria-hidden="true" />
          </button>
        {/if}
        {#if canDownload}
          <button class={controlClass} type="button" disabled={!image || downloading} aria-label={t("quickNotes.images.viewer.download")} title={t("quickNotes.images.viewer.download")} onclick={() => void download()}>
            <Download class="size-4" strokeWidth={1.75} aria-hidden="true" />
          </button>
        {/if}
        {#if canRemove}
          <button class={controlClass} type="button" aria-label={t("quickNotes.images.viewer.remove")} title={t("quickNotes.images.viewer.remove")} onclick={remove}>
            <Trash2 class="size-4" strokeWidth={1.75} aria-hidden="true" />
          </button>
        {/if}
        <button class={controlClass} type="button" aria-label={t("quickNotes.images.viewer.close")} title={t("quickNotes.images.viewer.close")} onclick={requestClose}>
          <X class="size-4" strokeWidth={1.75} aria-hidden="true" />
        </button>
      </div>

      {#if count > 1}
        <button class={`${sideClass} left-3`} type="button" aria-label={t("quickNotes.images.viewer.previous")} title={t("quickNotes.images.viewer.previous")} onclick={() => step(-1)}>
          <ChevronLeft class="size-5" strokeWidth={1.75} aria-hidden="true" />
        </button>
        <button class={`${sideClass} right-3`} type="button" aria-label={t("quickNotes.images.viewer.next")} title={t("quickNotes.images.viewer.next")} onclick={() => step(1)}>
          <ChevronRight class="size-5" strokeWidth={1.75} aria-hidden="true" />
        </button>
      {/if}

      <div class="absolute inset-x-0 bottom-4 flex justify-center px-4" role="status" aria-live="polite">
        {#if notice}
          <span class="surface-floating px-3 py-1.5">{notice}</span>
        {/if}
      </div>
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
