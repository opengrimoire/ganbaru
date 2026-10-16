// @vitest-environment jsdom

import { flushSync } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSidebarOrderDrag, type SidebarOrderDragOptions } from "./sidebar-order-drag.svelte";
import { TOUCH_LONG_PRESS_MS } from "./touch-hold";

const ROW_HEIGHT = 28;

let list: HTMLElement;
let ids: string[];
let starred: Set<string>;
let enabled: boolean;
let saves: string[][];
let cleanups: (() => void)[];

function drag(overrides: Partial<SidebarOrderDragOptions> = {}) {
  return createSidebarOrderDrag({
    getIds: () => ids,
    enabled: () => enabled,
    isStarred: (id) => starred.has(id),
    save: async (orderedIds) => {
      saves.push(orderedIds);
    },
    description: "test items",
    ...overrides,
  });
}

/** Render one evenly spaced row per id, each pressed through its own handle. */
function mountList(sidebarDrag: ReturnType<typeof createSidebarOrderDrag>): void {
  for (const id of ids) {
    const row = document.createElement("div");
    row.dataset.testId = id;
    const handle = document.createElement("button");
    handle.addEventListener("pointerdown", (event) => sidebarDrag.pointerDown(event, id));
    row.append(handle);
    list.append(row);
    cleanups.push($effect.root(() => {
      $effect(() => sidebarDrag.item(id)(row) ?? undefined);
    }));
  }
  flushSync();
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const index = [...list.children].indexOf(this);
    return DOMRect.fromRect({ x: 0, y: index < 0 ? 0 : index * ROW_HEIGHT, width: 200, height: index < 0 ? 0 : ROW_HEIGHT });
  });
}

function handle(id: string): HTMLButtonElement {
  const element = list.querySelector<HTMLButtonElement>(`[data-test-id="${id}"] button`);
  if (!element) throw new Error(`No handle for ${id}`);
  return element;
}

function pointer(element: EventTarget, type: string, clientY: number, pointerType = "mouse"): void {
  element.dispatchEvent(new PointerEvent(type, { bubbles: true, button: 0, pointerId: 1, pointerType, clientX: 20, clientY }));
}

async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(200);
  for (let step = 0; step < 5; step += 1) await Promise.resolve();
  flushSync();
}

beforeEach(() => {
  vi.useFakeTimers();
  list = document.createElement("div");
  document.body.append(list);
  ids = ["a", "b", "c", "d"];
  starred = new Set();
  enabled = true;
  saves = [];
  cleanups = [];
});

afterEach(() => {
  for (const cleanup of cleanups) cleanup();
  list.remove();
  vi.restoreAllMocks();
  vi.useRealTimers();
  document.documentElement.classList.remove("sortable-dragging");
});

describe("sidebar order drag", () => {
  it("moves an item one step among the items that share its starred state", async () => {
    starred = new Set(["a", "c"]);
    const sidebarDrag = drag();

    expect(sidebarDrag.canMove("a", -1)).toBe(false);
    expect(sidebarDrag.canMove("a", 1)).toBe(true);
    expect(sidebarDrag.canMove("c", 1)).toBe(false);
    expect(sidebarDrag.canMove("b", -1)).toBe(false);
    expect(sidebarDrag.canMove("d", -1)).toBe(true);

    sidebarDrag.move("d", -1);
    await Promise.resolve();
    expect(saves).toEqual([["d", "b"]]);

    sidebarDrag.move("c", 1);
    await Promise.resolve();
    expect(saves).toHaveLength(1);
  });

  it("keeps each kind in its own block when the list mixes kinds", async () => {
    ids = ["folder:a", "folder:b", "page:c", "page:d"];
    starred = new Set(["page:c"]);
    const sidebarDrag = drag({ kindOf: (id) => id.split(":")[0] ?? "" });

    expect(sidebarDrag.canMove("folder:b", 1)).toBe(false);
    expect(sidebarDrag.canMove("page:d", -1)).toBe(false);
    expect(sidebarDrag.canMove("folder:b", -1)).toBe(true);

    sidebarDrag.move("folder:b", -1);
    await Promise.resolve();
    expect(saves).toEqual([["folder:b", "folder:a"]]);
  });

  it("moves nothing outside the custom order and waits for a save in flight", async () => {
    enabled = false;
    const resolvers: (() => void)[] = [];
    const sidebarDrag = drag({
      save: (orderedIds) => new Promise<void>((resolve) => {
        saves.push(orderedIds);
        resolvers.push(resolve);
      }),
    });

    sidebarDrag.move("a", 1);
    expect(saves).toEqual([]);

    enabled = true;
    sidebarDrag.move("a", 1);
    sidebarDrag.move("c", 1);
    expect(saves).toEqual([["b", "a", "c", "d"]]);

    resolvers[0]?.();
    await Promise.resolve();
    await Promise.resolve();
    sidebarDrag.move("c", 1);
    expect(saves).toEqual([["b", "a", "c", "d"], ["a", "b", "d", "c"]]);
  });

  it("logs a failed save and accepts the next move", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
    let fail = true;
    const sidebarDrag = drag({
      save: async (orderedIds) => {
        saves.push(orderedIds);
        if (fail) throw new Error("offline");
      },
    });

    sidebarDrag.move("a", 1);
    await Promise.resolve();
    await Promise.resolve();
    expect(error).toHaveBeenCalledWith("Failed to reorder test items", expect.any(Error));

    fail = false;
    sidebarDrag.move("b", 1);
    expect(saves).toHaveLength(2);
  });

  it("drags a mouse press within its starred partition and saves the order it lands in", async () => {
    starred = new Set(["a", "b"]);
    const sidebarDrag = drag();
    mountList(sidebarDrag);

    pointer(handle("a"), "pointerdown", 10);
    pointer(window, "pointermove", 200);
    flushSync();
    expect(sidebarDrag.draggedId).toBe("a");

    pointer(window, "pointerup", 200);
    await settle();
    expect(saves).toEqual([["b", "a"]]);
    expect(sidebarDrag.draggedId).toBeNull();
  });

  it("lifts a touch press after a stationary hold and drops it where it is released", async () => {
    const sidebarDrag = drag();
    mountList(sidebarDrag);

    pointer(handle("a"), "pointerdown", 10, "touch");
    await vi.advanceTimersByTimeAsync(TOUCH_LONG_PRESS_MS);
    flushSync();
    expect(sidebarDrag.draggedId).toBe("a");

    const touchMove = new Event("touchmove", { cancelable: true });
    window.dispatchEvent(touchMove);
    expect(touchMove.defaultPrevented).toBe(true);
    pointer(window, "pointermove", 40, "touch");
    pointer(window, "pointerup", 40, "touch");
    await settle();
    expect(saves).toEqual([["b", "a", "c", "d"]]);
  });

  it("leaves a touch that moves before the hold to scroll the list", async () => {
    const sidebarDrag = drag();
    mountList(sidebarDrag);

    pointer(handle("a"), "pointerdown", 10, "touch");
    pointer(window, "pointermove", 100, "touch");
    await vi.advanceTimersByTimeAsync(TOUCH_LONG_PRESS_MS);
    flushSync();
    expect(sidebarDrag.draggedId).toBeNull();
    pointer(window, "pointerup", 100, "touch");
    await settle();
    expect(saves).toEqual([]);
  });

  it("ignores presses outside the custom order", async () => {
    const sidebarDrag = drag();
    mountList(sidebarDrag);

    enabled = false;
    pointer(handle("a"), "pointerdown", 10);
    pointer(window, "pointermove", 100);
    flushSync();
    expect(sidebarDrag.draggedId).toBeNull();
    pointer(window, "pointerup", 100);
    await settle();
    expect(saves).toEqual([]);
  });
});
