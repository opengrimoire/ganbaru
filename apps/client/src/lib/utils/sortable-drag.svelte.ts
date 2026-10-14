import type { Attachment } from "svelte/attachments";
import {
  SORTABLE_DRAG_SETTLE_MS,
  SORTABLE_DRAG_THRESHOLD,
  sortableDragBounds,
  sortableDragLanding,
  sortableDragShift,
  sortableDragTarget,
  type SortableSlot,
} from "./sortable-drag";

/** Class on the document root while an item is dragged, so the cursor and text selection hold across the page. */
const DRAGGING_ROOT_CLASS = "sortable-dragging";

/** `lifted` follows the pointer and `settling` glides into the slot the item lands in. */
export type SortableDragPhase = "lifted" | "settling";

export interface SortableDragOptions {
  /** Ids of the items `id` may move among, in rendered order. */
  getIds: (id: string) => readonly string[];
  /** Whether a drag may start now, for example while no earlier move is still saving. */
  canStart?: () => boolean;
  /** Moves `id` to `targetIndex` among its siblings once it has settled into its new slot. */
  commit: (id: string, targetIndex: number) => void | Promise<void>;
}

interface PendingPress {
  pointerId: number;
  x: number;
  y: number;
  id: string;
  handle: HTMLElement;
}

interface ActiveDrag {
  pointerId: number;
  id: string;
  handle: HTMLElement;
  ids: string[];
  source: number;
  /** Item positions relative to `list` when the drag started, before any item slides. */
  slots: SortableSlot[];
  /** The element holding the items, which anchors positions while the page scrolls. */
  list: HTMLElement;
  /** Pointer position relative to `list` when the drag started. */
  grabY: number;
  bounds: { min: number; max: number };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * Pointer-driven reordering for one vertical list whose items move in place. Pressing an item's drag handle and moving
 * past the threshold lifts the item, which then follows the pointer while the items between its slot and the hovered
 * slot slide aside; release settles it into the gap and then commits the move, so the list reorders without a jump.
 * Escape, a canceled pointer, or leaving the window returns the item to its slot. Items may differ in height.
 *
 * Attach `item(id)` to each item element and call `pointerDown` from its handle. The handle should carry
 * `touch-action: none` so touch drags instead of scrolling.
 */
export function createSortableDrag(options: SortableDragOptions) {
  const elements = new Map<string, HTMLElement>();
  let draggedId = $state<string | null>(null);
  let phase = $state<SortableDragPhase | null>(null);
  let offset = $state(0);
  let target = $state(0);
  let shiftAnimated = $state(true);
  let pending: PendingPress | null = null;
  let active: ActiveDrag | null = null;
  let settleTimer: ReturnType<typeof setTimeout> | null = null;

  function addPressListeners(): void {
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", handlePointerCancel);
  }

  function removeListeners(): void {
    window.removeEventListener("pointermove", handlePointerMove);
    window.removeEventListener("pointerup", handlePointerUp);
    window.removeEventListener("pointercancel", handlePointerCancel);
    window.removeEventListener("keydown", handleKeydown, { capture: true });
    window.removeEventListener("blur", cancel);
    document.documentElement.classList.remove(DRAGGING_ROOT_CLASS);
  }

  function clearSettleTimer(): void {
    if (settleTimer === null) return;
    clearTimeout(settleTimer);
    settleTimer = null;
  }

  function releaseCapture(drag: ActiveDrag): void {
    try {
      if (drag.handle.hasPointerCapture(drag.pointerId)) drag.handle.releasePointerCapture(drag.pointerId);
    } catch {
      // A detached handle has no capture left to release.
    }
  }

  /** Return the vertical offset of the item `id`: the pointer's for the dragged item, the slide for its siblings. */
  function offsetFor(id: string): number {
    if (!active || draggedId === null) return 0;
    if (id === draggedId) return offset;
    return sortableDragShift(active.slots, active.ids.indexOf(id), active.source, target);
  }

  /** Register an item element and keep its drag offset and phase in sync with the drag. */
  function item(id: string): Attachment<HTMLElement> {
    return (element) => {
      elements.set(id, element);
      element.dataset.sortableItem = "";
      element.style.setProperty("--sortable-duration", `${SORTABLE_DRAG_SETTLE_MS}ms`);
      $effect(() => {
        const itemOffset = offsetFor(id);
        const itemPhase = draggedId === id ? phase : null;
        element.style.translate = itemOffset ? `0 ${itemOffset}px` : "";
        if (itemPhase) element.dataset.sortablePhase = itemPhase;
        else delete element.dataset.sortablePhase;
        if (shiftAnimated) element.dataset.sortableAnimated = "";
        else delete element.dataset.sortableAnimated;
      });
      return () => {
        if (elements.get(id) === element) elements.delete(id);
      };
    };
  }

  /** Track a press on the handle of `id`; it becomes a drag only once the pointer moves past the threshold. */
  function pointerDown(event: PointerEvent, id: string): void {
    if (event.button !== 0 || pending || active) return;
    if (!(event.currentTarget instanceof HTMLElement)) return;
    if (options.canStart?.() === false || options.getIds(id).length < 2) return;
    pending = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, id, handle: event.currentTarget };
    addPressListeners();
  }

  function lift(event: PointerEvent, press: PendingPress): void {
    const ids = [...options.getIds(press.id)];
    const source = ids.indexOf(press.id);
    const itemElements = ids.map((id) => elements.get(id));
    const list = elements.get(press.id)?.parentElement;
    if (source < 0 || !list || itemElements.some((element) => element === undefined)) return endPress();
    const listTop = list.getBoundingClientRect().top;
    const slots = itemElements.map((element) => {
      const rect = element?.getBoundingClientRect();
      return { top: (rect?.top ?? 0) - listTop, height: rect?.height ?? 0 };
    });
    active = {
      pointerId: press.pointerId,
      id: press.id,
      handle: press.handle,
      ids,
      source,
      slots,
      list,
      grabY: press.y - listTop,
      bounds: sortableDragBounds(slots, source),
    };
    pending = null;
    try {
      press.handle.setPointerCapture(press.pointerId);
    } catch {
      // The pointer may already be gone; window listeners still track it.
    }
    window.addEventListener("keydown", handleKeydown, { capture: true });
    window.addEventListener("blur", cancel);
    document.documentElement.classList.add(DRAGGING_ROOT_CLASS);
    window.getSelection()?.removeAllRanges();
    shiftAnimated = true;
    target = source;
    offset = 0;
    phase = "lifted";
    draggedId = press.id;
    follow(event);
  }

  /** Move the lifted item with the pointer, held between the first and last slots, and open the gap it hovers. */
  function follow(event: PointerEvent): void {
    if (!active) return;
    const { list, grabY, bounds, slots, source } = active;
    offset = clamp(event.clientY - list.getBoundingClientRect().top - grabY, bounds.min, bounds.max);
    target = sortableDragTarget(slots, source, offset);
  }

  function handlePointerMove(event: PointerEvent): void {
    if (active) {
      if (event.pointerId === active.pointerId && phase === "lifted") follow(event);
      return;
    }
    if (!pending || event.pointerId !== pending.pointerId) return;
    if (Math.hypot(event.clientX - pending.x, event.clientY - pending.y) < SORTABLE_DRAG_THRESHOLD) return;
    lift(event, pending);
  }

  function handlePointerUp(event: PointerEvent): void {
    if (active) {
      if (event.pointerId === active.pointerId) settle(active, target);
      return;
    }
    if (pending && event.pointerId === pending.pointerId) endPress();
  }

  function handlePointerCancel(event: PointerEvent): void {
    if (active ? event.pointerId === active.pointerId : pending?.pointerId === event.pointerId) cancel();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    cancel();
  }

  /** Put the lifted item back in its own slot while its siblings slide closed. */
  function cancel(): void {
    if (!active) return endPress();
    if (phase === "lifted") settle(active, active.source);
  }

  /** Glide the item into the slot at `landing`, then commit the move once it rests there. */
  function settle(drag: ActiveDrag, landing: number): void {
    removeListeners();
    releaseCapture(drag);
    target = landing;
    if (prefersReducedMotion()) return finish();
    offset = sortableDragLanding(drag.slots, drag.source, landing);
    phase = "settling";
    clearSettleTimer();
    settleTimer = setTimeout(finish, SORTABLE_DRAG_SETTLE_MS);
  }

  /** Clear every offset without sliding, in the same update as the reorder so the items stay where they rest. */
  function finish(): void {
    clearSettleTimer();
    const drag = active;
    const landing = target;
    active = null;
    shiftAnimated = false;
    draggedId = null;
    phase = null;
    offset = 0;
    if (!drag || landing === drag.source) return;
    const committed = options.commit(drag.id, landing);
    if (committed instanceof Promise) {
      committed.catch((error: unknown) => console.error("Failed to commit a sortable drag move", error));
    }
  }

  function endPress(): void {
    pending = null;
    if (!active) removeListeners();
  }

  /** Drop any press or drag in progress without moving anything, for when the list reloads or unmounts. */
  function reset(): void {
    pending = null;
    if (active) releaseCapture(active);
    removeListeners();
    clearSettleTimer();
    active = null;
    draggedId = null;
    phase = null;
    offset = 0;
  }

  return {
    get draggedId() { return draggedId; },
    get phase() { return phase; },
    item,
    pointerDown,
    cancel,
    reset,
  };
}

export type SortableDrag = ReturnType<typeof createSortableDrag>;
