<script lang="ts">
  import type { CalendarEvent, PositionedEvent, PersistedSegment, TimelineBand } from "$lib/calendar/types";
  import type { CreateStartTiming } from "./timed-event-drag-controller.svelte";
  import {
    layoutEventsForDay,
    effectiveMinuteRange,
    formatDatePart,
    parseCalendarDate,
    snapToGrid,
    snapSimpleClickStartMinute,
    clampMinute,
    getEventColor,
    formatTimeRange,
  } from "$lib/calendar/utils";
  import { computeDayTimelineBands } from "$lib/pomodoro/segments";
  import { isPendingCreateEventId, PENDING_CREATE_ID } from "$lib/components/calendar/display-events";
  import { getPomodoro } from "$lib/stores/pomodoro.svelte";
  import EventBlock from "./EventBlock.svelte";
  import { getEventIndicatorState } from "./event-indicators";
  import { CALENDAR_ZOOM_FRAME_EVENT, getCalendarZoom } from "$lib/stores/calendar-zoom.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { onMount } from "svelte";
  import Repeat from "@lucide/svelte/icons/repeat";
  import Video from "@lucide/svelte/icons/video";
  import MapPin from "@lucide/svelte/icons/map-pin";
  import Users from "@lucide/svelte/icons/users";
  import {
    isThemeCalendarDark,
    type Theme,
  } from "$lib/themes";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  const { t } = getLocalization();

  let {
    date,
    positionedEvents,
    theme,
    isToday = false,
    isPast = false,
    currentTimeMinute = -1,
    dragPreview = null,
    createPreview = null,
    onEventClick,
    onEventPrefetch,
    onDragStart,
    onCreateStart,
    isActiveEvent,
    isEventLocked,
    editingId,
    previewedIds,
    draggingEventId,
    grabbingId,
    didDrag = false,
    persistedSegmentsByEvent = new Map<string, PersistedSegment[]>(),
    visibleStartMinute = 0,
    visibleEndMinute = 1440,
    allowPointerEditing = true,
    mobileLayout = false,
  }: {
    date: Date;
    positionedEvents: PositionedEvent[];
    theme: Theme;
    isToday?: boolean;
    isPast?: boolean;
    currentTimeMinute?: number;
    editingId?: string;
    previewedIds?: Set<string>;
    dragPreview?: PositionedEvent | null;
    createPreview?: PositionedEvent | null;
    draggingEventId?: string;
    grabbingId?: string;
    didDrag?: boolean;
    persistedSegmentsByEvent?: ReadonlyMap<string, PersistedSegment[]>;
    visibleStartMinute?: number;
    visibleEndMinute?: number;
    allowPointerEditing?: boolean;
    mobileLayout?: boolean;
    onEventClick: (event: CalendarEvent, rect?: DOMRect) => void;
    onEventPrefetch?: (event: CalendarEvent) => void;
    onDragStart: (eventId: string, e: PointerEvent, forceEdge?: "resize-top" | "resize-bottom") => void;
    onCreateStart: (dateStr: string, timing: CreateStartTiming, e: PointerEvent) => void;
    isActiveEvent?: (event: CalendarEvent) => boolean;
    isEventLocked?: (eventId: string) => boolean;
  } = $props();

  const isDark = $derived(isThemeCalendarDark(theme));
  const preferences = getPreferences();

  const TIMED_RENDER_BUFFER_MINUTES = 1440;
  const panelOpen = $derived(!!editingId);

  const dateStr = $derived(formatDatePart(date));
  const dayEvents = $derived(positionedEvents.map((positionedEvent) => positionedEvent.event));
  const positioned = $derived(positionedEvents);

  // Layout-aware preview: include drag/create preview in layout computation
  // so overlapping events shift in real time to show the final result.
  const dragPreviewList = $derived(dragPreview ? [dragPreview] : []);
  const previewEvents = $derived([
    ...dragPreviewList.map((preview) => preview.event),
    ...(createPreview ? [createPreview.event] : []),
  ]);

  const layoutWithPreview = $derived.by(() => {
    if (previewEvents.length === 0 && !draggingEventId) {
      return {
        items: positioned,
        dragPreviews: [] as PositionedEvent[],
        createPreview: null as PositionedEvent | null,
      };
    }

    // Exclude dragged and replaced preview events so the preview is laid out
    // in the same column slot the saved event will use.
    const previewEventIds = new Set(previewEvents.map((event) => event.id));
    const baseEvents = draggingEventId
      ? dayEvents.filter(e => e.id !== draggingEventId && !previewEventIds.has(e.id))
      : dayEvents.filter(e => !previewEventIds.has(e.id));

    const eventsForLayout = previewEvents.length > 0
      ? [...baseEvents, ...previewEvents]
      : baseEvents;

    const all = layoutEventsForDay(eventsForLayout, dateStr);

    if (previewEvents.length === 0) {
      return {
        items: all,
        dragPreviews: [] as PositionedEvent[],
        createPreview: null as PositionedEvent | null,
      };
    }

    const previewIds = new Set(previewEvents.map((event) => event.id));
    const createPreviewId = createPreview?.event.id;
    const items: PositionedEvent[] = [];
    const dragPreviews: PositionedEvent[] = [];
    let layoutedCreatePreview: PositionedEvent | null = null;

    for (const p of all) {
      if (previewIds.has(p.event.id)) {
        if (p.event.id === createPreviewId) {
          layoutedCreatePreview = p;
        } else {
          dragPreviews.push(p);
        }
      } else {
        items.push(p);
      }
    }

    return { items, dragPreviews, createPreview: layoutedCreatePreview };
  });

  const effectivePositioned = $derived(layoutWithPreview.items);
  const layoutedPreviews = $derived(layoutWithPreview.dragPreviews);
  const layoutedCreatePreview = $derived(layoutWithPreview.createPreview);
  const layoutAnimationActive = $derived(!!dragPreview || !!createPreview || !!draggingEventId);
  const renderedPositioned = $derived.by(() => {
    const minMinute = Math.max(0, visibleStartMinute - TIMED_RENDER_BUFFER_MINUTES);
    const maxMinute = Math.min(1440, visibleEndMinute + TIMED_RENDER_BUFFER_MINUTES);
    return effectivePositioned.filter((pos) => {
      if (pos.event.id === editingId || pos.event.id === grabbingId) return true;
      if (previewedIds?.has(pos.event.id) === true) return true;
      const start = pos.startMinute;
      const end = pos.startMinute + pos.durationMinutes;
      return end >= minMinute && start <= maxMinute;
    });
  });

  const pomodoro = getPomodoro();
  const calendarZoom = getCalendarZoom();
  const railWidth = 6;

  // One rail segment per contiguous group of pomodoro events (merge overlapping/adjacent)
  const railSegments = $derived.by(() => {
    const ranges: { start: number; end: number }[] = [];
    for (const p of positioned) {
      if (draggingEventId && p.event.id === draggingEventId) continue;
      if (!p.event.pomodoroConfig) continue;
      const { startMinute, endMinute } = effectiveMinuteRange(p.event, dateStr);
      ranges.push({ start: startMinute, end: endMinute });
    }
    // Include drag previews if they have pomodoro config.
    for (const preview of dragPreviewList) {
      if (!preview.event.pomodoroConfig || preview.event.allDay || preview.event.status === "cancelled") continue;
      const { startMinute, endMinute } = effectiveMinuteRange(preview.event, dateStr);
      ranges.push({ start: startMinute, end: endMinute });
    }
    if (ranges.length === 0) return [];
    ranges.sort((a, b) => a.start - b.start);
    const merged: { start: number; end: number }[] = [ranges[0]];
    for (let i = 1; i < ranges.length; i++) {
      const last = merged[merged.length - 1];
      if (ranges[i].start <= last.end) {
        last.end = Math.max(last.end, ranges[i].end);
      } else {
        merged.push(ranges[i]);
      }
    }
    return merged;
  });

  const visibleRailSegments = $derived.by(() => {
    const minMinute = Math.max(0, visibleStartMinute - TIMED_RENDER_BUFFER_MINUTES);
    const maxMinute = Math.min(1440, visibleEndMinute + TIMED_RENDER_BUFFER_MINUTES);
    return railSegments.filter((seg) => seg.end >= minMinute && seg.start <= maxMinute);
  });


  const timelineBands = $derived.by(() => {
    void pomodoro.breakOvertimeSeconds;
    void currentTimeMinute; // re-run every second (even during pause)
    const dayMidnight = parseCalendarDate(`${dateStr} 00:00`);
    const dayStartMs = dayMidnight.getTime();
    const nowMs = Date.now();

    const nowMinuteOfDay = (nowMs - dayStartMs) / 60000;
    const pomodoroEvents = positioned
      .filter((p) => p.event.pomodoroConfig && !p.event.allDay && p.event.status !== "cancelled" && !(draggingEventId && p.event.id === draggingEventId))
      .map((p) => {
        const { startMinute, endMinute } = effectiveMinuteRange(p.event, dateStr);
        let evStartMs = parseCalendarDate(p.event.start).getTime();
        let evStartMinute = startMinute;

        // Create preview (including expanded recurring instances): focus will start
        // at save time (now), not event start. Clamp to now so break marks match.
        if (p.event.id.startsWith(PENDING_CREATE_ID) && evStartMs < nowMs) {
          evStartMs = nowMs;
          evStartMinute = Math.max(startMinute, nowMinuteOfDay);
        }

        return {
          id: p.event.id,
          createdAt: p.event.createdAt,
          config: p.event.pomodoroConfig!,
          startMs: evStartMs,
          endMs: parseCalendarDate(p.event.end).getTime(),
          startMinute: evStartMinute,
          endMinute,
        };
      });
    // Include drag previews for rail band previsualization.
    for (const preview of dragPreviewList) {
      if (!preview.event.pomodoroConfig || preview.event.allDay || preview.event.status === "cancelled") continue;
      const { startMinute, endMinute } = effectiveMinuteRange(preview.event, dateStr);
      pomodoroEvents.push({
        id: preview.event.id,
        createdAt: preview.event.createdAt,
        config: preview.event.pomodoroConfig,
        startMs: parseCalendarDate(preview.event.start).getTime(),
        endMs: parseCalendarDate(preview.event.end).getTime(),
        startMinute,
        endMinute,
      });
    }

    return computeDayTimelineBands(pomodoroEvents, {
      activeOccurrenceId: pomodoro.activeOccurrenceId,
      segments: pomodoro.segments,
      remainingSeconds: pomodoro.remainingSeconds,
      phaseElapsedSeconds: pomodoro.phaseElapsedSeconds,
      phaseWorkDurationSeconds: pomodoro.phaseWorkDurationSeconds,
      currentConfig: pomodoro.currentConfig,
      breakOvertimeSeconds: pomodoro.breakOvertimeSeconds,
    }, dayStartMs, nowMs, persistedSegmentsByEvent);
  });

  function timelineBandKey(band: TimelineBand, index: number): string {
    return `${band.phase}:${band.status}:${band.topMinute.toFixed(3)}:${band.heightMinutes.toFixed(3)}:${index}`;
  }

  let columnEl: HTMLDivElement | undefined = $state();
  let lastClientX: number | null = null;
  let lastClientY: number | null = null;
  let scrollProximityRaf = 0;
  // Event whose top or bottom resize edge is under the pointer.
  let hoverResizeEventId: string | null = $state(null);

  $effect(() => {
    if (dragPreview || createPreview) {
      hoverResizeEventId = null;
    }
  });

  // Re-check resize proximity when the event layout changes, such as after a new event is saved.
  $effect(() => {
    void renderedPositioned;
    if (lastClientX === null || lastClientY === null || !columnEl) return;
    recheckProximity();
  });

  function recheckProximity() {
    if (!columnEl || lastClientX === null || lastClientY === null) return;
    updateHoverStateFromClientPoint(lastClientX, lastClientY);
  }

  onMount(() => {
    function clearMouseState() {
      clearHoverTracking();
    }
    function handleVisibilityChange() {
      if (document.hidden) clearMouseState();
    }
    window.addEventListener("blur", clearMouseState);
    document.addEventListener("visibilitychange", handleVisibilityChange);

    // Track scroll for resize detection during scroll
    const scrollParent = columnEl?.closest(".hide-scrollbar") as HTMLElement | null;
    if (scrollParent) {
      scrollParent.addEventListener("scroll", handleParentScroll);
      scrollParent.addEventListener(CALENDAR_ZOOM_FRAME_EVENT, handleZoomFrame);
    }

    return () => {
      window.removeEventListener("blur", clearMouseState);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      scrollParent?.removeEventListener("scroll", handleParentScroll);
      scrollParent?.removeEventListener(CALENDAR_ZOOM_FRAME_EVENT, handleZoomFrame);
      if (scrollProximityRaf) cancelAnimationFrame(scrollProximityRaf);
    };
  });

  function clearHoverTracking() {
    const hasTracking = lastClientX !== null
      || lastClientY !== null
      || hoverResizeEventId !== null
      || scrollProximityRaf !== 0;

    if (!hasTracking) return;

    lastClientX = null;
    lastClientY = null;
    if (scrollProximityRaf) {
      cancelAnimationFrame(scrollProximityRaf);
      scrollProximityRaf = 0;
    }
    hoverResizeEventId = null;
  }

  function isTimedColumnSurface(target: EventTarget | null): boolean {
    return target instanceof Element
      && target.closest("[data-day-column], [data-day-column-shell]") !== null;
  }

  function getResizeThreshold(): number {
    // Fixed 6px to match the resize handle's visible zone in EventBlock
    // (resize handle has height: 11px, top: -5px, but overflow: hidden clips it to 6px inside)
    return 6;
  }

  function getRenderedHourHeight(): number {
    const scrollContainer = columnEl?.closest(".hide-scrollbar") as HTMLElement | null;
    const raw = scrollContainer?.style.getPropertyValue("--hour-h") ?? "";
    const rendered = raw ? parseFloat(raw) : Number.NaN;
    return Number.isFinite(rendered) && rendered > 0 ? rendered : calendarZoom.hourHeight;
  }

  type EventHit =
    | { kind: "edge"; eventId: string; edge: "resize-top" | "resize-bottom" }
    | { kind: "body"; eventId: string };

  function isPastTimedPosition(pos: PositionedEvent): boolean {
    if (isPendingCreateEventId(pos.event.id)) return false;
    const range = effectiveMinuteRange(pos.event, dateStr);
    return isPast || (isToday && currentTimeMinute >= 0 && range.endMinute <= currentTimeMinute);
  }

  function canResizeEdge(pos: PositionedEvent, edge: "resize-top" | "resize-bottom"): boolean {
    if (isPastTimedPosition(pos)) return false;
    if (isActiveEvent?.(pos.event)) return edge === "resize-bottom";
    if (!isPendingCreateEventId(pos.event.id) && isEventLocked?.(pos.event.id)) return false;
    return true;
  }

  function findEventHit(offsetX: number, offsetY: number, colWidth: number): EventHit | null {
    const hourHeight = getRenderedHourHeight();
    const threshold = getResizeThreshold();

    // Events are positioned inside a container offset by railWidth + 4
    const eventAreaLeft = railWidth + 4;
    const eventAreaWidth = colWidth - eventAreaLeft;

    // Find the event that strictly contains the pointer. Events span [top, bottom),
    // so only one event contains any given point at a boundary.
    for (const pos of renderedPositioned) {
      if (pos.event.id === "__create__") continue;
      if (panelOpen && pos.event.id !== editingId) continue;

      // Don't subtract the gap for hit detection to avoid dead zones between stacked events
      const eventLeftPx = eventAreaLeft + (pos.left / 100) * eventAreaWidth;
      const eventRightPx = eventAreaLeft + ((pos.left + pos.width) / 100) * eventAreaWidth;
      if (offsetX < eventLeftPx || offsetX > eventRightPx) continue;

      const eventTopY = (pos.startMinute / 60) * hourHeight;
      const eventBottomY = ((pos.startMinute + pos.durationMinutes) / 60) * hourHeight;

      // Strict containment: [top, bottom)
      if (offsetY >= eventTopY && offsetY < eventBottomY) {
        // The pointer is inside this event. Check whether it is near an edge.
        if (!pos.isClippedTop && Math.abs(offsetY - eventTopY) < threshold) {
          return canResizeEdge(pos, "resize-top")
            ? { kind: "edge", eventId: pos.event.id, edge: "resize-top" }
            : { kind: "body", eventId: pos.event.id };
        }
        if (!pos.isClippedBottom && Math.abs(offsetY - eventBottomY) < threshold) {
          return canResizeEdge(pos, "resize-bottom")
            ? { kind: "edge", eventId: pos.event.id, edge: "resize-bottom" }
            : { kind: "body", eventId: pos.event.id };
        }
        return { kind: "body", eventId: pos.event.id };
      }
    }

    // No event contains the pointer, so creation is allowed
    return null;
  }

  function findNearbyEventEdge(offsetX: number, offsetY: number, colWidth: number): { eventId: string; edge: "resize-top" | "resize-bottom" } | null {
    const hit = findEventHit(offsetX, offsetY, colWidth);
    return hit?.kind === "edge" ? { eventId: hit.eventId, edge: hit.edge } : null;
  }

  function getCreateTimingFromOffset(offsetY: number): CreateStartTiming {
    const hourHeight = getRenderedHourHeight();
    const rawMinute = (offsetY / hourHeight) * 60;
    const clickMinute = snapSimpleClickStartMinute(rawMinute);
    const selectionMinute = clampMinute(snapToGrid(rawMinute, calendarZoom.gridMinutes));

    return { selectionMinute, clickMinute };
  }

  function updateHoverStateFromClientPoint(
    clientX: number,
    clientY: number,
  ) {
    if (!columnEl) return;

    const colRect = columnEl.getBoundingClientRect();
    const colOffsetX = clientX - colRect.left;
    const colOffsetY = clientY - colRect.top;
    const hit = findEventHit(colOffsetX, colOffsetY, colRect.width);

    if (hit?.kind === "edge") {
      hoverResizeEventId = (panelOpen && hit.eventId !== editingId) ? null : hit.eventId;
    } else {
      hoverResizeEventId = null;
    }
  }

  /** Resize edge of the given event under the pointer, if any. */
  function getEventEdgeFromClick(eventId: string, e: PointerEvent): "resize-top" | "resize-bottom" | undefined {
    if (mobileLayout) return undefined;
    if (!columnEl) return undefined;
    const colRect = columnEl.getBoundingClientRect();
    const colOffsetX = e.clientX - colRect.left;
    const colOffsetY = e.clientY - colRect.top;
    const nearby = findNearbyEventEdge(colOffsetX, colOffsetY, colRect.width);
    if (nearby && nearby.eventId === eventId) {
      return nearby.edge;
    }
    return undefined;
  }

  function handleColumnAreaPointerDown(e: PointerEvent) {
    if (!allowPointerEditing || e.button !== 0 || draggingEventId) return;

    if (!columnEl) return;
    const colRect = columnEl.getBoundingClientRect();
    const colOffsetX = e.clientX - colRect.left;
    const colOffsetY = e.clientY - colRect.top;

    // Resize when the pointer is near an existing event edge
    const nearby = findNearbyEventEdge(colOffsetX, colOffsetY, colRect.width);
    if (nearby) {
      onDragStart(nearby.eventId, e, nearby.edge);
      return;
    }

    // No event creation when panel is open
    if (panelOpen) return;

    const timing = getCreateTimingFromOffset(colOffsetY);

    onCreateStart(dateStr, timing, e);
  }

  function handleParentScroll() {
    if (lastClientY === null || !columnEl) return;
    if (calendarZoom.isAnimating) return;

    if (!scrollProximityRaf) {
      scrollProximityRaf = requestAnimationFrame(() => {
        scrollProximityRaf = 0;
        if (lastClientX !== null && lastClientY !== null) {
          updateHoverStateFromClientPoint(lastClientX, lastClientY);
        }
      });
    }
  }

  function handleZoomFrame() {
    if (lastClientX === null || lastClientY === null || !columnEl) return;
    updateHoverStateFromClientPoint(lastClientX, lastClientY);
  }

  function handleColumnMouseMove(e: MouseEvent) {
    if (!columnEl) return;

    lastClientX = e.clientX;
    lastClientY = e.clientY;
    updateHoverStateFromClientPoint(e.clientX, e.clientY);
  }

  function handleColumnMouseLeave(e: MouseEvent) {
    if (calendarZoom.isAnimating) return;
    const enteringTimedSurface = isTimedColumnSurface(e.relatedTarget);
    if (!enteringTimedSurface) clearHoverTracking();
  }

  function handleRailAreaPointerDown(e: PointerEvent) {
    if (!allowPointerEditing || e.button !== 0 || !columnEl || draggingEventId || panelOpen) return;
    const colRect = columnEl.getBoundingClientRect();
    // Only handle clicks in the rail zone (left of columnEl)
    if (e.clientX >= colRect.left) return;
    const offsetY = e.clientY - colRect.top;
    const rawMinute = (offsetY / getRenderedHourHeight()) * 60;
    if (visibleRailSegments.some(seg => seg.start <= rawMinute && seg.end >= rawMinute)) return;
    const timing = getCreateTimingFromOffset(offsetY);

    onCreateStart(dateStr, timing, e);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  data-day-column
  class="relative z-2 min-w-0 {hoverResizeEventId !== null ? 'cursor-ns-resize' : 'cursor-default'}"
  style="
    height: calc(24 * var(--hour-h) * 1px);
    contain: layout style;
  "
  onmousemove={handleColumnMouseMove}
  onmouseleave={handleColumnMouseLeave}
  onpointerdown={handleRailAreaPointerDown}
>
  <!-- Current time indicator (outside overflow-hidden so the circle can bleed left) -->
  {#if isToday && currentTimeMinute >= 0}
    <div
      class="pointer-events-none absolute left-0 right-0"
      style="top: calc({currentTimeMinute} / 60 * var(--hour-h) * 1px); z-index: 46;"
    >
      <div
        class="h-2.5 w-2.5 shrink-0 rounded-full absolute"
        style="background-color: var(--cal-current-time); left: -5px; top: -5px;"
      ></div>
      <div
        class="absolute left-0 right-0 h-[2.3px]"
        style="background-color: var(--cal-current-time); top: -1.15px;"
      ></div>
    </div>
  {/if}

  <!-- Pomodoro timeline rails (one per contiguous group of pomodoro events) -->
  {#each visibleRailSegments as seg}
    <div
      class="pointer-events-none absolute z-3 overflow-hidden"
      style="
        left: 2px;
        width: {railWidth}px;
        top: calc({seg.start} / 60 * var(--hour-h) * 1px);
        height: calc({seg.end - seg.start} / 60 * var(--hour-h) * 1px);
        background-color: var(--cal-timeline-rail);
        border-radius: 3px;
      "
    >
      <!-- Focus fill bands (green, behind breaks) -->
      {#each timelineBands.filter(b => b.phase === "focus" && b.topMinute < seg.end && b.topMinute + b.heightMinutes > seg.start) as band, bandIndex (timelineBandKey(band, bandIndex))}
        <div
          class="absolute left-0 right-0"
          style="
            top: {((band.topMinute - seg.start) / (seg.end - seg.start)) * 100}%;
            height: {Math.max((band.heightMinutes / (seg.end - seg.start)) * 100, 0)}%;
            background-color: var(--cal-timeline-focus);
          "
        ></div>
      {/each}
      <!-- Break bands (on top of focus fills) -->
      {#each timelineBands.filter(b => b.phase !== "focus" && b.topMinute < seg.end && b.topMinute + b.heightMinutes > seg.start) as band, bandIndex (timelineBandKey(band, bandIndex))}
        <div
          class="absolute left-0 right-0 {band.status === 'active' ? 'break-band-active' : ''}"
          style="
            top: {((band.topMinute - seg.start) / (seg.end - seg.start)) * 100}%;
            height: {Math.max((band.heightMinutes / (seg.end - seg.start)) * 100, 0.5)}%;
            background-color: var(--cal-timeline-break);
          "
        ></div>
      {/each}
    </div>
  {/each}

  <div
    bind:this={columnEl}
    class="absolute top-0 right-0 bottom-0 {draggingEventId ? 'pointer-events-none' : hoverResizeEventId !== null ? 'cursor-ns-resize' : 'cursor-default'}"
    style="left: {railWidth + 4}px;"
    onpointerdown={handleColumnAreaPointerDown}
  >
  <!-- Events (use layout-aware positions that account for drag/create preview) -->
  {#each renderedPositioned as pos (pos.event.id)}
    <EventBlock
      positioned={pos}
      {theme}
      editing={pos.event.id === editingId}
      preview={previewedIds?.has(pos.event.id) === true}
      grabbing={pos.event.id === grabbingId}
      animateLayout={layoutAnimationActive}
      canDrag={allowPointerEditing && (!panelOpen || pos.event.id === editingId)}
      {mobileLayout}
      isPast={!isPendingCreateEventId(pos.event.id) && (
        isPast || (isToday && currentTimeMinute >= 0 && effectiveMinuteRange(pos.event, dateStr).endMinute <= currentTimeMinute)
      )}
      inResizeZone={hoverResizeEventId === pos.event.id}
      onClick={(rect) => { if (!didDrag) onEventClick(pos.event, rect); }}
      onPrefetch={() => onEventPrefetch?.(pos.event)}
      onPointerDown={(e) => {
        if (allowPointerEditing) onDragStart(pos.event.id, e, getEventEdgeFromClick(pos.event.id, e));
      }}
    />
  {/each}

  <!-- Drag previews replace the original event at the target position, layout-aware. -->
  {#each layoutedPreviews as previewPosition (previewPosition.event.id)}
    {@const previewEvent = previewPosition.event}
    {@const dragBase = getEventColor(previewEvent.color, theme)}
    {@const dragIconColor = `color-mix(in srgb, ${dragBase.text} 70%, ${dragBase.bg})`}
    {@const dragTimeColor = `color-mix(in srgb, ${dragBase.text} 80%, ${dragBase.bg})`}
    {@const dragLocationColor = `color-mix(in srgb, ${dragBase.text} 60%, ${dragBase.bg})`}
    {@const dragHeightPx = (previewPosition.durationMinutes / 60) * calendarZoom.hourHeight}
    {@const dragIndicators = getEventIndicatorState(previewEvent)}
    <div
      data-event-id={previewEvent.id}
      class="preview-outline pointer-events-none absolute flex overflow-hidden rounded text-[0.8rem] leading-tight"
      style="
        top: calc({previewPosition.startMinute} / 60 * var(--hour-h) * 1px);
        height: calc({previewPosition.durationMinutes} / 60 * var(--hour-h) * 1px);
        left: {previewPosition.left}%;
        width: {previewPosition.totalColumns > 1 ? `calc(${previewPosition.width}% - 2px)` : `${previewPosition.width}%`};
        color: {dragBase.text};
        --event-bg: {dragBase.bg};
        --outline-mix: {isDark ? 'white' : 'black'};
        z-index: 46;
      "
    >
      <div class="min-w-0 flex-1 px-1 py-0.5" style="background-color: {dragBase.bg};">
        {#if dragIndicators.iconCount > 0}
          <div class="absolute right-1 flex items-center gap-0.5" style="top: 5px; color: {dragIconColor};">
            {#if dragIndicators.hasRepeat}
              <Repeat size={9} class="shrink-0" />
            {/if}
            {#if dragIndicators.hasCallLink}
              <Video size={9} class="shrink-0" />
            {/if}
            {#if dragIndicators.hasLocation}
              <MapPin size={9} class="shrink-0" />
            {/if}
            {#if dragIndicators.hasGenericMeeting}
              <Users size={9} class="shrink-0" />
            {/if}
          </div>
        {/if}
        <div
          class="truncate font-medium"
          class:pr-5={dragIndicators.iconCount > 0 && dragIndicators.iconCount <= 2}
          class:pr-8={dragIndicators.iconCount > 2}
        >
          {#if previewEvent.title}{previewEvent.title}{:else}{t("calendar.event.noTitle")}{/if}
        </div>
        {#if dragHeightPx > 32}
          {@const startTime = previewEvent.start.split(" ")[1] ?? ""}
          {@const endTime = previewEvent.end.split(" ")[1] ?? ""}
          <div class="truncate" style="color: {dragTimeColor};">{formatTimeRange(startTime, endTime, preferences.calendarTimeFormat, "compact")}</div>
        {/if}
        {#if dragHeightPx > 48 && previewEvent.location}
          <div class="truncate text-[0.666667rem]" style="color: {dragLocationColor};">{previewEvent.location}</div>
        {/if}
      </div>
    </div>
  {/each}

  <!-- Create preview (new event being drawn, layout-aware) -->
  {#if createPreview && layoutedCreatePreview}
    {@const previewPosition = layoutedCreatePreview}
    {@const createBase = getEventColor(undefined, theme)}
    {@const createTimeColor = `color-mix(in srgb, ${createBase.text} 80%, ${createBase.bg})`}
    {@const createHeightPx = (previewPosition.durationMinutes / 60) * calendarZoom.hourHeight}
    <div
      data-create-preview
      class="preview-outline pointer-events-none absolute flex overflow-hidden rounded text-[0.8rem] leading-tight"
      style="
        top: calc({previewPosition.startMinute} / 60 * var(--hour-h) * 1px);
        height: calc({previewPosition.durationMinutes} / 60 * var(--hour-h) * 1px);
        left: {previewPosition.left}%;
        width: {previewPosition.totalColumns > 1 ? `calc(${previewPosition.width}% - 2px)` : `${previewPosition.width}%`};
        color: {createBase.text};
        --event-bg: {createBase.bg};
        --outline-mix: {isDark ? 'white' : 'black'};
        z-index: 10;
      "
    >
      <div class="min-w-0 flex-1 px-1 py-0.5" style="background-color: {createBase.bg};">
        <div class="truncate font-medium">
          {#if createPreview.event.title}{createPreview.event.title}{:else}{t("calendar.event.noTitle")}{/if}
        </div>
        {#if createHeightPx > 32}
          {@const startTime = createPreview.event.start.split(" ")[1] ?? ""}
          {@const endTime = createPreview.event.end.split(" ")[1] ?? ""}
          <div class="truncate" style="color: {createTimeColor};">{formatTimeRange(startTime, endTime, preferences.calendarTimeFormat, "compact")}</div>
        {/if}
      </div>
    </div>
  {/if}

  </div>

  </div>

<style>
  .preview-outline {
    position: absolute;
    outline: 2px solid color-mix(in oklab, var(--event-bg) 65%, var(--outline-mix));
    outline-offset: 0;
    transition: left 120ms ease-out, width 120ms ease-out;
  }

  .break-band-active {
    animation: break-pulse 1.5s ease-in-out infinite;
  }

  @keyframes break-pulse {
    0%, 100% { opacity: 0.5; }
    50% { opacity: 0.9; }
  }

</style>
