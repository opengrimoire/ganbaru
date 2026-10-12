<script lang="ts">
  import { untrack } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  const { t } = getLocalization();

  let {
    scrollContainer,
    getMaxScrollLeft,
    onScrollPositionChange,
  }: {
    scrollContainer: HTMLElement | null | undefined;
    getMaxScrollLeft?: () => number;
    onScrollPositionChange?: (scrollLeft: number) => number;
  } = $props();

  const SCROLLBAR_VISIBILITY_THRESHOLD_PX = 2;
  const MIN_THUMB_SIZE_PX = 24;
  const TRACK_EDGE_INSET_PX = 8;
  const TRACK_GUTTER_PX = 8;

  type ScrollbarAxis = "horizontal" | "vertical";

  interface DragState {
    axis: ScrollbarAxis;
    startPointerPosition: number;
    startScrollPosition: number;
  }

  let horizontalTrackEl: HTMLDivElement | undefined = $state();
  let verticalTrackEl: HTMLDivElement | undefined = $state();
  let horizontalThumbLeft = $state(0);
  let horizontalThumbWidth = $state(0);
  let verticalThumbTop = $state(0);
  let verticalThumbHeight = $state(0);
  let dragging = $state<DragState | null>(null);
  let hoveredAxis = $state<ScrollbarAxis | null>(null);

  function maxHorizontalScrollLeft(): number {
    if (!scrollContainer) return 0;
    return getMaxScrollLeft?.() ?? Math.max(0, scrollContainer.scrollWidth - scrollContainer.clientWidth);
  }

  function updateHorizontalThumb(): void {
    if (!scrollContainer || !horizontalTrackEl) return;

    const { scrollLeft, clientWidth } = scrollContainer;
    const scrollRange = maxHorizontalScrollLeft();
    if (scrollRange <= SCROLLBAR_VISIBILITY_THRESHOLD_PX) {
      horizontalThumbLeft = 0;
      horizontalThumbWidth = 0;
      return;
    }

    const trackWidth = horizontalTrackEl.clientWidth;
    const ratio = clientWidth / (clientWidth + scrollRange);
    const thumbWidth = Math.min(trackWidth, Math.max(ratio * trackWidth, MIN_THUMB_SIZE_PX));
    horizontalThumbWidth = thumbWidth;
    horizontalThumbLeft = scrollRange > 0
      ? (Math.min(scrollLeft, scrollRange) / scrollRange) * (trackWidth - thumbWidth)
      : 0;
  }

  function updateVerticalThumb(): void {
    if (!scrollContainer || !verticalTrackEl) return;

    const { scrollTop, scrollHeight, clientHeight } = scrollContainer;
    const scrollRange = scrollHeight - clientHeight;
    if (scrollRange <= SCROLLBAR_VISIBILITY_THRESHOLD_PX) {
      verticalThumbTop = 0;
      verticalThumbHeight = 0;
      return;
    }

    const trackHeight = verticalTrackEl.clientHeight;
    const ratio = clientHeight / scrollHeight;
    const thumbHeight = Math.min(trackHeight, Math.max(ratio * trackHeight, MIN_THUMB_SIZE_PX));
    verticalThumbHeight = thumbHeight;
    verticalThumbTop = scrollRange > 0
      ? (scrollTop / scrollRange) * (trackHeight - thumbHeight)
      : 0;
  }

  function updateThumbs(): void {
    updateHorizontalThumb();
    updateVerticalThumb();
  }

  function setHorizontalScrollLeft(nextScrollLeft: number): void {
    if (!scrollContainer) return;
    const scrollRange = maxHorizontalScrollLeft();
    const clampedScrollLeft = Math.max(0, Math.min(scrollRange, nextScrollLeft));
    const actualScrollLeft = onScrollPositionChange?.(clampedScrollLeft) ?? clampedScrollLeft;
    scrollContainer.scrollLeft = actualScrollLeft;
    updateThumbs();
  }

  $effect(() => {
    const el = scrollContainer;
    const horizontalTrack = horizontalTrackEl;
    const verticalTrack = verticalTrackEl;
    if (!el || !horizontalTrack || !verticalTrack) return;

    el.addEventListener("scroll", updateThumbs, { passive: true });

    const refreshGeometry = (): void => {
      onScrollPositionChange?.(el.scrollLeft);
      updateThumbs();
    };

    const observer = new ResizeObserver(refreshGeometry);
    const mutations = new MutationObserver(() => observeContent());

    // Groups can overflow a fixed-width wrapper without resizing that wrapper.
    const observeContent = (): void => {
      observer.disconnect();
      mutations.disconnect();
      for (const target of [el, horizontalTrack, verticalTrack]) observer.observe(target);
      mutations.observe(el, { childList: true });
      const content = el.firstElementChild;
      if (content) {
        observer.observe(content);
        for (const child of content.children) observer.observe(child);
        mutations.observe(content, { childList: true });
      }
      refreshGeometry();
    };
    untrack(observeContent);

    return () => {
      el.removeEventListener("scroll", updateThumbs);
      observer.disconnect();
      mutations.disconnect();
    };
  });

  function handleHorizontalTrackPointerDown(event: PointerEvent): void {
    if (!scrollContainer || !horizontalTrackEl) return;

    event.preventDefault();
    const rect = horizontalTrackEl.getBoundingClientRect();
    const clickX = event.clientX - rect.left;
    const onThumb = clickX >= horizontalThumbLeft && clickX <= horizontalThumbLeft + horizontalThumbWidth;

    if (!onThumb) {
      const trackWidth = horizontalTrackEl.clientWidth;
      const scrollRange = maxHorizontalScrollLeft();
      const thumbRange = trackWidth - horizontalThumbWidth;
      if (thumbRange <= 0) return;
      const targetRatio = (clickX - horizontalThumbWidth / 2) / thumbRange;
      setHorizontalScrollLeft(Math.max(0, Math.min(scrollRange, targetRatio * scrollRange)));
    }

    dragging = {
      axis: "horizontal",
      startPointerPosition: event.clientX,
      startScrollPosition: scrollContainer.scrollLeft,
    };
    horizontalTrackEl.setPointerCapture(event.pointerId);
  }

  function handleVerticalTrackPointerDown(event: PointerEvent): void {
    if (!scrollContainer || !verticalTrackEl) return;

    event.preventDefault();
    const rect = verticalTrackEl.getBoundingClientRect();
    const clickY = event.clientY - rect.top;
    const onThumb = clickY >= verticalThumbTop && clickY <= verticalThumbTop + verticalThumbHeight;

    if (!onThumb) {
      const trackHeight = verticalTrackEl.clientHeight;
      const scrollRange = scrollContainer.scrollHeight - scrollContainer.clientHeight;
      const thumbRange = trackHeight - verticalThumbHeight;
      if (thumbRange <= 0) return;
      const targetRatio = (clickY - verticalThumbHeight / 2) / thumbRange;
      scrollContainer.scrollTop = Math.max(0, Math.min(scrollRange, targetRatio * scrollRange));
    }

    dragging = {
      axis: "vertical",
      startPointerPosition: event.clientY,
      startScrollPosition: scrollContainer.scrollTop,
    };
    verticalTrackEl.setPointerCapture(event.pointerId);
  }

  function handlePointerMove(event: PointerEvent): void {
    if (!dragging || !scrollContainer) return;

    if (dragging.axis === "horizontal") {
      if (!horizontalTrackEl) return;

      const deltaX = event.clientX - dragging.startPointerPosition;
      const trackWidth = horizontalTrackEl.clientWidth;
      const scrollRange = maxHorizontalScrollLeft();
      const thumbRange = trackWidth - horizontalThumbWidth;
      if (thumbRange <= 0) return;

      setHorizontalScrollLeft(dragging.startScrollPosition + (deltaX / thumbRange) * scrollRange);
      return;
    }

    if (!verticalTrackEl) return;

    const deltaY = event.clientY - dragging.startPointerPosition;
    const trackHeight = verticalTrackEl.clientHeight;
    const scrollRange = scrollContainer.scrollHeight - scrollContainer.clientHeight;
    const thumbRange = trackHeight - verticalThumbHeight;
    if (thumbRange <= 0) return;

    scrollContainer.scrollTop = dragging.startScrollPosition + (deltaY / thumbRange) * scrollRange;
  }

  function handlePointerUp(): void {
    dragging = null;
  }

  function wheelDeltaPixels(event: WheelEvent, pageSize: number): number {
    const rawDelta = Math.abs(event.deltaX) > Math.abs(event.deltaY)
      ? event.deltaX
      : event.shiftKey
        ? event.deltaY
        : 0;

    if (event.deltaMode === WheelEvent.DOM_DELTA_LINE) return rawDelta * 16;
    if (event.deltaMode === WheelEvent.DOM_DELTA_PAGE) return rawDelta * pageSize;
    return rawDelta;
  }

  function handleContainerWheel(event: WheelEvent): void {
    if (!scrollContainer) return;

    const maxScrollLeft = maxHorizontalScrollLeft();
    if (maxScrollLeft <= SCROLLBAR_VISIBILITY_THRESHOLD_PX) return;

    const delta = wheelDeltaPixels(event, scrollContainer.clientWidth);
    if (delta === 0) return;

    const currentScrollLeft = scrollContainer.scrollLeft;
    const nextScrollLeft = Math.max(0, Math.min(maxScrollLeft, currentScrollLeft + delta));
    if (nextScrollLeft === currentScrollLeft) return;

    event.preventDefault();
    setHorizontalScrollLeft(nextScrollLeft);
  }

  $effect(() => {
    const el = scrollContainer;
    if (!el) return;

    el.addEventListener("wheel", handleContainerWheel, { passive: false });
    return () => {
      el.removeEventListener("wheel", handleContainerWheel);
    };
  });

  function thumbColor(axis: ScrollbarAxis): string {
    return dragging?.axis === axis || hoveredAxis === axis
      ? "var(--cal-scrollbar-thumb-hover, var(--muted-foreground))"
      : "var(--cal-scrollbar-thumb, var(--muted))";
  }
</script>

<div
  bind:this={verticalTrackEl}
  class="pointer-events-auto absolute right-0 z-40"
  role="presentation"
  aria-hidden="true"
  style="
    top: {TRACK_EDGE_INSET_PX}px;
    bottom: {TRACK_GUTTER_PX}px;
    width: {TRACK_GUTTER_PX}px;
  "
  onpointerdown={handleVerticalTrackPointerDown}
  onpointermove={handlePointerMove}
  onpointerup={handlePointerUp}
  onpointercancel={handlePointerUp}
  onpointerenter={() => (hoveredAxis = "vertical")}
  onpointerleave={() => { if (!dragging) hoveredAxis = null; }}
>
  {#if verticalThumbHeight > 0}
    <div
      class="absolute left-0.5 right-0.5 rounded-full transition-colors duration-150"
      style="
        top: {verticalThumbTop}px;
        height: {verticalThumbHeight}px;
        background-color: {thumbColor("vertical")};
      "
    ></div>
  {/if}
</div>

<div
  bind:this={horizontalTrackEl}
  class="pointer-events-auto absolute bottom-0 z-40"
  data-app-tooltip={t("common.horizontalScrollTooltip")}
  role="presentation"
  aria-hidden="true"
  style="
    left: {TRACK_EDGE_INSET_PX}px;
    right: {TRACK_GUTTER_PX}px;
    height: {TRACK_GUTTER_PX}px;
  "
  onpointerdown={handleHorizontalTrackPointerDown}
  onpointermove={handlePointerMove}
  onpointerup={handlePointerUp}
  onpointercancel={handlePointerUp}
  onpointerenter={() => (hoveredAxis = "horizontal")}
  onpointerleave={() => { if (!dragging) hoveredAxis = null; }}
>
  {#if horizontalThumbWidth > 0}
    <div
      class="absolute bottom-px top-px rounded-full transition-colors duration-150"
      style="
        left: {horizontalThumbLeft}px;
        width: {horizontalThumbWidth}px;
        background-color: {thumbColor("horizontal")};
      "
    ></div>
  {/if}
</div>

<div
  class="pointer-events-none absolute bottom-0 right-0 z-40 h-2 w-2"
  style="background-color: var(--cal-bg);"
></div>
