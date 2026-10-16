import {
  collectionColumnAutoScrollStep,
  collectionColumnDragLeft,
  collectionColumnDropIndex,
  collectionColumnOffsets,
  type CollectionColumnSlot,
} from "./column-reorder";
import { suppressClickAfterRelease } from "$lib/utils/click-suppression";

/** Pixels the pointer travels horizontally before a press on a column name becomes a drag. */
const DRAG_THRESHOLD = 4;
/** Marks the document while a column is dragged, so hover feedback, text selection, and cursors pause app-wide. */
const DRAGGING_ATTRIBUTE = "data-collection-column-dragging";
/** Stacks the dragged header above frozen header cells while it slides over them. */
const DRAGGED_Z_INDEX = 40;
/** Header cells that take part in a column drag carry their column ID in this attribute. */
export const COLLECTION_COLUMN_ID_ATTRIBUTE = "data-collection-column-id";
/** Present on header cells whose column can be dragged to a new place. */
export const COLLECTION_COLUMN_MOVABLE_ATTRIBUTE = "data-collection-column-movable";
/** Targets inside a header cell that keep their own pointer behavior instead of starting a column drag. */
const IGNORED_TARGETS = "[data-collection-resize], [aria-expanded='true'], input, textarea, select, [contenteditable='true'], [role='menu'], [role='dialog'], [role='listbox']";

/** Horizontal scroll container that a column drag scrolls near its edges. */
export interface CollectionColumnScroller {
  element: HTMLElement;
  /** Scroll horizontally to `left` and return the offset the container accepted. */
  scrollTo: (left: number) => number;
}

export interface CollectionColumnReorderOptions {
  /** Whether a drag may start now, such as while no save is pending and editing is unlocked. */
  canStart: () => boolean;
  /** Move the dragged column to `targetIndex` among the column cells of the header that started the drag. */
  onReorder: (columnId: string, targetIndex: number) => void;
  /** Container to auto-scroll; defaults to the nearest ancestor of the header row that scrolls horizontally. */
  scroller?: (row: HTMLElement) => CollectionColumnScroller | null;
}

/** What the header cells render while a drag previews the new column order. */
export interface CollectionColumnDragPreview {
  rowKey: string;
  columnId: string;
  /** Translation of the dragged header cell from its own place. */
  translate: number;
  /** Translation of the other header cells that make room for the dragged one. */
  offsets: ReadonlyMap<string, number>;
  /** Whether a frozen dragged cell leaves sticky positioning, so it follows the pointer while the table scrolls. */
  unstick: boolean;
}

interface ColumnPress {
  pointerId: number;
  startX: number;
  rowKey: string;
  columnId: string;
  cell: HTMLElement;
  row: HTMLElement;
  /** Set once the press became a drag, so the click that ends it is swallowed even after a cancel. */
  dragged: boolean;
  /** Set when Escape ended the preview while the button is still down. */
  cancelled: boolean;
}

interface ColumnDragSession {
  slots: CollectionColumnSlot[];
  startIndex: number;
  targetIndex: number;
  /** Left edge of the dragged slot in the session's coordinate frame. */
  startLeft: number;
  /** Distance from the dragged cell's left edge to the pointer when the drag started. */
  grab: number;
  /** Client X of the coordinate frame's origin, read again on each update because the frame can scroll. */
  origin: () => number;
  scroller: CollectionColumnScroller | null;
}

interface MeasuredColumnCell {
  cell: HTMLElement;
  rect: DOMRect;
  sticky: boolean;
}

/**
 * Pointer drag that reorders table columns by their header cells, like Notion: the pressed header slides with the
 * pointer while the other headers make room, and rows follow only after release. Mouse and pen only; touch keeps
 * native panning and uses the column menu's move actions, which also serve keyboards.
 *
 * Positions use the header row's layout, so auto-scrolling does not disturb them. A frozen column dragged while the
 * table is scrolled is stuck to the container edge instead; it then reorders among the frozen columns in container
 * coordinates, without auto-scrolling.
 */
export class CollectionColumnReorder {
  #options: CollectionColumnReorderOptions;
  #press: ColumnPress | null = null;
  #session: ColumnDragSession | null = null;
  #lastClientX = 0;
  #scrollFrame = 0;
  #listening = false;
  preview = $state.raw<CollectionColumnDragPreview | null>(null);

  constructor(options: CollectionColumnReorderOptions) {
    this.#options = options;
  }

  /** Whether this header cell is the one being dragged. */
  dragged(rowKey: string, columnId: string): boolean {
    return this.preview?.rowKey === rowKey && this.preview.columnId === columnId;
  }

  /** Whether this header cell makes room for a dragged column in its row. */
  shifting(rowKey: string, columnId: string): boolean {
    return this.preview?.rowKey === rowKey && this.preview.columnId !== columnId;
  }

  /** Inline style for one header cell while a drag previews the new column order. */
  cellStyle(rowKey: string, columnId: string): string {
    const preview = this.preview;
    if (!preview || preview.rowKey !== rowKey) return "";
    if (preview.columnId === columnId) {
      return `transform: translateX(${preview.translate}px); z-index: ${DRAGGED_Z_INDEX};${preview.unstick ? " position: relative; left: auto;" : ""}`;
    }
    const offset = preview.offsets.get(columnId) ?? 0;
    return offset === 0 ? "" : `transform: translateX(${offset}px);`;
  }

  /** Begin watching a press on a movable header cell; it becomes a drag once the pointer travels sideways. */
  pointerDown = (event: PointerEvent, rowKey: string, columnId: string): void => {
    if (event.button !== 0 || event.pointerType === "touch" || this.#press || !this.#options.canStart()) return;
    const cell = event.currentTarget;
    if (!(cell instanceof HTMLElement) || !(event.target instanceof Element) || event.target.closest(IGNORED_TARGETS)) return;
    const row = cell.closest<HTMLElement>("[role='row']");
    if (!row) return;
    this.#press = { pointerId: event.pointerId, startX: event.clientX, rowKey, columnId, cell, row, dragged: false, cancelled: false };
    this.#listen(true);
  };

  /** Stop any press or drag without reordering, such as when the table unmounts. */
  destroy(): void {
    this.#finish(false);
  }

  #listen(on: boolean): void {
    if (on === this.#listening) return;
    this.#listening = on;
    if (on) {
      window.addEventListener("pointermove", this.#move);
      window.addEventListener("pointerup", this.#up);
      window.addEventListener("pointercancel", this.#cancel);
      window.addEventListener("blur", this.#blur);
      window.addEventListener("keydown", this.#keydown, true);
      return;
    }
    window.removeEventListener("pointermove", this.#move);
    window.removeEventListener("pointerup", this.#up);
    window.removeEventListener("pointercancel", this.#cancel);
    window.removeEventListener("blur", this.#blur);
    window.removeEventListener("keydown", this.#keydown, true);
  }

  #move = (event: PointerEvent): void => {
    const press = this.#press;
    if (!press || press.cancelled || event.pointerId !== press.pointerId) return;
    this.#lastClientX = event.clientX;
    if (!this.#session) {
      if (Math.abs(event.clientX - press.startX) < DRAG_THRESHOLD) return;
      if (!this.#options.canStart() || !this.#begin(press)) {
        this.#finish(false);
        return;
      }
    }
    this.#update();
    this.#scheduleAutoScroll();
  };

  #up = (event: PointerEvent): void => {
    if (this.#press && event.pointerId === this.#press.pointerId) this.#finish(true);
  };

  #cancel = (event: PointerEvent): void => {
    if (this.#press && event.pointerId === this.#press.pointerId) this.#finish(false);
  };

  #blur = (): void => this.#finish(false);

  /** Escape drops the preview but keeps watching the press, so releasing the button afterwards opens no menu. */
  #keydown = (event: KeyboardEvent): void => {
    if (event.key !== "Escape" || !this.#press || !this.#session) return;
    event.preventDefault();
    event.stopPropagation();
    this.#press.cancelled = true;
    this.#endPreview();
  };

  /** Measure the pressed header row and lift the pressed cell; returns false when the row has nothing to reorder. */
  #begin(press: ColumnPress): boolean {
    const measured = measureColumnCells(press.row);
    const startIndex = measured.findIndex((entry) => entry.cell === press.cell);
    const dragged = measured[startIndex];
    if (!dragged || !dragged.cell.hasAttribute(COLLECTION_COLUMN_MOVABLE_ATTRIBUTE)) return false;
    const rowLeft = press.row.getBoundingClientRect().left;
    const layoutLefts = columnLayoutLefts(press.row, measured, rowLeft);
    const visualLeft = dragged.rect.left - rowLeft;
    const scroller = this.#options.scroller ? this.#options.scroller(press.row) : defaultScroller(press.row);
    const stuck = dragged.sticky && Math.abs(visualLeft - (layoutLefts[startIndex] ?? visualLeft)) > 0.5;
    let slots: CollectionColumnSlot[];
    let origin: () => number;
    if (stuck) {
      if (!scroller) return false;
      const scrollerLeft = scroller.element.getBoundingClientRect().left;
      slots = measured.filter((entry) => entry.sticky).map((entry) => columnSlot(entry, entry.rect.left - scrollerLeft));
      origin = () => scroller.element.getBoundingClientRect().left;
    } else {
      slots = measured.map((entry, index) => columnSlot(entry, layoutLefts[index] ?? entry.rect.left - rowLeft));
      origin = () => press.row.getBoundingClientRect().left;
    }
    if (slots.filter((slot) => slot.movable).length < 2) return false;
    const startLeft = slots[startIndex]?.left ?? 0;
    this.#session = {
      slots,
      startIndex,
      targetIndex: startIndex,
      startLeft,
      grab: press.startX - origin() - (stuck ? startLeft : visualLeft),
      origin,
      scroller: stuck ? null : scroller,
    };
    press.dragged = true;
    try {
      press.cell.setPointerCapture(press.pointerId);
    } catch {
      // The pointer can already be gone; the window listeners still end the drag.
    }
    window.getSelection()?.removeAllRanges();
    document.documentElement.setAttribute(DRAGGING_ATTRIBUTE, "");
    this.preview = { rowKey: press.rowKey, columnId: press.columnId, translate: 0, offsets: new Map(), unstick: dragged.sticky && !stuck };
    return true;
  }

  /** Follow the pointer in the session's frame and recompute the drop target and the room other headers make. */
  #update(): void {
    const session = this.#session;
    const press = this.#press;
    const preview = this.preview;
    if (!session || !press || !preview) return;
    if (!press.cell.isConnected) {
      this.#finish(false);
      return;
    }
    const pointer = this.#lastClientX - session.origin();
    const left = collectionColumnDragLeft(session.slots, press.columnId, pointer - session.grab);
    const targetIndex = collectionColumnDropIndex(session.slots, press.columnId, left);
    const translate = left - session.startLeft;
    if (translate === preview.translate && targetIndex === session.targetIndex) return;
    const offsets = targetIndex === session.targetIndex ? preview.offsets : collectionColumnOffsets(session.slots, press.columnId, targetIndex);
    session.targetIndex = targetIndex;
    this.preview = { ...preview, translate, offsets };
  }

  #scheduleAutoScroll(): void {
    if (this.#scrollFrame || !this.#session?.scroller || typeof requestAnimationFrame !== "function") return;
    this.#scrollFrame = requestAnimationFrame(this.#autoScroll);
  }

  #autoScroll = (): void => {
    this.#scrollFrame = 0;
    const scroller = this.#session?.scroller;
    const press = this.#press;
    if (!scroller || !press) return;
    const rect = scroller.element.getBoundingClientRect();
    const step = collectionColumnAutoScrollStep(this.#lastClientX, rect.left, rect.right, frozenInset(press.row, press.cell, rect.left));
    if (step === 0) return;
    const before = scroller.element.scrollLeft;
    if (scroller.scrollTo(before + step) === before) return;
    this.#update();
    this.#scheduleAutoScroll();
  };

  /** Drop the drag preview and its document state while leaving the press, if any, to its listeners. */
  #endPreview(): void {
    this.#session = null;
    this.preview = null;
    if (this.#scrollFrame && typeof cancelAnimationFrame === "function") cancelAnimationFrame(this.#scrollFrame);
    this.#scrollFrame = 0;
    document.documentElement.removeAttribute(DRAGGING_ATTRIBUTE);
  }

  #finish(commit: boolean): void {
    const press = this.#press;
    const session = this.#session;
    this.#press = null;
    this.#listen(false);
    this.#endPreview();
    if (!press?.dragged) return;
    try {
      if (press.cell.hasPointerCapture(press.pointerId)) press.cell.releasePointerCapture(press.pointerId);
    } catch {
      // Capture already ended with the pointer.
    }
    suppressClickAfterRelease();
    if (commit && session && session.targetIndex !== session.startIndex) this.#options.onReorder(press.columnId, session.targetIndex);
  }
}

/** Header cells of one row that take part in column dragging, in visual order. */
function measureColumnCells(row: HTMLElement): MeasuredColumnCell[] {
  return Array.from(row.querySelectorAll<HTMLElement>(`[${COLLECTION_COLUMN_ID_ATTRIBUTE}]`))
    .filter((cell) => cell.closest("[role='row']") === row)
    .map((cell) => ({ cell, rect: cell.getBoundingClientRect(), sticky: getComputedStyle(cell).position === "sticky" }));
}

function columnSlot(entry: MeasuredColumnCell, left: number): CollectionColumnSlot {
  return {
    id: entry.cell.getAttribute(COLLECTION_COLUMN_ID_ATTRIBUTE) ?? "",
    left,
    width: entry.rect.width,
    movable: entry.cell.hasAttribute(COLLECTION_COLUMN_MOVABLE_ATTRIBUTE),
  };
}

/**
 * Left edges of the column cells in the header row's own layout. A frozen cell reports where it is stuck while the
 * table is scrolled, so its layout place is derived from the cell that follows it; frozen columns always lead.
 */
function columnLayoutLefts(row: HTMLElement, measured: readonly MeasuredColumnCell[], rowLeft: number): number[] {
  const gap = Number.parseFloat(getComputedStyle(row).columnGap);
  const columnGap = Number.isFinite(gap) ? gap : 0;
  const lefts = measured.map((entry) => entry.rect.left - rowLeft);
  const following = measured.at(-1)?.cell.nextElementSibling;
  let next = following instanceof HTMLElement && getComputedStyle(following).position !== "sticky" ? following.getBoundingClientRect().left - rowLeft : null;
  for (let index = measured.length - 1; index >= 0; index -= 1) {
    const entry = measured[index];
    if (!entry) continue;
    if (entry.sticky && next !== null) lefts[index] = next - columnGap - entry.rect.width;
    next = lefts[index] ?? null;
  }
  return lefts;
}

/** Nearest ancestor that scrolls horizontally, assigned through its native scroll offset. */
function defaultScroller(row: HTMLElement): CollectionColumnScroller | null {
  for (let node = row.parentElement; node; node = node.parentElement) {
    const overflow = getComputedStyle(node).overflowX;
    if ((overflow === "auto" || overflow === "scroll") && node.scrollWidth > node.clientWidth) {
      const element = node;
      return {
        element,
        scrollTo: (left) => {
          element.scrollLeft = left;
          return element.scrollLeft;
        },
      };
    }
  }
  return null;
}

/** Width of frozen header cells covering the scroller's left edge, so the left auto-scroll zone starts after them. */
function frozenInset(row: HTMLElement, dragged: HTMLElement, scrollerLeft: number): number {
  let right = scrollerLeft;
  for (const child of Array.from(row.children)) {
    if (child === dragged || !(child instanceof HTMLElement) || getComputedStyle(child).position !== "sticky") continue;
    right = Math.max(right, child.getBoundingClientRect().right);
  }
  return right - scrollerLeft;
}
