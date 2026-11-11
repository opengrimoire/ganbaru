// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import CalendarScrollbar from "./CalendarScrollbar.svelte";

/** Records which elements each ResizeObserver currently watches, since jsdom has no layout. */
class RecordingResizeObserver {
  static instances: RecordingResizeObserver[] = [];
  readonly observed = new Set<Element>();

  constructor() {
    RecordingResizeObserver.instances.push(this);
  }

  observe(target: Element): void {
    this.observed.add(target);
  }

  unobserve(target: Element): void {
    this.observed.delete(target);
  }

  disconnect(): void {
    this.observed.clear();
  }
}

function observedElements(): Set<Element> {
  const observed = new Set<Element>();
  for (const instance of RecordingResizeObserver.instances) {
    for (const element of instance.observed) observed.add(element);
  }
  return observed;
}

describe("CalendarScrollbar content observation", () => {
  let host: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    RecordingResizeObserver.instances = [];
    vi.stubGlobal("ResizeObserver", RecordingResizeObserver);
  });

  afterEach(async () => {
    if (component) await unmount(component);
    host?.remove();
    component = undefined;
    host = undefined;
    vi.unstubAllGlobals();
  });

  it("follows content that replaces the scroll container's children", async () => {
    host = document.createElement("div");
    document.body.append(host);
    const scrollContainer = document.createElement("section");
    const firstContent = document.createElement("div");
    scrollContainer.append(firstContent);
    host.append(scrollContainer);

    component = mount(CalendarScrollbar, { target: host, props: { scrollContainer } });
    flushSync();
    expect(observedElements().has(firstContent)).toBe(true);

    const nextContent = document.createElement("div");
    firstContent.replaceWith(nextContent);
    await Promise.resolve();

    const observed = observedElements();
    expect(observed.has(scrollContainer)).toBe(true);
    expect(observed.has(nextContent)).toBe(true);
    expect(observed.has(firstContent)).toBe(false);
  });
});
