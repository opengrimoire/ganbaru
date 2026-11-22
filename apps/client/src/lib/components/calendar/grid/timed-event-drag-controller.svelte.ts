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
import { useCalendarZoom } from "$lib/stores/calendar-zoom.svelte";
import { isPendingCreateEventId, PENDING_CREATE_ID } from "$lib/components/calendar/display-events";
import { TouchHoldArbiter } from "$lib/utils/touch-hold";
import {
  IDLE_GRID_GESTURE,
  gestureDrag,
  gestureDragPreview,
  transitionGridGesture,
  type CreateGestureOrigin,
  type GridGesture,
  type GridGestureEvent,
} from "./grid-gesture";

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

export interface TimedEventDragStartOptions {
  /** Start a touch drag at once instead of after the long press, for dedicated touch handles. */
  immediate?: boolean;
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
  const calendarZoom = useCalendarZoom();
  let gesture = $state.raw<GridGesture>(IDLE_GRID_GESTURE);
  const dragPreview = $derived(gestureDragPreview(gesture));
  // The grabbed event shows its contour from pointer down until its update settles.
  const grabbingId = $derived(gestureDrag(gesture)?.eventId ?? null);
  let _didDrag = $state(false); // Suppress click after drag
  let createHoldTimer = 0;

  // Track latest pointer position for scroll-triggered updates and auto-scroll
  let lastPointerEvent: PointerEvent | null = null;
  let autoScrollRaf = 0;
  let scrollUpdateRaf = 0;
  let scrollUpdateContainer: HTMLElement | null = null;
  const touchHold = new TouchHoldArbiter();
  // Whether the parent was told a touch edit started, so a still tap never collapses the sheet.
  let touchEditNotified = false;

  function dispatch(event: GridGestureEvent): void {
    gesture = transitionGridGesture(gesture, event);
  }

  /** Recompute whichever preview the running gesture draws. */
  function updateGesturePreview(): void {
    if (gesture.phase === "eventDragging") updateDragPreview();
    else if (gesture.phase === "createSelecting") updateCreatePreview();
  }

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
    if (!container || gesture.phase === "idle" || !lastPointerEvent) return;

    const speed = getScrollEdgeSpeed(container, lastPointerEvent.clientY);
    if (speed !== 0) {
      container.scrollTop += speed;
      // Re-run the move handler so the preview tracks the new scroll position
      updateGesturePreview();
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
      updateGesturePreview();
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

  function scrollAwareDeltaY(dragState: DragState, clientY: number): number {
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

  function handleDragStart(
    eventId: string,
    e: PointerEvent,
    forceEdge?: "resize-top" | "resize-bottom",
    options: TimedEventDragStartOptions = {},
  ) {
    if (!canStartDrag(eventId, forceEdge)) return;
    if (config.mobileLayout() && e.pointerType === "touch") {
      touchHold.begin(e, () => {
        if (!canStartDrag(eventId, forceEdge) || !beginDragStart(eventId, e, forceEdge)) {
          touchHold.finish();
          return;
        }
        // An immediate touch reports the edit once it moves, so a still tap leaves the sheet as is.
        if (!options.immediate) notifyTouchEditStart();
      }, { immediate: options.immediate });
      return;
    }
    beginDragStart(eventId, e, forceEdge);
  }

  function beginDragStart(
    eventId: string,
    e: PointerEvent,
    forceEdge?: "resize-top" | "resize-bottom",
  ): boolean {
    if (gesture.phase !== "idle") return false;
    const event = config.events().find((ev) => ev.id === eventId);
    if (!event || event.allDay) return false;

    const dateStr = event.start.split(" ")[0];
    const startMin = minuteOfDay(event.start);
    const lengthMinutes = durationMinutes(event.start, event.end);
    const container = config.getScrollContainer();

    const dragState: DragState = {
      eventId,
      type: "move",
      originDate: dateStr,
      startColumnDate: config.getColumnDate(e.clientX),
      originStartMinute: startMin,
      originEndMinute: startMin + lengthMinutes,
      pointerStartY: e.clientY,
      pointerStartX: e.clientX,
      scrollTopAtStart: container ? container.scrollTop : 0,
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
    if (config.isActiveEvent?.(event) && dragState.type !== "resize-bottom") return false;

    // Locked saved events (past with completed progress): no drag/resize at all.
    // Unsaved create previews stay editable until the user commits them.
    if (!isPendingCreateEventId(eventId) && config.isEventLocked?.(eventId)) return false;

    if (dragState.type === "resize-top" || dragState.type === "resize-bottom") {
      lockCursor("ns-resize");
    }

    dispatch({ type: "eventPressed", drag: dragState });
    lastPointerEvent = e;
    window.addEventListener("pointermove", handleDragMove);
    window.addEventListener("pointerup", handleDragEnd);
    window.addEventListener("pointercancel", handleDragCancel);
    startScrollDrivenUpdates();
    return true;
  }

  function updateDragPreview() {
    if (gesture.phase !== "eventDragging" || !lastPointerEvent) return;
    const dragState = gesture.drag;

    const event = config.events().find((ev) => ev.id === dragState.eventId);
    if (!event) return;

    const pointerEvent = lastPointerEvent;
    const hourHeight = config.hourHeight();
    const deltaY = scrollAwareDeltaY(dragState, pointerEvent.clientY);
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
    dispatch({ type: "dragPreviewed", preview: nextPreview });
  }

  /** Whether the pointer moved far enough from the press to start dragging. */
  function passedDragThreshold(dragState: DragState, e: PointerEvent): boolean {
    const dx = e.clientX - dragState.pointerStartX;
    const dy = e.clientY - dragState.pointerStartY;
    return Math.hypot(dx, dy) >= EVENT_DRAG_THRESHOLD_PX;
  }

  function handleDragMove(e: PointerEvent) {
    if (gesture.phase !== "eventPressed" && gesture.phase !== "eventDragging") return;
    lastPointerEvent = e;
    if (gesture.phase === "eventPressed") {
      if (!passedDragThreshold(gesture.drag, e)) return;
      dispatch({ type: "dragThresholdPassed" });
      if (touchHold.editingActive) notifyTouchEditStart();
      if (gestureDrag(gesture)?.type === "move") lockCursor("grabbing");
      startAutoScroll();
    }
    updateDragPreview();
  }

  function removeDragListeners(): void {
    window.removeEventListener("pointermove", handleDragMove);
    window.removeEventListener("pointerup", handleDragEnd);
    window.removeEventListener("pointercancel", handleDragCancel);
    stopAutoScroll();
    stopScrollDrivenUpdates();
    unlockCursor();
  }

  async function handleDragEnd(e: PointerEvent) {
    removeDragListeners();

    if (gesture.phase === "eventPressed" || gesture.phase === "eventDragging") {
      lastPointerEvent = e;
      if (gesture.phase === "eventPressed" && passedDragThreshold(gesture.drag, e)) {
        dispatch({ type: "dragThresholdPassed" });
      }
      updateDragPreview();
    }

    if (gestureDragPreview(gesture) || touchHold.editingActive) {
      _didDrag = true;
      setTimeout(() => { _didDrag = false; }, 0);
    }
    finishTouchEditing();
    lastPointerEvent = null;

    dispatch({ type: "dragReleased" });
    const committing = gesture;
    if (committing.phase !== "eventCommitting") return;
    try {
      // Always notify the parent so it records lastDragEndTime and keeps the panel open.
      // The parent decides whether the position changed enough to persist.
      await config.onEventUpdate(committing.preview.event);
    } finally {
      // A cancel while the update ran already ended this gesture, and any later one is not this one to settle.
      if (gesture === committing) dispatch({ type: "dragSettled" });
    }
  }

  function handleDragCancel(): void {
    removeDragListeners();
    lastPointerEvent = null;
    if (gestureDrag(gesture)) dispatch({ type: "ended" });
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
        notifyTouchEditStart();
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
    if (gesture.phase !== "idle") return false; // don't start create while another gesture runs

    const columnEl = (e.target as HTMLElement).closest("[data-day-column-shell]") as HTMLElement | null;
    if (!columnEl) return false;

    dispatch({
      type: "createPressed",
      create: {
        dateStr,
        anchorMinute: Math.round(timing.selectionMinute),
        clickStartMinute: Math.round(timing.clickMinute),
        columnEl,
        pointerStartX: e.clientX,
        pointerStartY: e.clientY,
      },
    });

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
    if (gesture.phase !== "createPending") return;
    const { create } = gesture;

    clearCreateHoldTimer();
    const snap = calendarZoom.gridMinutes;
    dispatch({
      type: "createPreviewed",
      preview: buildPreview(create.dateStr, create.anchorMinute, Math.min(create.anchorMinute + snap, 1440)),
    });
    startScrollDrivenUpdates();
    startAutoScroll();
  }

  function updateCreatePreview() {
    if (gesture.phase !== "createSelecting" || !lastPointerEvent) return;
    const { create } = gesture;

    const hourHeight = config.hourHeight();
    const rect = create.columnEl.getBoundingClientRect();
    const offsetY = lastPointerEvent.clientY - rect.top;
    const cursorMinute = clampMinute(snapToGrid((offsetY / hourHeight) * 60, calendarZoom.gridMinutes));

    const anchor = create.anchorMinute;
    let startMinute = Math.min(anchor, cursorMinute);
    let endMinute = Math.max(anchor, cursorMinute);
    const snap = calendarZoom.gridMinutes;
    if (endMinute - startMinute < snap) endMinute = startMinute + snap;

    dispatch({
      type: "createPreviewed",
      preview: buildPreview(create.dateStr, startMinute, clampMinute(endMinute)),
    });
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

  /** Whether the pointer moved far enough from the press to select a range. */
  function passedCreateThreshold(create: CreateGestureOrigin, e: PointerEvent): boolean {
    const dx = e.clientX - create.pointerStartX;
    const dy = e.clientY - create.pointerStartY;
    return Math.hypot(dx, dy) >= CREATE_DRAG_THRESHOLD_PX;
  }

  function handleCreateMove(e: PointerEvent) {
    if (gesture.phase !== "createPending" && gesture.phase !== "createSelecting") return;
    lastPointerEvent = e;

    if (gesture.phase === "createPending") {
      if (!passedCreateThreshold(gesture.create, e)) return;
      enterCreateSelection();
    }

    updateCreatePreview();
  }

  function removeCreateListeners(): void {
    window.removeEventListener("pointermove", handleCreateMove);
    window.removeEventListener("pointerup", handleCreateEnd);
    window.removeEventListener("pointercancel", handleCreateCancel);
    clearCreateHoldTimer();
    stopAutoScroll();
    stopScrollDrivenUpdates();
    unlockCursor();
  }

  async function handleCreateEnd(e: PointerEvent) {
    removeCreateListeners();

    if (gesture.phase === "createPending" || gesture.phase === "createSelecting") {
      lastPointerEvent = e;
      if (gesture.phase === "createPending" && passedCreateThreshold(gesture.create, e)) {
        enterCreateSelection();
      }
      updateCreatePreview();
    }

    const state = gesture;
    finishTouchEditing();
    lastPointerEvent = null;

    if (state.phase === "createPending") {
      const { create } = state;
      const movedDraftMinutes = tapMovedDraftMinutes();
      if (movedDraftMinutes !== undefined) {
        dispatch({ type: "ended" });
        config.onEventCreate(
          minuteOffsetToDateStr(create.dateStr, create.clickStartMinute),
          minuteOffsetToDateStr(create.dateStr, create.clickStartMinute + movedDraftMinutes),
        );
        return;
      }

      // A click draws a default-length range for one frame, then opens the editor anchored to it.
      const endMinute = clampMinute(
        Math.min(create.clickStartMinute + DEFAULT_CLICK_EVENT_MINUTES, 1440),
      );
      dispatch({ type: "createPreviewed", preview: buildPreview(create.dateStr, create.clickStartMinute, endMinute) });
      const clicked = gesture;
      await tick();
      // A cancel during the frame already ended this gesture.
      if (gesture !== clicked || clicked.phase !== "createSelecting") return;
      finishCreateSelection(clicked.create, clicked.preview);
      return;
    }

    if (state.phase === "createSelecting") finishCreateSelection(state.create, state.preview);
  }

  /**
   * End the create gesture before reporting its range, so the `__create__` preview and the pending draft never
   * coexist; the layout would see them as overlapping and animate width changes.
   */
  function finishCreateSelection(create: CreateGestureOrigin, preview: PositionedEvent): void {
    const anchor = getCreatePreviewAnchor(preview, create.columnEl);
    dispatch({ type: "ended" });
    config.onEventCreate(preview.event.start, preview.event.end, anchor);
  }

  function handleCreateCancel(): void {
    removeCreateListeners();
    lastPointerEvent = null;
    if (gesture.phase === "createPending" || gesture.phase === "createSelecting") dispatch({ type: "ended" });
    finishTouchEditing();
  }


  /**
   * Return the length in minutes of the open timed draft that a tap on a touch layout moves to the tapped slot, keeping
   * its length, or undefined when a tap creates a new event instead.
   */
  function tapMovedDraftMinutes(): number | undefined {
    if (!config.mobileLayout()) return undefined;
    const draft = config.events().find((event) => event.id === PENDING_CREATE_ID);
    if (!draft || draft.allDay) return undefined;
    return durationMinutes(draft.start, draft.end);
  }

  function notifyTouchEditStart(): void {
    if (touchEditNotified) return;
    touchEditNotified = true;
    config.onTouchEditStart?.();
  }

  function finishTouchEditing(): void {
    const wasNotified = touchEditNotified;
    touchEditNotified = false;
    touchHold.finish();
    if (wasNotified) config.onTouchEditEnd?.();
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
    return gesture.phase === "createSelecting" && gesture.create.dateStr === dateStr ? gesture.preview : null;
  }

  return {
    /** The event whose drag preview is on screen. */
    get draggingEventId() { return dragPreview ? gestureDrag(gesture)?.eventId : undefined; },
    get grabbingId() { return grabbingId; },
    get didDrag() { return _didDrag; },
    handleDragStart,
    handleCreateStart,
    getDragPreviewForDate,
    getCreatePreviewForDate,
  };
}
