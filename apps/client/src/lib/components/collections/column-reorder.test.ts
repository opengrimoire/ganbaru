// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  COLLECTION_COLUMN_AUTO_SCROLL_MAX_STEP,
  collectionColumnAutoScrollStep,
  collectionColumnDragLeft,
  collectionColumnDropIndex,
  collectionColumnOffsets,
  collectionMoveItem,
  collectionOrderById,
  collectionPlaceVisibleItem,
  type CollectionColumnSlot,
} from "./column-reorder";
import {
  COLLECTION_COLUMN_ID_ATTRIBUTE,
  COLLECTION_COLUMN_MOVABLE_ATTRIBUTE,
  CollectionColumnReorder,
  type CollectionColumnReorderOptions,
} from "./column-reorder.svelte";

/** A fixed title column followed by three movable columns of different widths. */
const SLOTS: CollectionColumnSlot[] = [
  { id: "title", left: 0, width: 200, movable: false },
  { id: "a", left: 200, width: 100, movable: true },
  { id: "b", left: 300, width: 150, movable: true },
  { id: "c", left: 450, width: 120, movable: true },
];

describe("collection column reorder geometry", () => {
  it("passes a following column once the dragged right edge crosses its center", () => {
    expect(collectionColumnDropIndex(SLOTS, "a", 200)).toBe(1);
    expect(collectionColumnDropIndex(SLOTS, "a", 275)).toBe(1);
    expect(collectionColumnDropIndex(SLOTS, "a", 276)).toBe(2);
    expect(collectionColumnDropIndex(SLOTS, "a", 500)).toBe(3);
  });

  it("passes a preceding column once the dragged left edge crosses its center", () => {
    expect(collectionColumnDropIndex(SLOTS, "b", 251)).toBe(2);
    expect(collectionColumnDropIndex(SLOTS, "b", 249)).toBe(1);
  });

  it("never lands before the leading fixed columns and ignores unknown columns", () => {
    expect(collectionColumnDropIndex(SLOTS, "c", -400)).toBe(1);
    expect(collectionColumnDropIndex(SLOTS, "missing", 0)).toBe(-1);
    const fixedPair = [{ ...SLOTS[0]! }, { ...SLOTS[1]!, movable: false }, SLOTS[2]!, SLOTS[3]!];
    expect(collectionColumnDropIndex(fixedPair, "c", 0)).toBe(2);
  });

  it("shifts only the passed columns by the dragged width toward its original place", () => {
    expect([...collectionColumnOffsets(SLOTS, "a", 3)]).toEqual([["b", -100], ["c", -100]]);
    expect([...collectionColumnOffsets(SLOTS, "c", 1)]).toEqual([["a", 120], ["b", 120]]);
    expect(collectionColumnOffsets(SLOTS, "b", 2).size).toBe(0);
    expect(collectionColumnOffsets(SLOTS, "missing", 1).size).toBe(0);
  });

  it("keeps the dragged column between the first movable column and the end of the header", () => {
    expect(collectionColumnDragLeft(SLOTS, "b", -50)).toBe(200);
    expect(collectionColumnDragLeft(SLOTS, "b", 333)).toBe(333);
    expect(collectionColumnDragLeft(SLOTS, "b", 1000)).toBe(420);
    expect(collectionColumnDragLeft(SLOTS, "missing", 1000)).toBe(1000);
  });

  it("scrolls faster toward each edge and starts the left zone after frozen columns", () => {
    expect(collectionColumnAutoScrollStep(250, 0, 500)).toBe(0);
    expect(collectionColumnAutoScrollStep(0, 0, 500)).toBe(-COLLECTION_COLUMN_AUTO_SCROLL_MAX_STEP);
    expect(collectionColumnAutoScrollStep(24, 0, 500)).toBe(-6);
    expect(collectionColumnAutoScrollStep(476, 0, 500)).toBe(6);
    expect(collectionColumnAutoScrollStep(900, 0, 500)).toBe(COLLECTION_COLUMN_AUTO_SCROLL_MAX_STEP);
    expect(collectionColumnAutoScrollStep(110, 0, 500, 100)).toBe(-10);
    expect(collectionColumnAutoScrollStep(50, 0, 500, 100)).toBe(-COLLECTION_COLUMN_AUTO_SCROLL_MAX_STEP);
    expect(collectionColumnAutoScrollStep(160, 0, 500, 100)).toBe(0);
  });
});

describe("collection column order helpers", () => {
  it("moves one item and clamps the target index", () => {
    expect(collectionMoveItem(["a", "b", "c"], "a", 2)).toEqual(["b", "c", "a"]);
    expect(collectionMoveItem(["a", "b", "c"], "c", -5)).toEqual(["c", "a", "b"]);
    expect(collectionMoveItem(["a", "b", "c"], "a", 99)).toEqual(["b", "c", "a"]);
    expect(collectionMoveItem(["a", "b", "c"], "missing", 0)).toEqual(["a", "b", "c"]);
  });

  it("places a visible column next to its new visible neighbors while hidden columns keep their place", () => {
    const all = ["title", "a", "hidden", "b", "c"];
    const visible = ["title", "a", "b", "c"];
    expect(collectionPlaceVisibleItem(all, visible, "c", 1)).toEqual(["title", "c", "a", "hidden", "b"]);
    expect(collectionPlaceVisibleItem(all, visible, "a", 2)).toEqual(["title", "hidden", "b", "a", "c"]);
    expect(collectionPlaceVisibleItem(all, visible, "a", 3)).toEqual(["title", "hidden", "b", "c", "a"]);
  });

  it("leaves the order alone for hidden, unknown, or lone columns", () => {
    const all = ["title", "a", "hidden", "b"];
    expect(collectionPlaceVisibleItem(all, ["title", "a", "b"], "hidden", 0)).toEqual(all);
    expect(collectionPlaceVisibleItem(all, ["title", "a", "b"], "missing", 0)).toEqual(all);
    expect(collectionPlaceVisibleItem(all, ["a"], "a", 0)).toEqual(all);
  });

  it("arranges items by a preferred ID order and keeps the rest in their existing order", () => {
    const items = [{ id: "a" }, { id: "b" }, { id: "c" }, { id: "d" }];
    expect(collectionOrderById(items, ["c", "missing", "a", "c"]).map((item) => item.id)).toEqual(["c", "a", "b", "d"]);
    expect(collectionOrderById(items, []).map((item) => item.id)).toEqual(["a", "b", "c", "d"]);
  });
});

interface TestCell {
  id: string;
  /** Left edge in the header row's layout. */
  left: number;
  width: number;
  movable?: boolean;
  sticky?: boolean;
}

interface TestHeader {
  row: HTMLElement;
  cells: Map<string, HTMLElement>;
  scroller: HTMLElement;
  scrollTo: (left: number) => number;
  scrollLeft: () => number;
}

const sticky = new Set<Element>();
const scrollers = new Set<Element>();
let frames: FrameRequestCallback[] = [];

beforeEach(() => {
  frames = [];
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => frames.push(callback));
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  const computed = window.getComputedStyle.bind(window);
  vi.spyOn(window, "getComputedStyle").mockImplementation((element) => {
    if (!sticky.has(element) && !scrollers.has(element)) return computed(element);
    return { position: sticky.has(element) ? "sticky" : "static", overflowX: scrollers.has(element) ? "auto" : "visible", columnGap: "0px" } as CSSStyleDeclaration;
  });
});

afterEach(() => {
  sticky.clear();
  scrollers.clear();
  document.body.replaceChildren();
  document.documentElement.removeAttribute("data-collection-column-dragging");
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const SCROLLER_WIDTH = 300;

function rect(left: number, width: number): DOMRect {
  return new DOMRect(left, 0, width, 32);
}

/**
 * Lay out a header row inside a 300px scroller without a layout engine. Frozen cells stay stuck at their layout
 * place within the scroller while the rest of the row moves with the scroll offset.
 */
function header(cells: readonly TestCell[], options: { scroll?: number; reorder?: () => CollectionColumnReorder } = {}): TestHeader {
  let scrollLeft = options.scroll ?? 0;
  const scroller = document.createElement("div");
  const row = document.createElement("div");
  row.setAttribute("role", "row");
  scroller.append(row);
  document.body.append(scroller);
  scrollers.add(scroller);
  scroller.getBoundingClientRect = () => rect(0, SCROLLER_WIDTH);
  Object.defineProperty(scroller, "scrollLeft", { configurable: true, get: () => scrollLeft });
  const width = cells.reduce((sum, cell) => sum + cell.width, 0) + 40;
  row.getBoundingClientRect = () => rect(-scrollLeft, width);
  const map = new Map<string, HTMLElement>();
  for (const cell of cells) {
    const element = document.createElement("div");
    element.setAttribute(COLLECTION_COLUMN_ID_ATTRIBUTE, cell.id);
    if (cell.movable ?? true) element.setAttribute(COLLECTION_COLUMN_MOVABLE_ATTRIBUTE, "");
    if (cell.sticky) sticky.add(element);
    element.getBoundingClientRect = () => rect(cell.sticky ? cell.left : cell.left - scrollLeft, cell.width);
    row.append(element);
    map.set(cell.id, element);
  }
  const trailing = document.createElement("div");
  trailing.getBoundingClientRect = () => rect(width - 40 - scrollLeft, 40);
  row.append(trailing);
  return {
    row,
    cells: map,
    scroller,
    scrollLeft: () => scrollLeft,
    scrollTo: (left) => {
      scrollLeft = Math.max(0, Math.min(width - SCROLLER_WIDTH, left));
      return scrollLeft;
    },
  };
}

function controller(table: () => TestHeader, overrides: Partial<CollectionColumnReorderOptions> = {}) {
  const onReorder = vi.fn<CollectionColumnReorderOptions["onReorder"]>();
  const reorder = new CollectionColumnReorder({
    canStart: () => true,
    onReorder,
    scroller: () => ({ element: table().scroller, scrollTo: table().scrollTo }),
    ...overrides,
  });
  return { reorder, onReorder };
}

/** Dispatch a pointer event using jsdom's mouse event implementation. */
function pointer(target: EventTarget, type: string, x: number, init: { button?: number; pointerType?: string } = {}): void {
  const event = new MouseEvent(type, { clientX: x, button: init.button ?? 0, bubbles: true, cancelable: true });
  Object.defineProperty(event, "pointerId", { value: 1 });
  Object.defineProperty(event, "pointerType", { value: init.pointerType ?? "mouse" });
  target.dispatchEvent(event);
}

/** Wire every cell's pointerdown to the controller, as the column header component does. */
function press(reorder: CollectionColumnReorder, table: TestHeader, id: string, x: number, init: { button?: number; pointerType?: string } = {}): void {
  const cell = table.cells.get(id)!;
  const listener = (event: Event) => reorder.pointerDown(event as PointerEvent, "header", id);
  cell.addEventListener("pointerdown", listener, { once: true });
  pointer(cell, "pointerdown", x, init);
  cell.removeEventListener("pointerdown", listener);
}

function runFrames(): void {
  const pending = frames;
  frames = [];
  pending.forEach((frame) => frame(0));
}

const COLUMNS: TestCell[] = [
  { id: "title", left: 0, width: 100, movable: false },
  { id: "a", left: 100, width: 100 },
  { id: "b", left: 200, width: 100 },
  { id: "c", left: 300, width: 100 },
  { id: "d", left: 400, width: 100 },
];

describe("collection column reorder controller", () => {
  it("waits for a sideways threshold, previews the new order, and reorders only on release", () => {
    let table!: TestHeader;
    const { reorder, onReorder } = controller(() => table);
    table = header(COLUMNS);
    press(reorder, table, "a", 150);
    pointer(window, "pointermove", 153);
    expect(reorder.preview).toBeNull();
    pointer(window, "pointermove", 240);
    expect(reorder.preview).toMatchObject({ columnId: "a", translate: 90, unstick: false });
    expect(reorder.cellStyle("header", "b")).toBe("transform: translateX(-100px);");
    expect(reorder.cellStyle("header", "c")).toBe("");
    expect(reorder.dragged("header", "a")).toBe(true);
    expect(reorder.shifting("header", "b")).toBe(true);
    expect(reorder.shifting("other", "b")).toBe(false);
    expect(document.documentElement.hasAttribute("data-collection-column-dragging")).toBe(true);
    expect(onReorder).not.toHaveBeenCalled();
    pointer(window, "pointerup", 240);
    expect(onReorder).toHaveBeenCalledExactlyOnceWith("a", 2);
    expect(reorder.preview).toBeNull();
    expect(document.documentElement.hasAttribute("data-collection-column-dragging")).toBe(false);
  });

  it("swallows the click that ends a drag but not later clicks", async () => {
    let table!: TestHeader;
    const { reorder } = controller(() => table);
    table = header(COLUMNS);
    const click = vi.fn();
    table.cells.get("a")!.addEventListener("click", click);
    press(reorder, table, "a", 150);
    pointer(window, "pointermove", 240);
    pointer(window, "pointerup", 240);
    table.cells.get("a")!.click();
    expect(click).not.toHaveBeenCalled();
    await new Promise((resolve) => setTimeout(resolve, 0));
    table.cells.get("a")!.click();
    expect(click).toHaveBeenCalledOnce();
  });

  it("leaves a plain press as a click", () => {
    let table!: TestHeader;
    const { reorder, onReorder } = controller(() => table);
    table = header(COLUMNS);
    const click = vi.fn();
    table.cells.get("a")!.addEventListener("click", click);
    press(reorder, table, "a", 150);
    pointer(window, "pointerup", 151);
    table.cells.get("a")!.click();
    expect(click).toHaveBeenCalledOnce();
    expect(onReorder).not.toHaveBeenCalled();
  });

  it("cancels with Escape while still swallowing the release click", () => {
    let table!: TestHeader;
    const { reorder, onReorder } = controller(() => table);
    table = header(COLUMNS);
    const click = vi.fn();
    table.cells.get("a")!.addEventListener("click", click);
    press(reorder, table, "a", 150);
    pointer(window, "pointermove", 340);
    const escape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    window.dispatchEvent(escape);
    expect(escape.defaultPrevented).toBe(true);
    expect(reorder.preview).toBeNull();
    pointer(window, "pointermove", 400);
    expect(reorder.preview).toBeNull();
    pointer(window, "pointerup", 400);
    table.cells.get("a")!.click();
    expect(onReorder).not.toHaveBeenCalled();
    expect(click).not.toHaveBeenCalled();
  });

  it("cancels when the window loses focus", () => {
    let table!: TestHeader;
    const { reorder, onReorder } = controller(() => table);
    table = header(COLUMNS);
    press(reorder, table, "a", 150);
    pointer(window, "pointermove", 340);
    window.dispatchEvent(new Event("blur"));
    pointer(window, "pointerup", 340);
    expect(reorder.preview).toBeNull();
    expect(onReorder).not.toHaveBeenCalled();
  });

  it("ignores touch, secondary buttons, fixed columns, and presses it may not start", () => {
    let table!: TestHeader;
    let allowed = true;
    const { reorder, onReorder } = controller(() => table, { canStart: () => allowed });
    table = header(COLUMNS);
    press(reorder, table, "a", 150, { pointerType: "touch" });
    pointer(window, "pointermove", 340);
    press(reorder, table, "a", 150, { button: 2 });
    pointer(window, "pointermove", 340);
    press(reorder, table, "title", 50);
    pointer(window, "pointermove", 240);
    pointer(window, "pointerup", 240);
    expect(reorder.preview).toBeNull();
    press(reorder, table, "a", 150);
    allowed = false;
    pointer(window, "pointermove", 340);
    expect(reorder.preview).toBeNull();
    pointer(window, "pointerup", 340);
    expect(onReorder).not.toHaveBeenCalled();
  });

  it("does not start from a control that keeps its own pointer behavior", () => {
    let table!: TestHeader;
    const { reorder } = controller(() => table);
    table = header(COLUMNS);
    const handle = document.createElement("button");
    handle.setAttribute("data-collection-resize", "");
    table.cells.get("a")!.append(handle);
    const listener = (event: Event) => reorder.pointerDown(event as PointerEvent, "header", "a");
    table.cells.get("a")!.addEventListener("pointerdown", listener);
    pointer(handle, "pointerdown", 195);
    pointer(window, "pointermove", 300);
    expect(reorder.preview).toBeNull();
  });

  it("unsticks a frozen column dragged while the table is not scrolled", () => {
    let table!: TestHeader;
    const { reorder, onReorder } = controller(() => table);
    table = header(COLUMNS.map((cell) => ({ ...cell, sticky: cell.left < 200 })));
    press(reorder, table, "a", 150);
    pointer(window, "pointermove", 360);
    expect(reorder.preview).toMatchObject({ unstick: true, translate: 210 });
    expect(reorder.cellStyle("header", "a")).toContain("position: relative; left: auto;");
    pointer(window, "pointerup", 360);
    expect(onReorder).toHaveBeenCalledExactlyOnceWith("a", 3);
  });

  it("reorders a stuck frozen column only among the frozen columns while the table is scrolled", () => {
    let table!: TestHeader;
    const { reorder, onReorder } = controller(() => table);
    table = header(COLUMNS.map((cell) => ({ ...cell, sticky: cell.left < 300 })), { scroll: 150 });
    press(reorder, table, "a", 150);
    pointer(window, "pointermove", 1000);
    expect(reorder.preview).toMatchObject({ unstick: false, translate: 100 });
    expect(reorder.cellStyle("header", "a")).not.toContain("position");
    expect(reorder.cellStyle("header", "c")).toBe("");
    runFrames();
    expect(table.scrollLeft()).toBe(150);
    pointer(window, "pointerup", 1000);
    expect(onReorder).toHaveBeenCalledExactlyOnceWith("a", 2);
  });

  it("auto-scrolls near the scroller edge and keeps the dragged header under the pointer", () => {
    let table!: TestHeader;
    const { reorder, onReorder } = controller(() => table);
    table = header(COLUMNS);
    press(reorder, table, "b", 250);
    pointer(window, "pointermove", 295);
    expect(reorder.preview?.translate).toBe(45);
    runFrames();
    expect(table.scrollLeft()).toBe(11);
    expect(reorder.preview?.translate).toBe(56);
    for (let frame = 0; frame < 40 && frames.length > 0; frame += 1) runFrames();
    expect(frames).toHaveLength(0);
    expect(table.scrollLeft()).toBe(240);
    expect(reorder.preview?.translate).toBe(200);
    pointer(window, "pointerup", 295);
    expect(onReorder).toHaveBeenCalledExactlyOnceWith("b", 4);
  });

  it("does not drag when fewer than two columns can move", () => {
    let table!: TestHeader;
    const { reorder } = controller(() => table);
    table = header(COLUMNS.slice(0, 2));
    press(reorder, table, "a", 150);
    pointer(window, "pointermove", 300);
    expect(reorder.preview).toBeNull();
    expect(document.documentElement.hasAttribute("data-collection-column-dragging")).toBe(false);
  });

  it("drops the preview and listeners when destroyed mid-drag", () => {
    let table!: TestHeader;
    const { reorder, onReorder } = controller(() => table);
    table = header(COLUMNS);
    press(reorder, table, "a", 150);
    pointer(window, "pointermove", 340);
    reorder.destroy();
    pointer(window, "pointerup", 340);
    expect(reorder.preview).toBeNull();
    expect(onReorder).not.toHaveBeenCalled();
    expect(document.documentElement.hasAttribute("data-collection-column-dragging")).toBe(false);
  });
});
