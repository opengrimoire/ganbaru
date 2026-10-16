// @vitest-environment jsdom

import { flushSync } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSortableDrag, type SortableAxis } from "./sortable-drag.svelte";
import { TOUCH_LONG_PRESS_MS } from "./touch-hold";

const ITEM_SIZE = 60;
const CROSS_SIZE = 40;

let list: HTMLElement;
let commits: [string, number][];
let cleanups: (() => void)[];

/** Render one evenly spaced item per id along `axis`, each pressed directly, and return the drag. */
function mountList(ids: string[], axis: SortableAxis) {
  const drag = createSortableDrag({
    getIds: () => ids,
    commit: (id, targetIndex) => {
      commits.push([id, targetIndex]);
    },
    touchHold: true,
    axis,
  });
  for (const id of ids) {
    const item = document.createElement("button");
    item.dataset.testId = id;
    item.addEventListener("pointerdown", (event) => drag.pointerDown(event, id));
    list.append(item);
    cleanups.push($effect.root(() => {
      $effect(() => drag.item(id)(item) ?? undefined);
    }));
  }
  flushSync();
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const index = [...list.children].indexOf(this);
    const start = index < 0 ? 0 : index * ITEM_SIZE;
    const size = index < 0 ? ITEM_SIZE * ids.length : ITEM_SIZE;
    return axis === "x"
      ? DOMRect.fromRect({ x: start, y: 0, width: size, height: CROSS_SIZE })
      : DOMRect.fromRect({ x: 0, y: start, width: CROSS_SIZE, height: size });
  });
  return drag;
}

function item(id: string): HTMLElement {
  const element = list.querySelector<HTMLElement>(`[data-test-id="${id}"]`);
  if (!element) throw new Error(`No item for ${id}`);
  return element;
}

function pointer(target: EventTarget, type: string, clientX: number, clientY: number, pointerType = "mouse"): void {
  target.dispatchEvent(new PointerEvent(type, { bubbles: true, button: 0, pointerId: 1, pointerType, clientX, clientY }));
}

async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(200);
  flushSync();
}

beforeEach(() => {
  vi.useFakeTimers();
  list = document.createElement("div");
  document.body.append(list);
  commits = [];
  cleanups = [];
});

afterEach(() => {
  for (const cleanup of cleanups) cleanup();
  list.remove();
  vi.restoreAllMocks();
  vi.useRealTimers();
  document.documentElement.classList.remove("sortable-dragging");
});

describe("sortable drag along a row", () => {
  it("follows the pointer sideways, slides the passed items back, and commits the slot it lands in", async () => {
    mountList(["a", "b", "c", "d"], "x");

    pointer(item("a"), "pointerdown", 20, 20);
    pointer(window, "pointermove", 150, 20);
    flushSync();

    expect(item("a").dataset.sortablePhase).toBe("lifted");
    expect(item("a").style.translate).toBe("130px 0");
    expect(item("b").style.translate).toBe(`-${ITEM_SIZE}px 0`);
    expect(item("c").style.translate).toBe(`-${ITEM_SIZE}px 0`);
    expect(item("d").style.translate).toBe("");

    pointer(window, "pointerup", 150, 20);
    await settle();

    expect(commits).toEqual([["a", 2]]);
    expect(item("a").style.translate).toBe("");
  });

  it("ignores vertical movement in a row and lifts a touch only after a hold", async () => {
    mountList(["a", "b", "c"], "x");

    pointer(item("c"), "pointerdown", 140, 20, "touch");
    await vi.advanceTimersByTimeAsync(TOUCH_LONG_PRESS_MS);
    flushSync();
    expect(item("c").dataset.sortablePhase).toBe("lifted");

    pointer(window, "pointermove", 140, 400, "touch");
    flushSync();
    expect(item("c").style.translate).toBe("");

    pointer(window, "pointermove", 20, 400, "touch");
    pointer(window, "pointerup", 20, 400, "touch");
    await settle();

    expect(commits).toEqual([["c", 0]]);
  });

  it("moves a column vertically", async () => {
    mountList(["a", "b"], "y");

    pointer(item("a"), "pointerdown", 20, 20);
    pointer(window, "pointermove", 20, 90);
    flushSync();

    expect(item("a").style.translate).toBe("0 60px");
    pointer(window, "pointerup", 20, 90);
    await settle();

    expect(commits).toEqual([["a", 1]]);
  });
});
