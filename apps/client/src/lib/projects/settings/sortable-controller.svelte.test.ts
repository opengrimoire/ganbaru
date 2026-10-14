// @vitest-environment jsdom

import { flushSync } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createProjectSettingsSortableController } from "./sortable-controller.svelte";

interface Entry {
  id: string;
  fieldId: string;
}

const ROW_HEIGHT = 28;
const ROW_GAP = 8;

let list: HTMLElement;
let entries: Entry[];
let cleanups: (() => void)[];

/** Move an entry one step, the way the settings structure draft reorders its local rows. */
async function moveEntry(entry: Entry, direction: -1 | 1): Promise<void> {
  const index = entries.findIndex((candidate) => candidate.id === entry.id);
  const next = [...entries];
  const [moved] = next.splice(index, 1);
  if (moved) next.splice(index + direction, 0, moved);
  entries = next;
}

/** Render one item element per entry, attach it to the controller, and lay the items out in an evenly spaced column. */
function mountList(sortable: ReturnType<typeof createProjectSettingsSortableController<Entry>>): void {
  for (const entry of entries) {
    const item = document.createElement("div");
    item.dataset.testId = entry.id;
    const handle = document.createElement("button");
    handle.addEventListener("pointerdown", (event) => sortable.pointerDown(event, entry.id));
    item.append(handle);
    list.append(item);
    cleanups.push($effect.root(() => {
      $effect(() => sortable.item(entry.id)(item) ?? undefined);
    }));
  }
  flushSync();
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const index = [...list.children].indexOf(this);
    return DOMRect.fromRect({ x: 0, y: index < 0 ? 0 : index * (ROW_HEIGHT + ROW_GAP), width: 200, height: index < 0 ? 0 : ROW_HEIGHT });
  });
}

function item(id: string): HTMLElement {
  const element = list.querySelector<HTMLElement>(`[data-test-id="${id}"]`);
  if (!element) throw new Error(`No item ${id}`);
  return element;
}

function handle(id: string): HTMLButtonElement {
  const element = item(id).querySelector("button");
  if (!element) throw new Error(`No handle for ${id}`);
  return element;
}

function pointer(element: EventTarget, type: string, clientY: number): void {
  element.dispatchEvent(new PointerEvent(type, { bubbles: true, button: 0, pointerId: 1, pointerType: "mouse", clientX: 20, clientY }));
}

/** Let the settle timer fire and the stepwise draft moves run. */
async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(200);
  for (let step = 0; step < 10; step += 1) await Promise.resolve();
  flushSync();
}

function controller(overrides: Partial<Parameters<typeof createProjectSettingsSortableController<Entry>>[0]> = {}) {
  return createProjectSettingsSortableController<Entry>({
    getEntries: () => entries,
    moveEntry,
    setError: () => undefined,
    reorderFailedMessage: () => "reorder failed",
    saveFailedMessage: (message) => `save failed: ${message}`,
    ...overrides,
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  list = document.createElement("div");
  document.body.append(list);
  entries = [
    { id: "a", fieldId: "field-1" },
    { id: "b", fieldId: "field-1" },
    { id: "c", fieldId: "field-1" },
  ];
  cleanups = [];
});

afterEach(() => {
  for (const cleanup of cleanups) cleanup();
  list.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  document.documentElement.classList.remove("sortable-dragging");
});

describe("project settings sortable controller", () => {
  it("lifts the dragged row, slides the rows it passes, and commits the move once it settles", async () => {
    const sortable = controller();
    mountList(sortable);

    pointer(handle("a"), "pointerdown", 10);
    pointer(window, "pointermove", 60);
    flushSync();
    expect(item("a").dataset.sortablePhase).toBe("lifted");
    expect(item("a").style.translate).toBe("0 50px");
    expect(item("b").style.translate).toBe("0 -36px");
    expect(item("c").style.translate).toBe("");

    pointer(window, "pointermove", 200);
    flushSync();
    expect(item("a").style.translate).toBe("0 72px");
    expect(item("c").style.translate).toBe("0 -36px");

    pointer(window, "pointerup", 200);
    flushSync();
    expect(item("a").dataset.sortablePhase).toBe("settling");
    expect(entries.map((entry) => entry.id)).toEqual(["a", "b", "c"]);

    await settle();
    expect(entries.map((entry) => entry.id)).toEqual(["b", "c", "a"]);
    expect(sortable.pending).toBe(false);
    for (const id of ["a", "b", "c"]) {
      expect(item(id).style.translate).toBe("");
      expect(item(id).dataset.sortablePhase).toBeUndefined();
      expect(item(id).dataset.sortableAnimated).toBeUndefined();
    }
  });

  it("returns the row to its slot when Escape cancels, and ignores a press that never travels", async () => {
    const sortable = controller();
    mountList(sortable);

    pointer(handle("c"), "pointerdown", 80);
    pointer(window, "pointermove", 10);
    flushSync();
    expect(item("a").style.translate).toBe("0 36px");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    expect(item("a").style.translate).toBe("");
    expect(item("c").style.translate).toBe("");
    await settle();

    pointer(handle("b"), "pointerdown", 45);
    pointer(window, "pointermove", 47);
    pointer(window, "pointerup", 47);
    await settle();
    expect(item("b").dataset.sortablePhase).toBeUndefined();
    expect(entries.map((entry) => entry.id)).toEqual(["a", "b", "c"]);
  });

  it("keeps an option among the options of its own field", async () => {
    entries = [
      { id: "a", fieldId: "field-1" },
      { id: "b", fieldId: "field-1" },
      { id: "c", fieldId: "field-2" },
    ];
    const sortable = controller({
      getEntriesForEntry: (entry) => entries.filter((candidate) => candidate.fieldId === entry.fieldId),
    });
    mountList(sortable);

    pointer(handle("a"), "pointerdown", 10);
    pointer(window, "pointermove", 200);
    flushSync();
    expect(item("a").style.translate).toBe("0 36px");
    expect(item("c").style.translate).toBe("");
    pointer(window, "pointerup", 200);
    await settle();
    expect(entries.map((entry) => entry.id)).toEqual(["b", "a", "c"]);
  });

  it("reports a failed move and clears pending", async () => {
    let error: string | null = null;
    const sortable = controller({
      moveEntry: async () => { throw new Error("write failed"); },
      setError: (value) => { error = value; },
    });
    mountList(sortable);

    pointer(handle("a"), "pointerdown", 10);
    pointer(window, "pointermove", 60);
    pointer(window, "pointerup", 60);
    await settle();
    expect(error).toBe("save failed: write failed");
    expect(sortable.pending).toBe(false);
  });

  it("commits right away when reduced motion is preferred", async () => {
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: query.includes("reduce") }));
    const sortable = controller();
    mountList(sortable);

    pointer(handle("b"), "pointerdown", 45);
    pointer(window, "pointermove", 0);
    pointer(window, "pointerup", 0);
    for (let step = 0; step < 10; step += 1) await Promise.resolve();
    flushSync();
    expect(entries.map((entry) => entry.id)).toEqual(["b", "a", "c"]);
    expect(item("b").dataset.sortablePhase).toBeUndefined();
  });
});
