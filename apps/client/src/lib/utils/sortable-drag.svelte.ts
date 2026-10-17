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
import { suppressClickAfterRelease } from "./click-suppression";
import { TouchHoldArbiter } from "./touch-hold";

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
  /**
   * Whether a touch lifts the item only after a stationary hold, so a swipe over the list keeps scrolling it. Without
   * it, a touch drags as soon as it moves, which suits a handle with `touch-action: none`.
   */
  touchHold?: boolean;
  /** Direction the items run in: `y` for a column, the default, or `x` for a row. A getter may change it between drags. */
  axis?: SortableAxis;
}

/** Direction a sortable list runs in. */
export type SortableAxis = "x" | "y";

interface PendingPress {
  pointerId: number;
  x: number;
  y: number;
  id: string;
  handle: HTMLElement;
  /** The press waits for a touch hold instead of a movement threshold. */
  hold: boolean;
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
  /** Pointer position along the axis, relative to `list`, when the drag started. */
  grab: number;
  bounds: { min: number; max: number };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * Pointer-driven reordering for one list, a column or a row, whose items move in place. Pressing an item's drag handle
 * and moving past the threshold lifts the item, which then follows the pointer along the list while the items between
 * its slot and the hovered slot slide aside; release settles it into the gap and then commits the move, so the list
 * reorders without a jump. Escape, a canceled pointer, or leaving the window returns the item to its slot. Items may
 * differ in size.
 *
 * Attach `item(id)` to each item element and call `pointerDown` from its handle. The handle should carry
 * `touch-action: none` so touch drags instead of scrolling, unless `touchHold` lets a hold take the touch from scrolling.
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
  const touchHold = new TouchHoldArbiter();
  const horizontal = (): boolean => options.axis === "x";

  /** Return a pointer position along the axis. */
  function along(x: number, y: number): number {
    return horizontal() ? x : y;
  }

  /** Return where an element starts along the axis, and its size there. */
  function extent(element: Element): SortableSlot {
    const rect = element.getBoundingClientRect();
    return horizontal() ? { start: rect.left, size: rect.width } : { start: rect.top, size: rect.height };
  }

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
    touchHold.finish();
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

  /** Return the offset along the axis of the item `id`: the pointer's for the dragged item, the slide for its siblings. */
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
        element.style.translate = !itemOffset ? "" : horizontal() ? `${itemOffset}px 0` : `0 ${itemOffset}px`;
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
    const hold = options.touchHold === true && event.pointerType === "touch";
    const press: PendingPress = {
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      id,
      handle: event.currentTarget,
      hold,
    };
    pending = press;
    addPressListeners();
    if (!hold) return;
    touchHold.begin(event, () => {
      if (pending === press && !active) lift(along(press.x, press.y), press);
    });
  }

  function lift(position: number, press: PendingPress): void {
    const ids = [...options.getIds(press.id)];
    const source = ids.indexOf(press.id);
    const itemElements = ids.map((id) => elements.get(id));
    const list = elements.get(press.id)?.parentElement;
    if (source < 0 || !list || itemElements.some((element) => element === undefined)) return endPress();
    const listStart = extent(list).start;
    const slots = itemElements.map((element) => {
      const slot = element ? extent(element) : { start: listStart, size: 0 };
      return { start: slot.start - listStart, size: slot.size };
    });
    active = {
      pointerId: press.pointerId,
      id: press.id,
      handle: press.handle,
      ids,
      source,
      slots,
      list,
      grab: along(press.x, press.y) - listStart,
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
    follow(position);
  }

  /** Move the lifted item with the pointer, held between the first and last slots, and open the gap it hovers. */
  function follow(position: number): void {
    if (!active) return;
    const { list, grab, bounds, slots, source } = active;
    offset = clamp(position - extent(list).start - grab, bounds.min, bounds.max);
    target = sortableDragTarget(slots, source, offset);
  }

  function handlePointerMove(event: PointerEvent): void {
    if (active) {
      if (event.pointerId === active.pointerId && phase === "lifted") follow(along(event.clientX, event.clientY));
      return;
    }
    if (!pending || event.pointerId !== pending.pointerId) return;
    // A touch hold lifts from its timer; earlier movement belongs to native scrolling.
    if (pending.hold) return;
    if (Math.hypot(event.clientX - pending.x, event.clientY - pending.y) < SORTABLE_DRAG_THRESHOLD) return;
    lift(along(event.clientX, event.clientY), pending);
  }

  function handlePointerUp(event: PointerEvent): void {
    if (active) {
      if (event.pointerId === active.pointerId) {
        suppressClickAfterRelease();
        settle(active, target);
      }
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
