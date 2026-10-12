// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Loading from "./Loading.svelte";

describe("Loading", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.useRealTimers();
  });

  function render(props: Record<string, unknown> = {}): HTMLElement {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(Loading, { target, props });
    flushSync();
    const status = target.querySelector<HTMLElement>("[role='status']");
    if (!status) throw new Error("Missing loading status");
    return status;
  }

  it("announces what is loading without showing visible text", () => {
    const status = render({ label: "Loading projects" });

    expect(status.textContent).toBe("Loading projects");
    expect(status.querySelector(".sr-only")?.textContent).toBe("Loading projects");
    expect(status.getAttribute("aria-busy")).toBe("true");
  });

  it("falls back to the generic loading label", () => {
    expect(render().textContent).toBe("Loading");
  });

  it("stays invisible until the reveal delay passes so fast loads never flash", async () => {
    const status = render();

    expect(status.hasAttribute("data-revealed")).toBe(false);
    await vi.advanceTimersByTimeAsync(299);
    flushSync();
    expect(status.hasAttribute("data-revealed")).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    flushSync();
    expect(status.hasAttribute("data-revealed")).toBe(true);
  });

  it("reveals immediately when the delay is zero", () => {
    expect(render({ delay: 0 }).hasAttribute("data-revealed")).toBe(true);
  });

  it("sketches the requested number of rows for lists", () => {
    const status = render({ layout: "rows", rows: 4 });

    expect(status.querySelectorAll(".loading-row")).toHaveLength(4);
    expect(status.querySelector(".loading-spinner")).toBeNull();
  });

  it("keeps at least one row when asked for none", () => {
    expect(render({ layout: "rows", rows: 0 }).querySelectorAll(".loading-row")).toHaveLength(1);
  });

  it("shows a spinner for fill and inline layouts", () => {
    expect(render({ layout: "inline" }).querySelector(".loading-spinner")).not.toBeNull();
  });

  it("renders inline loading as a span so it fits inside phrasing content", () => {
    expect(render({ layout: "inline" }).tagName).toBe("SPAN");
  });
});
