// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import NotesLoadingSkeletonHarness from "./NotesLoadingSkeletonHarness.test.svelte";

let component: ReturnType<typeof mount> | undefined;

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
  vi.useRealTimers();
});

/** Mount actual loading state while keeping readiness controlled by a sibling trigger. */
async function mountLoadingSkeleton(initiallyReady = false, kind: "page" | "database" | "cover" | "block" = "database") {
  vi.useFakeTimers();
  const onAction = vi.fn();
  component = mount(NotesLoadingSkeletonHarness, { target: document.body, props: { initiallyReady, kind, onAction } });
  await tick();
  const toggle = document.querySelector<HTMLButtonElement>("[data-toggle-loading]")!;
  const content = document.querySelector<HTMLButtonElement>("[data-ready-content]")!;
  return { toggle, content, onAction };
}

describe("Notes loading handoff", () => {
  it("reveals the note title and body skeleton only after half a second", async () => {
    await mountLoadingSkeleton(false, "page");
    await vi.advanceTimersByTimeAsync(499);
    await tick();
    expect(document.querySelector<HTMLElement>(".notes-skeleton-shapes")?.style.opacity).toBe("0");
    await vi.advanceTimersByTimeAsync(1);
    await tick();
    expect(document.querySelector<HTMLElement>(".notes-skeleton-shapes")?.style.opacity).toBe("1");
  });

  it("shows a note ready before half a second without a skeleton or fade", async () => {
    const { toggle, content, onAction } = await mountLoadingSkeleton(false, "page");
    await vi.advanceTimersByTimeAsync(499);
    toggle.click();
    await tick();
    expect(document.querySelector(".notes-skeleton-placeholder")).toBeNull();
    expect(content.closest(".notes-skeleton-content")?.getAttribute("data-handoff")).toBe("false");
    content.click();
    expect(onAction).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(1);
    await tick();
    expect(document.querySelector(".notes-skeleton-placeholder")).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });

  it.each(["database", "cover", "block"] as const)("keeps the existing reveal delay for %s content", async (kind) => {
    await mountLoadingSkeleton(false, kind);
    await vi.advanceTimersByTimeAsync(119);
    await tick();
    expect(document.querySelector<HTMLElement>(".notes-skeleton-shapes")?.style.opacity).toBe("0");
    await vi.advanceTimersByTimeAsync(1);
    await tick();
    expect(document.querySelector<HTMLElement>(".notes-skeleton-shapes")?.style.opacity).toBe("1");
  });

  it("skips the placeholder and fade for already ready content", async () => {
    const { content, onAction } = await mountLoadingSkeleton(true);
    expect(document.querySelector(".notes-skeleton-placeholder")).toBeNull();
    expect(content.closest(".notes-skeleton-content")?.getAttribute("data-handoff")).toBe("false");
    content.click();
    expect(onAction).toHaveBeenCalledOnce();
  });

  it("shows fast results immediately without waiting for the reveal timer", async () => {
    const { toggle, content, onAction } = await mountLoadingSkeleton();
    expect(document.querySelector<HTMLElement>(".notes-skeleton-shapes")?.style.opacity).toBe("0");
    toggle.click();
    await tick();
    expect(document.querySelector(".notes-skeleton-placeholder")).toBeNull();
    expect(content.closest<HTMLElement>(".notes-skeleton-content")?.inert).toBe(false);
    expect(content.closest(".notes-skeleton-content")?.getAttribute("data-handoff")).toBe("false");
    content.click();
    expect(onAction).toHaveBeenCalledOnce();
  });

  it("retires a visible skeleton without blocking ready content or keeping a second layout row", async () => {
    const { toggle, content, onAction } = await mountLoadingSkeleton();
    await vi.advanceTimersToNextTimerAsync();
    await tick();
    expect(document.querySelector<HTMLElement>(".notes-skeleton-shapes")?.style.opacity).toBe("1");
    toggle.click();
    await tick();
    expect(document.querySelector("[data-notes-skeleton]")).toBeNull();
    expect(document.querySelector(".notes-skeleton-placeholder")?.getAttribute("data-retiring")).toBe("true");
    expect(document.querySelector<HTMLElement>(".notes-skeleton-shapes")?.style.opacity).toBe("0");
    expect(content.closest<HTMLElement>(".notes-skeleton-content")?.inert).toBe(false);
    content.click();
    expect(onAction).toHaveBeenCalledOnce();
    await vi.advanceTimersToNextTimerAsync();
    await tick();
    expect(document.querySelector(".notes-skeleton-placeholder")).toBeNull();
  });

  it("cancels an earlier handoff when loading restarts and clears timers on disposal", async () => {
    const { toggle, content } = await mountLoadingSkeleton();
    await vi.advanceTimersToNextTimerAsync();
    toggle.click();
    await tick();
    toggle.click();
    await tick();
    expect(content.closest<HTMLElement>(".notes-skeleton-content")?.inert).toBe(true);
    await vi.advanceTimersToNextTimerAsync();
    await tick();
    expect(document.querySelector<HTMLElement>(".notes-skeleton-shapes")?.style.opacity).toBe("1");
    toggle.click();
    await tick();
    await unmount(component!);
    component = undefined;
    expect(vi.getTimerCount()).toBe(0);
  });
});
