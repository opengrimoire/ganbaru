import type { ProjectView } from "$lib/projects/types";
import {
  PROJECT_VIEW_DRAG_SETTLE_MS,
  PROJECT_VIEW_DRAG_THRESHOLD,
  projectViewDragPlacement,
  projectViewDragRange,
  projectViewDragShift,
  projectViewDragTarget,
  type ProjectViewDragCardPhase,
  type ProjectViewDragFrame,
  type ProjectViewDragRange,
} from "$lib/projects/view-drag";

/** Class on the document root while a view drag is active, so the cursor and text selection hold across the page. */
const DRAGGING_ROOT_CLASS = "project-view-dragging";

export interface ProjectViewDragCard {
  view: ProjectView;
  frame: ProjectViewDragFrame;
  phase: ProjectViewDragCardPhase;
}

export interface ProjectViewDragControllerOptions {
  /** Views in sidebar order, matching the rendered rows. */
  getViews: () => readonly ProjectView[];
  isStarred: (viewId: string) => boolean;
  /** The element holding the rows, which also anchors slot positions while the sidebar scrolls. */
  getContainer: () => HTMLElement | null;
  /** Commits a drop and reports whether the order changed. */
  move: (viewId: string, targetId: string, placement: "before" | "after") => boolean;
}

interface PendingPress {
  pointerId: number;
  x: number;
  y: number;
  view: ProjectView;
  row: HTMLElement;
}

interface ActiveDrag {
  pointerId: number;
  row: HTMLElement;
  view: ProjectView;
  ids: string[];
  source: number;
  range: ProjectViewDragRange;
  /** Row tops relative to the container when the drag started, before any row slides. */
  slotOffsets: number[];
  grabY: number;
  left: number;
  width: number;
  height: number;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function swallowClick(event: MouseEvent): void {
  event.preventDefault();
  event.stopPropagation();
}

/** Keep the click that follows a pointer release from opening the view that was just dragged. */
function suppressClickAfterRelease(): void {
  window.addEventListener("click", swallowClick, { capture: true, once: true });
  window.setTimeout(() => window.removeEventListener("click", swallowClick, { capture: true }), 0);
}

/**
 * Pointer-driven reordering for the views of one Projects sidebar section. A mouse or pen press that travels past the
 * threshold lifts a card copy of the row; the rows between its slot and the hovered slot slide aside to open a gap,
 * and release settles the card into that gap. Escape, a canceled pointer, or leaving the window returns the row.
 * Touch keeps the Move up and Move down menu items.
 */
export function createProjectViewDragController(options: ProjectViewDragControllerOptions) {
  let card = $state<ProjectViewDragCard | null>(null);
  let draggedId = $state<string | null>(null);
  let target = $state(0);
  let shifting = $state(false);
  let shiftAnimated = $state(true);
  let pending: PendingPress | null = null;
  let active: ActiveDrag | null = null;
  let settleTimer: ReturnType<typeof setTimeout> | null = null;

  function containerTop(): number {
    return options.getContainer()?.getBoundingClientRect().top ?? 0;
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
  }

  function clearSettleTimer(): void {
    if (settleTimer === null) return;
    clearTimeout(settleTimer);
    settleTimer = null;
  }

  /** Track a press on a row; it becomes a drag only once the pointer moves past the threshold. */
  function pointerDown(event: PointerEvent, view: ProjectView, row: HTMLElement): void {
    if (event.button !== 0 || event.pointerType === "touch" || pending || card) return;
    if (event.target instanceof Element && event.target.closest("input, .explorer-row-action")) return;
    pending = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, view, row };
    addPressListeners();
  }

  function lift(event: PointerEvent, press: PendingPress): void {
    const container = options.getContainer();
    const ids = options.getViews().map((view) => view.id);
    const source = ids.indexOf(press.view.id);
    if (!container || source < 0) return endPress();
    const rowsById = new Map<string, HTMLElement>();
    for (const row of container.querySelectorAll<HTMLElement>("[data-project-view-id]")) {
      const id = row.dataset.projectViewId;
      if (id) rowsById.set(id, row);
    }
    const rects = ids.map((id) => rowsById.get(id)?.getBoundingClientRect());
    const sourceRect = rects[source];
    if (!sourceRect || rects.some((rect) => rect === undefined)) return endPress();
    const top = container.getBoundingClientRect().top;
    active = {
      pointerId: press.pointerId,
      row: press.row,
      view: press.view,
      ids,
      source,
      range: projectViewDragRange(ids.map(options.isStarred), source),
      slotOffsets: rects.map((rect) => (rect?.top ?? 0) - top),
      grabY: clamp(press.y - sourceRect.top, 0, sourceRect.height),
      left: sourceRect.left,
      width: sourceRect.width,
      height: sourceRect.height,
    };
    pending = null;
    try {
      press.row.setPointerCapture(press.pointerId);
    } catch {
      // The pointer may already be gone; window listeners still track it.
    }
    window.addEventListener("keydown", handleKeydown, { capture: true });
    window.addEventListener("blur", cancel);
    document.documentElement.classList.add(DRAGGING_ROOT_CLASS);
    window.getSelection()?.removeAllRanges();
    draggedId = press.view.id;
    target = source;
    shiftAnimated = true;
    shifting = true;
    card = {
      view: press.view,
      phase: "lifted",
      frame: { left: sourceRect.left, top: sourceRect.top, width: sourceRect.width, height: sourceRect.height },
    };
    follow(event);
  }

  /** Move the card with the pointer, held to the slots of its starred run, and open the gap under its center. */
  function follow(event: PointerEvent): void {
    if (!active || !card) return;
    const top = containerTop();
    const { slotOffsets, range, grabY, height } = active;
    const cardTop = clamp(
      event.clientY - grabY,
      top + (slotOffsets[range.start] ?? 0),
      top + (slotOffsets[range.end] ?? 0),
    );
    card = { ...card, frame: { ...card.frame, top: cardTop } };
    target = projectViewDragTarget(slotOffsets, range, cardTop - top + height / 2);
  }

  function handlePointerMove(event: PointerEvent): void {
    if (active) {
      if (event.pointerId === active.pointerId) follow(event);
      return;
    }
    if (!pending || event.pointerId !== pending.pointerId) return;
    if (Math.hypot(event.clientX - pending.x, event.clientY - pending.y) < PROJECT_VIEW_DRAG_THRESHOLD) return;
    lift(event, pending);
  }

  function handlePointerUp(event: PointerEvent): void {
    if (active) {
      if (event.pointerId !== active.pointerId) return;
      suppressClickAfterRelease();
      drop(active);
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
    window.addEventListener("pointerup", suppressClickAfterRelease, { capture: true, once: true });
    cancel();
  }

  function drop(drag: ActiveDrag): void {
    const placement = projectViewDragPlacement(drag.source, target);
    const targetId = drag.ids[target];
    const moved = placement !== null && targetId !== undefined && options.move(drag.view.id, targetId, placement);
    const landing = moved ? target : drag.source;
    if (moved) {
      // The rows reorder in this same update, so the gap closes without sliding the rows a second time.
      shiftAnimated = false;
      shifting = false;
    } else {
      target = drag.source;
    }
    settle(drag, landing);
  }

  /** Put the lifted row back in its own slot while the rows slide closed. */
  function cancel(): void {
    if (!active) return endPress();
    target = active.source;
    settle(active, active.source);
  }

  function settle(drag: ActiveDrag, index: number): void {
    removeListeners();
    try {
      if (drag.row.hasPointerCapture(drag.pointerId)) drag.row.releasePointerCapture(drag.pointerId);
    } catch {
      // A detached row has no capture left to release.
    }
    if (!card || prefersReducedMotion()) return finish();
    card = {
      ...card,
      phase: "settling",
      frame: { left: drag.left, top: containerTop() + (drag.slotOffsets[index] ?? 0), width: drag.width, height: drag.height },
    };
    clearSettleTimer();
    settleTimer = setTimeout(finish, PROJECT_VIEW_DRAG_SETTLE_MS);
  }

  function finish(): void {
    clearSettleTimer();
    card = null;
    draggedId = null;
    active = null;
    shifting = false;
    shiftAnimated = true;
  }

  function endPress(): void {
    pending = null;
    if (!active) removeListeners();
  }

  /** Drop any drag in progress without moving the view, for when the section unmounts. */
  function destroy(): void {
    pending = null;
    removeListeners();
    finish();
  }

  /** Return how far the row of `viewId` slides to open the gap under the lifted card. */
  function shiftFor(viewId: string): number {
    if (!shifting || !active) return 0;
    return projectViewDragShift(active.slotOffsets, active.ids.indexOf(viewId), active.source, target);
  }

  return {
    get card() { return card; },
    get draggedId() { return draggedId; },
    get shiftAnimated() { return shiftAnimated; },
    pointerDown,
    shiftFor,
    destroy,
  };
}
