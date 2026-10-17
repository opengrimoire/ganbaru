// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ProjectListScrollbars from "./ProjectListScrollbars.svelte";

let component: ReturnType<typeof mount> | undefined;
let observed = new Set<Element>();
let resize: () => void;

beforeEach(() => {
  observed = new Set();
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { resize = callback; }
    observe(element: Element): void { observed.add(element); }
    disconnect(): void { observed.clear(); }
  });
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(400);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(200);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("Project list scrollbar geometry", () => {
  it("updates overflow without a scroll gesture and observes replacement groups", async () => {
    const scrollContainer = document.createElement("div");
    const content = document.createElement("div");
    const group = document.createElement("section");
    content.append(group);
    scrollContainer.append(content);
    document.body.append(scrollContainer);
    const host = document.createElement("div");
    document.body.append(host);
    let range = 0;
    const sync = vi.fn((left: number) => {
      scrollContainer.scrollLeft = Math.min(left, range);
      return scrollContainer.scrollLeft;
    });
    component = mount(ProjectListScrollbars, { target: host, props: {
      scrollContainer, getMaxScrollLeft: () => range, onScrollPositionChange: sync,
    } });
    await tick();
    const track = host.querySelector<HTMLElement>("[data-app-tooltip]")!;
    expect(track.childElementCount).toBe(0);
    expect(observed.has(group)).toBe(true);
    range = 400;
    resize();
    await tick();
    expect((track.firstElementChild as HTMLElement).style.width).toBe("200px");
    range = 800;
    resize();
    await tick();
    expect(parseFloat((track.firstElementChild as HTMLElement).style.width)).toBeCloseTo(400 / 3);
    scrollContainer.scrollLeft = 600;
    range = 0;
    resize();
    await tick();
    expect(track.childElementCount).toBe(0);
    expect(scrollContainer.scrollLeft).toBe(0);
    const replacement = document.createElement("section");
    content.replaceChildren(replacement);
    await tick();
    expect(observed.has(group)).toBe(false);
    expect(observed.has(replacement)).toBe(true);
  });
});
