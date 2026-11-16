// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPanelPointerResize, type PanelPointerResizeOptions } from "./panel-pointer-resize";

let frames: Map<number, FrameRequestCallback>;
let nextFrame: number;
let separator: HTMLInputElement;

/** Runs the animation frames queued so far, leaving frames they queue for the next call. */
function runFrames(): void {
  const queued = [...frames.entries()];
  frames.clear();
  for (const [, callback] of queued) callback(0);
}

function pointer(type: string, clientX: number, clientY = 0, button = 0): void {
  separator.dispatchEvent(new PointerEvent(type, { bubbles: true, button, pointerId: 1, clientX, clientY }));
}

interface Recorder {
  sizes: number[];
  resizing: boolean[];
  collapsed: number;
  committed: number;
}

function mount(overrides: Partial<PanelPointerResizeOptions> = {}): Recorder {
  const recorder: Recorder = { sizes: [], resizing: [], collapsed: 0, committed: 0 };
  const resize = createPanelPointerResize({
    axis: "x",
    currentSize: () => 400,
    sizeFromPointer: (requested) => (requested < 100 ? 0 : Math.min(600, requested)),
    applySize: (size) => { recorder.sizes.push(size); },
    setResizing: (resizing) => { recorder.resizing.push(resizing); },
    onCollapse: () => { recorder.collapsed += 1; },
    onCommit: () => { recorder.committed += 1; },
    ...overrides,
  });
  separator.addEventListener("pointerdown", resize.begin);
  return recorder;
}

beforeEach(() => {
  frames = new Map();
  nextFrame = 1;
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    const id = nextFrame++;
    frames.set(id, callback);
    return id;
  });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => { frames.delete(id); });
  separator = document.createElement("input");
  separator.setPointerCapture = vi.fn();
  document.body.append(separator);
});

afterEach(() => {
  separator.remove();
  vi.restoreAllMocks();
});

describe("panel pointer resize", () => {
  it("grows a side column as the pointer moves toward its start edge and commits once per changed drag", () => {
    const recorder = mount();
    pointer("pointerdown", 500);
    pointer("pointermove", 450);
    pointer("pointermove", 420);
    runFrames();
    expect(recorder.sizes).toEqual([480]);
    pointer("pointerup", 420);
    expect(recorder.sizes).toEqual([480, 480]);
    expect(recorder.committed).toBe(1);
    expect(recorder.resizing).toEqual([true]);
    runFrames();
    runFrames();
    expect(recorder.resizing).toEqual([true, false]);
  });

  it("follows the vertical axis for a bottom panel", () => {
    const recorder = mount({ axis: "y" });
    pointer("pointerdown", 0, 300);
    pointer("pointermove", 0, 250);
    pointer("pointerup", 0, 250);
    expect(recorder.sizes).toEqual([450]);
  });

  it("restores the start size on cancel and closes the panel from the collapse zone", () => {
    const cancelled = mount();
    pointer("pointerdown", 500);
    pointer("pointermove", 300);
    pointer("pointercancel", 300);
    expect(cancelled.sizes.at(-1)).toBe(400);
    expect(cancelled.committed).toBe(0);
    expect(cancelled.collapsed).toBe(0);

    separator.remove();
    separator = document.createElement("input");
    separator.setPointerCapture = vi.fn();
    document.body.append(separator);
    const collapsed = mount();
    pointer("pointerdown", 500);
    pointer("pointermove", 900);
    pointer("pointerup", 900);
    expect(collapsed.sizes.at(-1)).toBe(400);
    expect(collapsed.collapsed).toBe(1);
    expect(collapsed.committed).toBe(0);
  });

  it("ignores secondary buttons and does not commit an unchanged drag", () => {
    const recorder = mount();
    pointer("pointerdown", 500, 0, 2);
    expect(recorder.resizing).toEqual([]);
    pointer("pointerdown", 500);
    pointer("pointerup", 500);
    expect(recorder.committed).toBe(0);
  });

  it("cancels pending frames on dispose", () => {
    const recorder: Recorder = { sizes: [], resizing: [], collapsed: 0, committed: 0 };
    const resize = createPanelPointerResize({
      axis: "x",
      currentSize: () => 400,
      sizeFromPointer: (requested) => requested,
      applySize: (size) => { recorder.sizes.push(size); },
      setResizing: (resizing) => { recorder.resizing.push(resizing); },
      onCollapse: () => undefined,
    });
    separator.addEventListener("pointerdown", resize.begin);
    pointer("pointerdown", 500);
    pointer("pointermove", 450);
    resize.dispose();
    runFrames();
    expect(recorder.sizes).toEqual([]);
  });
});
