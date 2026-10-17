import type {
  CalendarEvent,
  AllDayDragState,
  PositionedAllDayEvent,
} from "$lib/calendar/types";
import { formatDatePart } from "$lib/calendar/utils";
import { onDestroy } from "svelte";
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
}

const CLICK_THRESHOLD = 5;
const EDGE_ZONE = 8;
export interface AllDayEventDragControllerConfig {
  events: () => CalendarEvent[];
  days: () => Date[];
  getColumnBounds: () => DOMRect[];
  getPositionedEvents: () => PositionedAllDayEvent[];
  onEventUpdate: (event: CalendarEvent) => void | Promise<void>;
  canDrag?: (eventId: string) => boolean;
  isEventLocked?: (eventId: string) => boolean;
  mobileLayout: () => boolean;
  onTouchEditStart?: () => void;
  onTouchEditEnd?: () => void;
}

export function createAllDayEventDragController(config: AllDayEventDragControllerConfig) {
  let dragState = $state<AllDayDragState | null>(null);
  let allDayDragPreview = $state<PositionedAllDayEvent | null>(null);
  let draggingEventId = $state<string | null>(null);
  let grabbingId = $state<string | null>(null); // Set immediately on pointerdown for visual feedback
  let _didDrag = $state(false);
  const touchHold = new TouchHoldArbiter();

  function columnFromX(clientX: number, bounds: DOMRect[]): number {
    for (let i = 0; i < bounds.length; i++) {
      if (clientX >= bounds[i].left && clientX < bounds[i].left + bounds[i].width) return i;
    }
    // Clamp to nearest edge
    if (bounds.length > 0 && clientX < bounds[0].left) return 0;
    return bounds.length - 1;
  }

  function dateStrForCol(col: number): string {
    const days = config.days();
    return formatDatePart(days[Math.max(0, Math.min(col, days.length - 1))]);
  }

  function shiftDateStr(dateStr: string, daysDelta: number): string {
    const [year, month, day] = dateStr.split("-").map(Number);
    const date = new Date(year, month - 1, day);
    date.setDate(date.getDate() + daysDelta);
    return formatDatePart(date);
  }

  function canStartDrag(eventId: string): boolean {
    if (config.canDrag && !config.canDrag(eventId)) return false;
    if (config.isEventLocked?.(eventId)) return false;
    return config.events().some((event) => event.id === eventId);
  }

  function handleDragStart(eventId: string, e: PointerEvent) {
    if (!canStartDrag(eventId)) return;
    if (config.mobileLayout() && e.pointerType === "touch") {
      touchHold.begin(e, () => {
        if (!canStartDrag(eventId) || !beginDragStart(eventId, e)) {
          touchHold.finish();
          return;
        }
        config.onTouchEditStart?.();
      });
      return;
    }
    beginDragStart(eventId, e);
  }

  function beginDragStart(eventId: string, e: PointerEvent): boolean {
    const event = config.events().find((ev) => ev.id === eventId);
    if (!event) return false;

    const bounds = config.getColumnBounds();
    if (bounds.length === 0) return false;

    const days = config.days();
    const dayStrs = days.map((d) => formatDatePart(d));
    const startDate = event.start.split(" ")[0];
    const endDate = event.end.split(" ")[0];

    const rangeStart = dayStrs[0];
    const rangeEnd = dayStrs[dayStrs.length - 1];
    const clippedStart = startDate < rangeStart ? rangeStart : startDate;
    const clippedEnd = endDate > rangeEnd ? rangeEnd : endDate;

    const startCol = dayStrs.indexOf(clippedStart);
    const endCol = dayStrs.indexOf(clippedEnd);
    if (startCol < 0 || endCol < 0) return false;
    const spanCols = endCol - startCol + 1;

    const positioned = config.getPositionedEvents();
    const currentPos = positioned.find((p) => p.event.id === eventId);
    const originRow = currentPos?.row ?? 0;

    // Detect type from pointer position relative to chip
    const chipEl = (e.target as HTMLElement).closest("[data-event-id]") as HTMLElement | null;
    let type: AllDayDragState["type"] = "move";
    if (chipEl && !(config.mobileLayout() && e.pointerType === "touch")) {
      const rect = chipEl.getBoundingClientRect();
      if (e.clientX - rect.left <= EDGE_ZONE && startDate >= rangeStart) {
        type = "resize-start";
      } else if (rect.right - e.clientX <= EDGE_ZONE && endDate <= rangeEnd) {
        type = "resize-end";
      }
    }

    dragState = {
      eventId,
      type,
      originStartCol: startCol,
      originSpanCols: spanCols,
      originRow,
      pointerStartX: e.clientX,
      pointerStartY: e.clientY,
      columnBounds: bounds,
    };

    draggingEventId = null; // Not a drag until the pointer passes the click threshold
    grabbingId = eventId; // Show contour immediately on grab

    window.addEventListener("pointermove", handleDragMove);
    window.addEventListener("pointerup", handleDragEnd);
    window.addEventListener("pointercancel", handleDragCancel);
    return true;
  }

  function handleDragMove(e: PointerEvent) {
    if (!dragState) return;

    const dx = e.clientX - dragState.pointerStartX;
    const dy = e.clientY - dragState.pointerStartY;

    // Click threshold: don't start visual drag until pointer has moved enough
    if (!draggingEventId) {
      if (Math.abs(dx) < CLICK_THRESHOLD && Math.abs(dy) < CLICK_THRESHOLD) return;
      draggingEventId = dragState.eventId;
      lockCursor(dragState.type === "move" ? "grabbing" : "ew-resize");
    }

    const event = config.events().find((ev) => ev.id === dragState!.eventId);
    if (!event) return;

    const currentCol = columnFromX(e.clientX, dragState.columnBounds);
    const maxCol = dragState.columnBounds.length;

    let newStartCol: number;
    let newSpanCols: number;

    if (dragState.type === "move") {
      const colDelta = currentCol - columnFromX(dragState.pointerStartX, dragState.columnBounds);
      newStartCol = dragState.originStartCol + colDelta;
      newSpanCols = dragState.originSpanCols;
      // Clamp within visible range bounds.
      if (newStartCol < 0) newStartCol = 0;
      if (newStartCol + newSpanCols > maxCol) newStartCol = maxCol - newSpanCols;
    } else if (dragState.type === "resize-start") {
      const endCol = dragState.originStartCol + dragState.originSpanCols - 1;
      newStartCol = Math.min(currentCol, endCol);
      newStartCol = Math.max(0, newStartCol);
      newSpanCols = endCol - newStartCol + 1;
    } else {
      // resize-end
      newStartCol = dragState.originStartCol;
      const newEndCol = Math.max(currentCol, newStartCol);
      newSpanCols = Math.min(newEndCol - newStartCol + 1, maxCol - newStartCol);
    }

    allDayDragPreview = {
      event,
      row: dragState.originRow,
      startCol: newStartCol,
      spanCols: newSpanCols,
    };
  }

  async function handleDragEnd() {
    window.removeEventListener("pointermove", handleDragMove);
    window.removeEventListener("pointerup", handleDragEnd);
    window.removeEventListener("pointercancel", handleDragCancel);
    unlockCursor();

    const state = dragState;
    const preview = allDayDragPreview;
    const wasDragging = !!draggingEventId;
    const consumedTouch = touchHold.editingActive;
    if (wasDragging || consumedTouch) {
      _didDrag = true;
      setTimeout(() => { _didDrag = false; }, 0);
    }
    finishTouchEditing();

    if (!state || !preview) {
      dragState = null;
      allDayDragPreview = null;
      draggingEventId = null;
      grabbingId = null;
      return;
    }

    const event = config.events().find((ev) => ev.id === state.eventId);
    if (!event) {
      dragState = null;
      allDayDragPreview = null;
      draggingEventId = null;
      grabbingId = null;
      return;
    }

    // Always notify the parent so it records lastDragEndTime and keeps the panel
    // open. The parent decides whether the position changed enough to persist.
    const visibleStartDate = dateStrForCol(preview.startCol);
    const visibleEndDate = dateStrForCol(preview.startCol + preview.spanCols - 1);
    const eventStartDate = event.start.split(" ")[0];
    const eventEndDate = event.end.split(" ")[0];
    const colDelta = preview.startCol - state.originStartCol;
    const newStartDate = state.type === "move"
      ? shiftDateStr(eventStartDate, colDelta)
      : state.type === "resize-start"
        ? visibleStartDate
        : eventStartDate;
    const newEndDate = state.type === "move"
      ? shiftDateStr(eventEndDate, colDelta)
      : state.type === "resize-end"
        ? visibleEndDate
        : eventEndDate;
    await config.onEventUpdate({
      ...event,
      start: `${newStartDate} 00:00`,
      end: `${newEndDate} 00:00`,
    });

    dragState = null;
    allDayDragPreview = null;
    draggingEventId = null;
    grabbingId = null;
  }

  function handleDragCancel(): void {
    window.removeEventListener("pointermove", handleDragMove);
    window.removeEventListener("pointerup", handleDragEnd);
    window.removeEventListener("pointercancel", handleDragCancel);
    unlockCursor();
    dragState = null;
    allDayDragPreview = null;
    draggingEventId = null;
    grabbingId = null;
    finishTouchEditing();
  }

  function finishTouchEditing(): void {
    const wasActive = touchHold.editingActive;
    touchHold.finish();
    if (wasActive) config.onTouchEditEnd?.();
  }

  onDestroy(() => {
    handleDragCancel();
    touchHold.finish();
  });

  return {
    get dragState() { return dragState; },
    get allDayDragPreview() { return allDayDragPreview; },
    get draggingEventId() { return draggingEventId; },
    get grabbingId() { return grabbingId; },
    get didDrag() { return _didDrag; },
    handleDragStart,
  };
}
