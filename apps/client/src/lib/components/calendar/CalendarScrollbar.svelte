<script lang="ts">
  import { useCalendarZoom } from "$lib/stores/calendar-zoom.svelte";
  import {
    CALENDAR_SCROLLBAR_TRACK_PX,
    calendarScrollbarThumb,
    CALENDAR_FORWARDED_WHEEL_EVENT,
    getSmoothScrollDelta,
    type CalendarForwardedWheelDetail,
  } from "$lib/calendar/timeline-scroll";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";

  let {
    scrollContainer,
    stickyTop = 0,
    stickyBottom = 0,
    onTimelineWheel,
    wheelPassthrough = false,
    activeThumb = false,
  }: {
    scrollContainer: HTMLElement | undefined;
    stickyTop?: number;
    stickyBottom?: number;
    onTimelineWheel?: (e: WheelEvent) => void;
    wheelPassthrough?: boolean;
    activeThumb?: boolean;
  } = $props();

  const calendarZoom = useCalendarZoom();
  /** The mobile shell shows no scrollbars. */
  const visible = BUILD_PLATFORM_PROFILE.shell !== "mobile";

  let trackEl: HTMLDivElement | undefined = $state();
  let thumbTop = $state(0);
  let thumbHeight = $state(0);
  let dragging = $state(false);
  let hovering = $state(false);
  let dragStartY = 0;
  let dragStartScrollTop = 0;

  function updateThumb() {
    // Freeze the scrollbar during zoom animations to avoid forced reflows
    // that cause visual flashes. The scrollbar updates on the next
    // scroll or resize event after the zoom animation completes.
    if (calendarZoom.isAnimating) return;
    if (!scrollContainer || !trackEl) return;
    const thumb = calendarScrollbarThumb(scrollContainer, trackEl.clientHeight);
    thumbTop = thumb.top;
    thumbHeight = thumb.height;
  }

  $effect(() => {
    const el = scrollContainer;
    if (!el) return;
    updateThumb();
    el.addEventListener("scroll", updateThumb, { passive: true });
    el.addEventListener("zoomcommit", updateThumb);
    const resizeObserver = new ResizeObserver(updateThumb);
    const observeContent = () => {
      resizeObserver.disconnect();
      resizeObserver.observe(el);
      for (const child of el.children) resizeObserver.observe(child);
    };
    observeContent();
    // Content swapped in place (a different settings section, a lazily loaded view) changes the scroll height without resizing the container, so the observed children follow the container's current children.
    const childListObserver = new MutationObserver(() => {
      observeContent();
      updateThumb();
    });
    childListObserver.observe(el, { childList: true });
    return () => {
      el.removeEventListener("scroll", updateThumb);
      el.removeEventListener("zoomcommit", updateThumb);
      childListObserver.disconnect();
      resizeObserver.disconnect();
    };
  });

  function handleTrackPointerDown(e: PointerEvent) {
    if (!scrollContainer || !trackEl) return;
    e.preventDefault();
    const rect = trackEl.getBoundingClientRect();
    const clickY = e.clientY - rect.top;
    const onThumb = clickY >= thumbTop && clickY <= thumbTop + thumbHeight;

    if (!onThumb) {
      const trackHeight = trackEl.clientHeight;
      const scrollRange = scrollContainer.scrollHeight - scrollContainer.clientHeight;
      const targetRatio = (clickY - thumbHeight / 2) / (trackHeight - thumbHeight);
      scrollContainer.scrollTop = Math.max(0, Math.min(scrollRange, targetRatio * scrollRange));
    }

    dragging = true;
    dragStartY = e.clientY;
    dragStartScrollTop = scrollContainer.scrollTop;
    trackEl.setPointerCapture(e.pointerId);
  }

  function isPointerInsideTrack(e: PointerEvent): boolean {
    if (!trackEl) return false;
    const rect = trackEl.getBoundingClientRect();
    return e.clientX >= rect.left && e.clientX <= rect.right && e.clientY >= rect.top && e.clientY <= rect.bottom;
  }

  function handlePointerMove(e: PointerEvent) {
    if (!dragging || !scrollContainer || !trackEl) return;
    const deltaY = e.clientY - dragStartY;
    const trackHeight = trackEl.clientHeight;
    const scrollRange = scrollContainer.scrollHeight - scrollContainer.clientHeight;
    const thumbRange = trackHeight - thumbHeight;
    if (thumbRange <= 0) return;
    scrollContainer.scrollTop = dragStartScrollTop + (deltaY / thumbRange) * scrollRange;
  }

  function handlePointerUp() {
    dragging = false;
  }

  function handleWheel(e: WheelEvent) {
    if (wheelPassthrough) return;
    e.preventDefault();
    e.stopPropagation();
    if (scrollContainer) {
      scrollContainer.dispatchEvent(new CustomEvent<CalendarForwardedWheelDetail>(
        CALENDAR_FORWARDED_WHEEL_EVENT,
        {
          detail: {
            deltaY: getSmoothScrollDelta(e),
            deltaMode: e.deltaMode,
            scrollTop: scrollContainer.scrollTop,
            scrollHeight: scrollContainer.scrollHeight,
            clientHeight: scrollContainer.clientHeight,
          },
        },
      ));
      onTimelineWheel?.(e);
    }
  }

  $effect(() => {
    const el = scrollContainer;
    if (!wheelPassthrough || !el) return;

    const handlePointerDown = (e: PointerEvent) => {
      if (!isPointerInsideTrack(e)) return;
      e.stopPropagation();
      handleTrackPointerDown(e);
    };
    const handlePointerMove = (e: PointerEvent) => {
      if (!dragging) hovering = isPointerInsideTrack(e);
    };
    const handlePointerLeave = () => {
      if (!dragging) hovering = false;
    };

    el.addEventListener("pointerdown", handlePointerDown, { capture: true });
    el.addEventListener("pointermove", handlePointerMove, { passive: true });
    el.addEventListener("pointerleave", handlePointerLeave);

    return () => {
      el.removeEventListener("pointerdown", handlePointerDown, { capture: true });
      el.removeEventListener("pointermove", handlePointerMove);
      el.removeEventListener("pointerleave", handlePointerLeave);
    };
  });
</script>

{#if visible}
  <!-- Absolute overlay: positioned outside scroll container so it never scrolls -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={trackEl}
    class="{wheelPassthrough ? 'pointer-events-none' : 'pointer-events-auto'} absolute right-0 z-48"
    style="top: {stickyTop}px; bottom: {stickyBottom}px; width: {CALENDAR_SCROLLBAR_TRACK_PX}px;"
    onpointerdown={handleTrackPointerDown}
    onpointermove={handlePointerMove}
    onpointerup={handlePointerUp}
    onpointerenter={() => (hovering = true)}
    onpointerleave={() => { if (!dragging) hovering = false; }}
    onwheel={handleWheel}
  >
    {#if thumbHeight > 0}
      <div
        class="absolute left-0.5 right-0.5 rounded-full transition-opacity duration-150"
        style="
          top: {thumbTop}px;
          height: {thumbHeight}px;
          background-color: {activeThumb || dragging || hovering
            ? 'var(--cal-scrollbar-thumb-hover, var(--muted-foreground))'
            : 'var(--cal-scrollbar-thumb, var(--muted))'};
        "
      ></div>
    {/if}
  </div>
{/if}
