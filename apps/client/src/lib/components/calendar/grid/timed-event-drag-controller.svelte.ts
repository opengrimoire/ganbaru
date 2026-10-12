import type { CalendarEvent, DragState, EventColor, PositionedEvent } from "$lib/calendar/types";
import type { PanelAnchor } from "$lib/components/calendar/edit-session.svelte";
import { onDestroy, tick } from "svelte";
import {
  minuteOfDay,
  snapToGrid,
  clampMinute,
  formatDatePart,
  durationMinutes,
  minuteOffsetToDateStr,
  parseCalendarDate,
} from "$lib/calendar/utils";
import { getCalendarZoom } from "$lib/stores/calendar-zoom.svelte";
import { isPendingCreateEventId } from "$lib/components/calendar/display-events";
import { TouchHoldArbiter } from "$lib/utils/touch-hold";

let cursorStyle: HTMLStyleElement | null = null;

function lockCursor(cursor: string) {
  unlockCursor();
  cursorStyle = document.createElement("style");
  cursorStyle.textContent = `* { cursor: ${cursor} !important; }`;
  document.head.appendChild(cursorStyle);
}

function unlockCursor() {
  if (cursorStyle) {
    cursorStyle.remove();
    cursorStyle = null;
  }
  document.body.style.cursor = "";
}

const AUTO_SCROLL_ZONE = 48; // px from edge to start scrolling
const AUTO_SCROLL_MAX_SPEED = 12; // px per frame at the very edge
const DEFAULT_CLICK_EVENT_MINUTES = 60;
const CREATE_HOLD_PREVIEW_DELAY_MS = 160;
const CREATE_DRAG_THRESHOLD_PX = 3;
const EVENT_DRAG_THRESHOLD_PX = 3;

export interface CreateStartTiming {
  selectionMinute: number;
  clickMinute: number;
}

export interface TimedEventDragControllerConfig {
  events: () => CalendarEvent[];
  hourHeight: () => number;
  getColumnDate: (clientX: number) => string;
  getScrollContainer: () => HTMLElement | null;
  onEventUpdate: (event: CalendarEvent) => void | Promise<void>;
  onEventCreate: (start: string, end: string, anchor?: PanelAnchor) => void;
  canDrag?: (eventId: string) => boolean;
  /** Returns true if event has completed progress and should not be moved or resized. */
  isEventLocked?: (eventId: string) => boolean;
  /** Returns true when the rendered event is the currently active pomodoro occurrence. */
  isActiveEvent?: (event: CalendarEvent) => boolean;
  mobileLayout: () => boolean;
  onTouchEditStart?: () => void;
  onTouchEditEnd?: () => void;
}

export function createTimedEventDragController(config: TimedEventDragControllerConfig) {
  const calendarZoom = getCalendarZoom();
  let dragState = $state<DragState | null>(null);
  let dragPreviewDate = $state<string | null>(null);
  let dragPreview = $state<PositionedEvent | null>(null);
  let grabbingId = $state<string | null>(null); // Set immediately on pointerdown for visual feedback
  let _didDrag = $state(false); // Suppress click after drag

  let createState = $state<{
    dateStr: string;
    anchorMinute: number;
    clickStartMinute: number;
    columnEl: HTMLElement;
    pointerStartX: number;
    pointerStartY: number;
    mode: "pending" | "selecting";
  } | null>(null);
  let createPreviewDate = $state<string | null>(null);
  let createPreview = $state<PositionedEvent | null>(null);
  let createHoldTimer = 0;

  // Track latest pointer position for scroll-triggered updates and auto-scroll
  let lastPointerEvent: PointerEvent | null = null;
  let dragInteractionActive = false;
  let autoScrollRaf = 0;
  let scrollUpdateRaf = 0;
  let scrollUpdateContainer: HTMLElement | null = null;
  const touchHold = new TouchHoldArbiter();

  // Auto-scroll

  function getScrollEdgeSpeed(container: HTMLElement, clientY: number): number {
    const rect = container.getBoundingClientRect();
    // Find the bottom of any sticky header inside the container
    let topEdge = rect.top;
    for (const el of container.querySelectorAll(":scope > .sticky, :scope > div > .sticky")) {
      const h = el as HTMLElement;
      if (h.offsetHeight > 0) {
        topEdge = Math.max(topEdge, h.getBoundingClientRect().bottom);
      }
    }
    const distFromTop = clientY - topEdge;
    const distFromBottom = rect.bottom - clientY;

    if (distFromTop < AUTO_SCROLL_ZONE && distFromTop < distFromBottom) {
      // Scroll up: speed increases as pointer gets closer to edge
      return -AUTO_SCROLL_MAX_SPEED * (1 - distFromTop / AUTO_SCROLL_ZONE);
    }
    if (distFromBottom < AUTO_SCROLL_ZONE && distFromBottom < distFromTop) {
      // Scroll down
      return AUTO_SCROLL_MAX_SPEED * (1 - distFromBottom / AUTO_SCROLL_ZONE);
    }
    return 0;
  }

  function autoScrollLoop() {
    autoScrollRaf = 0;
    const container = config.getScrollContainer();
    if (!container || (!dragState && !createState) || !lastPointerEvent) return;

    const speed = getScrollEdgeSpeed(container, lastPointerEvent.clientY);
    if (speed !== 0) {
      container.scrollTop += speed;
      // Re-run the move handler so the preview tracks the new scroll position
      if (dragState && dragInteractionActive) updateDragPreview();
      if (createState) updateCreatePreview();
    }
    autoScrollRaf = requestAnimationFrame(autoScrollLoop);
  }

  function startAutoScroll() {
    if (!autoScrollRaf) autoScrollRaf = requestAnimationFrame(autoScrollLoop);
  }

  function stopAutoScroll() {
    if (autoScrollRaf) {
      cancelAnimationFrame(autoScrollRaf);
      autoScrollRaf = 0;
    }
  }

  function scheduleScrollDrivenUpdate() {
    if (scrollUpdateRaf) return;
    scrollUpdateRaf = requestAnimationFrame(() => {
      scrollUpdateRaf = 0;
      if (dragState && dragInteractionActive) updateDragPreview();
      if (createState) updateCreatePreview();
    });
  }

  function startScrollDrivenUpdates() {
    const container = config.getScrollContainer();
    if (!container || scrollUpdateContainer === container) return;
    stopScrollDrivenUpdates();
    scrollUpdateContainer = container;
    container.addEventListener("scroll", scheduleScrollDrivenUpdate, { passive: true });
  }

  function stopScrollDrivenUpdates() {
    if (scrollUpdateContainer) {
      scrollUpdateContainer.removeEventListener("scroll", scheduleScrollDrivenUpdate);
      scrollUpdateContainer = null;
    }
    if (scrollUpdateRaf) {
      cancelAnimationFrame(scrollUpdateRaf);
      scrollUpdateRaf = 0;
    }
  }

  function clearCreateHoldTimer() {
    if (createHoldTimer) {
      clearTimeout(createHoldTimer);
      createHoldTimer = 0;
    }
  }

  // Scroll-aware delta

  function scrollAwareDeltaY(clientY: number): number {
    if (!dragState) return 0;
    const container = config.getScrollContainer();
    const scrollDelta = container ? container.scrollTop - dragState.scrollTopAtStart : 0;
    return (clientY - dragState.pointerStartY) + scrollDelta;
  }

  // Existing event drag (move / resize)

  function canStartDrag(
    eventId: string,
    forceEdge?: "resize-top" | "resize-bottom",
  ): boolean {
    if (config.canDrag && !config.canDrag(eventId)) return false;
    const event = config.events().find((candidate) => candidate.id === eventId);
    if (!event || event.allDay) return false;
    if (config.isActiveEvent?.(event) && forceEdge !== "resize-bottom") return false;
    return isPendingCreateEventId(eventId) || !config.isEventLocked?.(eventId);
  }

  function handleDragStart(eventId: string, e: PointerEvent, forceEdge?: "resize-top" | "resize-bottom") {
    if (!canStartDrag(eventId, forceEdge)) return;
    if (config.mobileLayout() && e.pointerType === "touch") {
      touchHold.begin(e, () => {
        if (!canStartDrag(eventId, forceEdge) || !beginDragStart(eventId, e, forceEdge)) {
          touchHold.finish();
          return;
        }
        config.onTouchEditStart?.();
      });
      return;
    }
    beginDragStart(eventId, e, forceEdge);
  }

  function beginDragStart(
    eventId: string,
    e: PointerEvent,
    forceEdge?: "resize-top" | "resize-bottom",
  ): boolean {
    const event = config.events().find((ev) => ev.id === eventId);
    if (!event || event.allDay) return false;

    const dateStr = event.start.split(" ")[0];
    const startMin = minuteOfDay(event.start);
    const lengthMinutes = durationMinutes(event.start, event.end);
    const container = config.getScrollContainer();

    dragState = {
      eventId,
      type: "move",
      originDate: dateStr,
      startColumnDate: config.getColumnDate(e.clientX),
      originStartMinute: startMin,
      originEndMinute: startMin + lengthMinutes,
      pointerStartY: e.clientY,
      pointerStartX: e.clientX,
      scrollTopAtStart: container ? container.scrollTop : 0,
      columnWidth: 0,
      startColumnIndex: 0,
    };

    if (forceEdge) {
      dragState.type = forceEdge;
    } else if (!(config.mobileLayout() && e.pointerType === "touch")) {
      const eventEl = (e.target as HTMLElement).closest(".event-block-wrapper");
      if (eventEl) {
        const rect = eventEl.getBoundingClientRect();
        const relY = e.clientY - rect.top;
        const clippedTop = eventEl.hasAttribute("data-clipped-top");
        const clippedBottom = eventEl.hasAttribute("data-clipped-bottom");
        // Match the resize handle's visible zone, 6px inside the event after overflow clipping.
        // Top handle: visible from y=0 to y<6.
        // Bottom handle: visible from y>H-6 to y<H, where H is the event height.
        if (relY < 6 && !clippedTop) {
          dragState.type = "resize-top";
        } else if (relY >= rect.height - 6 && !clippedBottom) {
          dragState.type = "resize-bottom";
        }
      }
    }

    // Active events can only change their end time from the bottom edge.
    // Moving or top-resizing would rewrite a start time that already has
    // pomodoro history.
    if (config.isActiveEvent?.(event) && dragState.type !== "resize-bottom") {
      dragState = null;
      return false;
    }

    // Locked saved events (past with completed progress): no drag/resize at all.
    // Unsaved create previews stay editable until the user commits them.
    if (!isPendingCreateEventId(eventId) && config.isEventLocked?.(eventId)) {
      dragState = null;
      return false;
    }

    if (dragState.type === "resize-top" || dragState.type === "resize-bottom") {
      lockCursor("ns-resize");
    }

    dragInteractionActive = false;
    grabbingId = eventId; // Show contour immediately on pointer down
    lastPointerEvent = e;
    window.addEventListener("pointermove", handleDragMove);
    window.addEventListener("pointerup", handleDragEnd);
    window.addEventListener("pointercancel", handleDragCancel);
    startScrollDrivenUpdates();
    if (dragInteractionActive) startAutoScroll();
    return true;
  }

  function updateDragPreview() {
    if (!dragState || !lastPointerEvent) return;

    const event = config.events().find((ev) => ev.id === dragState!.eventId);
    if (!event) return;

    const pointerEvent = lastPointerEvent;
    const hourHeight = config.hourHeight();
    const deltaY = scrollAwareDeltaY(pointerEvent.clientY);
    const deltaMinutes = snapToGrid((deltaY / hourHeight) * 60, calendarZoom.gridMinutes);

    let newStart: number;
    let newEnd: number;
    let targetDate = dragState.originDate;

    if (dragState.type === "move") {
      // Compute the column delta to handle dragging from continuation segments
      const currentColumnDate = config.getColumnDate(pointerEvent.clientX);
      const startCol = parseCalendarDate(`${dragState.startColumnDate} 00:00`);
      const currentCol = parseCalendarDate(`${currentColumnDate} 00:00`);
      const dayDelta = Math.round(
        (currentCol.getTime() - startCol.getTime()) / 86400000,
      );
      const originDate = parseCalendarDate(`${dragState.originDate} 00:00`);
      originDate.setDate(originDate.getDate() + dayDelta);
      targetDate = formatDatePart(originDate);

      const lengthMinutes = dragState.originEndMinute - dragState.originStartMinute;
      let rawStart = snapToGrid(dragState.originStartMinute + deltaMinutes, calendarZoom.gridMinutes);

      // Shift target day when event fully crosses midnight vertically
      while (rawStart >= 1440) {
        rawStart -= 1440;
        const targetDay = parseCalendarDate(`${targetDate} 00:00`);
        targetDay.setDate(targetDay.getDate() + 1);
        targetDate = formatDatePart(targetDay);
      }
      while (rawStart + lengthMinutes <= 0) {
        rawStart += 1440;
        const targetDay = parseCalendarDate(`${targetDate} 00:00`);
        targetDay.setDate(targetDay.getDate() - 1);
        targetDate = formatDatePart(targetDay);
      }

      newStart = Math.min(1430, rawStart);
      newEnd = newStart + lengthMinutes; // may exceed 1440 or start < 0 (cross-midnight)
    } else if (dragState.type === "resize-top") {
      const minSize = calendarZoom.gridMinutes;
      const anchor = dragState.originEndMinute;
      let raw = snapToGrid(dragState.originStartMinute + deltaMinutes, minSize);
      raw = Math.max(0, raw);
      if (raw < anchor) {
        newStart = raw;
        newEnd = anchor;
        if (newEnd - newStart < minSize) newStart = newEnd - minSize;
      } else {
        // Flipped: top handle crossed below bottom
        newStart = anchor;
        newEnd = raw;
        if (newEnd - newStart < minSize) newEnd = newStart + minSize;
      }
    } else {
      // resize-bottom (supports crossover and crossing midnight)
      const minSize = calendarZoom.gridMinutes;
      const anchor = dragState.originStartMinute;
      let raw = snapToGrid(dragState.originEndMinute + deltaMinutes, minSize);
      if (raw > anchor) {
        newStart = anchor;
        newEnd = raw;
        if (newEnd - newStart < minSize) newEnd = newStart + minSize;
      } else {
        // Flipped: bottom handle crossed above top
        raw = Math.max(0, raw);
        newStart = raw;
        newEnd = anchor;
        if (newEnd - newStart < minSize) newStart = newEnd - minSize;
      }
    }

    // Active resize keeps the historical start fixed. Active move keeps the
    // normal move preview and the save flow decides whether the session stops.
    const activeResize = config.isActiveEvent?.(event) === true;
    if (activeResize && dragState.type !== "move") {
      newStart = dragState.originStartMinute;
      const now = new Date();
      const nowMinute = now.getHours() * 60 + now.getMinutes();
      const snap = calendarZoom.gridMinutes;
      const minEnd = snapToGrid(nowMinute, snap) + snap;
      if (newEnd < minEnd) newEnd = minEnd;
    }

    // During resize, if end reaches midnight, snap to at least 00:30 next day
    // so the continuation "tip" is clearly visible and easy to grab.
    // For move, exact midnight (1440) is valid since duration is preserved.
    if (dragState.type !== "move" && newEnd >= 1440 && newEnd < 1470) {
      newEnd = 1470;
    }

    const startStr = minuteOffsetToDateStr(targetDate, newStart);
    const endStr = minuteOffsetToDateStr(targetDate, newEnd);

    // Minute-based metrics for the primary (start) day column
    const visibleEnd = Math.min(newEnd, 1440);

    const nextPreview: PositionedEvent = {
      event: {
        ...event,
        start: startStr,
        end: endStr,
      },
      startMinute: newStart,
      durationMinutes: visibleEnd - newStart,
      left: 0,
      width: 100,
      column: 0,
      totalColumns: 1,
    };
    dragPreviewDate = targetDate;
    dragPreview = nextPreview;

  }

  function handleDragMove(e: PointerEvent) {
    if (!dragState) return;
    lastPointerEvent = e;
    if (!dragInteractionActive) {
      const dx = e.clientX - dragState.pointerStartX;
      const dy = e.clientY - dragState.pointerStartY;
      const movedEnough = Math.hypot(dx, dy) >= EVENT_DRAG_THRESHOLD_PX;
      if (!movedEnough) return;
      dragInteractionActive = true;
      if (dragState.type === "move") lockCursor("grabbing");
      startAutoScroll();
    }
    updateDragPreview();
  }

  async function handleDragEnd(e: PointerEvent) {
    window.removeEventListener("pointermove", handleDragMove);
    window.removeEventListener("pointerup", handleDragEnd);
    window.removeEventListener("pointercancel", handleDragCancel);
    stopAutoScroll();
    stopScrollDrivenUpdates();
    unlockCursor();

    if (dragState) {
      lastPointerEvent = e;
      if (!dragInteractionActive) {
        const dx = e.clientX - dragState.pointerStartX;
        const dy = e.clientY - dragState.pointerStartY;
        dragInteractionActive = Math.hypot(dx, dy) >= EVENT_DRAG_THRESHOLD_PX;
      }
      if (dragInteractionActive) updateDragPreview();
    }

    const state = dragState;
    const wasDragging = !!dragPreview;

    if (wasDragging || touchHold.editingActive) {
      _didDrag = true;
      setTimeout(() => { _didDrag = false; }, 0);
    }
    finishTouchEditing();

    if (dragPreview && state) {
      // Always notify the parent so it records lastDragEndTime and keeps the panel open.
      // The parent decides whether the position changed enough to persist.
      await config.onEventUpdate(dragPreview.event);
    }

    dragState = null;
    dragPreview = null;
    dragPreviewDate = null;
    grabbingId = null;
    lastPointerEvent = null;
    dragInteractionActive = false;
  }

  function handleDragCancel(): void {
    window.removeEventListener("pointermove", handleDragMove);
    window.removeEventListener("pointerup", handleDragEnd);
    window.removeEventListener("pointercancel", handleDragCancel);
    stopAutoScroll();
    stopScrollDrivenUpdates();
    unlockCursor();
    dragState = null;
    dragPreview = null;
    dragPreviewDate = null;
    grabbingId = null;
    lastPointerEvent = null;
    dragInteractionActive = false;
    finishTouchEditing();
  }

  // Create-by-drag

  function handleCreateStart(dateStr: string, timing: CreateStartTiming, e: PointerEvent) {
    if (config.mobileLayout() && e.pointerType === "touch") {
      touchHold.begin(e, () => {
        if (!beginCreateStart(dateStr, timing, e, true)) {
          touchHold.finish();
          return;
        }
        config.onTouchEditStart?.();
      }, {
        onTap: (releaseEvent) => {
          if (!beginCreateStart(dateStr, timing, e, false)) return;
          void handleCreateEnd(releaseEvent);
        },
      });
      return;
    }
    beginCreateStart(dateStr, timing, e, false);
  }

  function beginCreateStart(
    dateStr: string,
    timing: CreateStartTiming,
    e: PointerEvent,
    selectImmediately: boolean,
  ): boolean {
    if (dragState) return false; // don't start create while an event drag is active

    const columnEl = (e.target as HTMLElement).closest("[data-day-column-shell]") as HTMLElement | null;
    if (!columnEl) return false;

    const roundedSelectionMinute = Math.round(timing.selectionMinute);
    const roundedClickMinute = Math.round(timing.clickMinute);
    createState = {
      dateStr,
      anchorMinute: roundedSelectionMinute,
      clickStartMinute: roundedClickMinute,
      columnEl,
      pointerStartX: e.clientX,
      pointerStartY: e.clientY,
      mode: "pending",
    };

    lastPointerEvent = e;
    lockCursor("ns-resize");
    window.addEventListener("pointermove", handleCreateMove);
    window.addEventListener("pointerup", handleCreateEnd);
    window.addEventListener("pointercancel", handleCreateCancel);
    if (selectImmediately) enterCreateSelection();
    else {
      createHoldTimer = window.setTimeout(() => {
        enterCreateSelection();
      }, CREATE_HOLD_PREVIEW_DELAY_MS);
    }
    return true;
  }

  function enterCreateSelection() {
    const state = createState;
    if (!state || state.mode === "selecting") return;

    clearCreateHoldTimer();
    createState = { ...state, mode: "selecting" };
    const snap = calendarZoom.gridMinutes;
    createPreviewDate = state.dateStr;
    createPreview = buildPreview(
      state.dateStr,
      state.anchorMinute,
      Math.min(state.anchorMinute + snap, 1440),
    );
    startScrollDrivenUpdates();
    startAutoScroll();
  }

  function updateCreatePreview() {
    if (!createState || createState.mode !== "selecting" || !lastPointerEvent) return;

    const hourHeight = config.hourHeight();
    const rect = createState.columnEl.getBoundingClientRect();
    const offsetY = lastPointerEvent.clientY - rect.top;
    const cursorMinute = clampMinute(snapToGrid((offsetY / hourHeight) * 60, calendarZoom.gridMinutes));

    const anchor = createState.anchorMinute;
    let startMinute = Math.min(anchor, cursorMinute);
    let endMinute = Math.max(anchor, cursorMinute);
    const snap = calendarZoom.gridMinutes;
    if (endMinute - startMinute < snap) endMinute = startMinute + snap;

    createPreview = buildPreview(
      createState.dateStr,
      startMinute,
      clampMinute(endMinute),
    );
  }

  function getCreatePreviewAnchor(preview: PositionedEvent, columnEl: HTMLElement): PanelAnchor {
    const rect = columnEl.getBoundingClientRect();
    const hourHeight = config.hourHeight();
    return {
      x: rect.right,
      y: rect.top + (preview.startMinute / 60) * hourHeight,
      width: rect.width,
      height: (preview.durationMinutes / 60) * hourHeight,
    };
  }

  function handleCreateMove(e: PointerEvent) {
    const state = createState;
    if (!state) return;
    lastPointerEvent = e;

    if (state.mode === "pending") {
      const dx = e.clientX - state.pointerStartX;
      const dy = e.clientY - state.pointerStartY;
      const movedEnough = Math.hypot(dx, dy) >= CREATE_DRAG_THRESHOLD_PX;
      if (!movedEnough) return;
      enterCreateSelection();
    }

    updateCreatePreview();
  }

  async function handleCreateEnd(e: PointerEvent) {
    window.removeEventListener("pointermove", handleCreateMove);
    window.removeEventListener("pointerup", handleCreateEnd);
    window.removeEventListener("pointercancel", handleCreateCancel);
    clearCreateHoldTimer();
    stopAutoScroll();
    stopScrollDrivenUpdates();
    unlockCursor();

    if (createState) {
      lastPointerEvent = e;
      if (createState.mode === "pending") {
        const dx = e.clientX - createState.pointerStartX;
        const dy = e.clientY - createState.pointerStartY;
        if (Math.hypot(dx, dy) >= CREATE_DRAG_THRESHOLD_PX) {
          enterCreateSelection();
        }
      }
      updateCreatePreview();
    }

    const state = createState;
    finishTouchEditing();

    if (state?.mode === "pending") {
      const endMinute = clampMinute(
        Math.min(state.clickStartMinute + DEFAULT_CLICK_EVENT_MINUTES, 1440),
      );
      const preview = buildPreview(state.dateStr, state.clickStartMinute, endMinute);
      createPreviewDate = state.dateStr;
      createPreview = preview;
      await tick();

      const start = preview.event.start;
      const end = preview.event.end;
      const anchor = getCreatePreviewAnchor(preview, state.columnEl);
      createState = null;
      createPreview = null;
      createPreviewDate = null;
      config.onEventCreate(start, end, anchor);
      lastPointerEvent = null;
      return;
    }

    if (createPreview && state) {
      // Save values and clear preview state before calling onEventCreate.
      // This prevents both __create__ and PENDING_CREATE_ID from existing simultaneously,
      // which would cause the layout to see them as overlapping and animate width changes.
      const start = createPreview.event.start;
      const end = createPreview.event.end;
      const anchor = getCreatePreviewAnchor(createPreview, state.columnEl);
      createState = null;
      createPreview = null;
      createPreviewDate = null;
      config.onEventCreate(start, end, anchor);
      lastPointerEvent = null;
    } else {
      createState = null;
      createPreview = null;
      createPreviewDate = null;
      lastPointerEvent = null;
    }
  }

  function handleCreateCancel(): void {
    window.removeEventListener("pointermove", handleCreateMove);
    window.removeEventListener("pointerup", handleCreateEnd);
    window.removeEventListener("pointercancel", handleCreateCancel);
    clearCreateHoldTimer();
    stopAutoScroll();
    stopScrollDrivenUpdates();
    unlockCursor();
    createState = null;
    createPreview = null;
    createPreviewDate = null;
    lastPointerEvent = null;
    finishTouchEditing();
  }

  function finishTouchEditing(): void {
    const wasActive = touchHold.editingActive;
    touchHold.finish();
    if (wasActive) config.onTouchEditEnd?.();
  }

  onDestroy(() => {
    handleDragCancel();
    handleCreateCancel();
    touchHold.finish();
  });

  function buildPreview(
    dateStr: string,
    startMinute: number,
    endMinute: number,
    title?: string,
    color?: EventColor,
  ): PositionedEvent {
    const start = Math.round(startMinute);
    const end = Math.round(Math.min(endMinute, 1440));

    return {
      event: {
        id: "__create__",
        title: title ?? "",
        start: minuteOffsetToDateStr(dateStr, start),
        end: minuteOffsetToDateStr(dateStr, end),
        color,
        timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
        calendarId: "default",
      },
      startMinute: start,
      durationMinutes: end - start,
      left: 0,
      width: 100,
      column: 0,
      totalColumns: 1,
    };
  }

  // Computed helpers for DayColumn props

  function positionDragPreviewForDate(
    preview: PositionedEvent,
    dateStr: string,
  ): PositionedEvent | null {
    const previewStartDate = preview.event.start.split(" ")[0];
    const previewEndDate = preview.event.end.split(" ")[0];

    // Single-day event: return preview as-is for its date
    if (previewStartDate === previewEndDate) {
      return dateStr === previewStartDate ? preview : null;
    }

    // Start day: show from event start to bottom of day
    if (dateStr === previewStartDate) {
      const startMin = minuteOfDay(preview.event.start);
      return {
        ...preview,
        startMinute: startMin,
        durationMinutes: 1440 - startMin,
      };
    }

    // End day: show from top to event end
    if (dateStr === previewEndDate) {
      const endMin = minuteOfDay(preview.event.end);
      if (endMin <= 0) return null;
      return {
        ...preview,
        startMinute: 0,
        durationMinutes: endMin,
      };
    }

    return null;
  }

  function getDragPreviewForDate(dateStr: string): PositionedEvent | null {
    if (!dragPreview) return null;
    return positionDragPreviewForDate(dragPreview, dateStr);
  }

  function getCreatePreviewForDate(dateStr: string): PositionedEvent | null {
    if (createPreviewDate === dateStr) return createPreview;
    return null;
  }

  return {
    get dragState() { return dragState; },
    get dragPreview() { return dragPreview; },
    get dragPreviewDate() { return dragPreviewDate; },
    get createPreview() { return createPreview; },
    get createPreviewDate() { return createPreviewDate; },
    get grabbingId() { return grabbingId; },
    get didDrag() { return _didDrag; },
    handleDragStart,
    handleCreateStart,
    getDragPreviewForDate,
    getCreatePreviewForDate,
  };
}
