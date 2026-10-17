// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import type { NotesNavigationDragCardPhase } from "$lib/notes/navigation/drag-motion";
import NotesNavigationDragCard from "./NotesNavigationDragCard.svelte";

describe("NotesNavigationDragCard", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
  });

  function render(phase: NotesNavigationDragCardPhase, options: { hint?: string | null; forbidden?: boolean } = {}): HTMLElement | null {
    target = document.createElement("div");
    document.body.appendChild(target);
    component = mount(NotesNavigationDragCard, {
      target,
      props: {
        kind: "page",
        title: "Weekly plan",
        frame: { left: 12, top: 40, width: 220, height: 32 },
        phase,
        hint: options.hint ?? null,
        forbidden: options.forbidden ?? false,
        durationMs: 200,
      },
    });
    flushSync();
    return document.body.querySelector<HTMLElement>(".notes-navigation-drag-card");
  }

  it("lifts outside the sidebar at the row's frame and names the destination", () => {
    const card = render("lifted", { hint: "Move to Archive" });
    expect(card?.parentElement).toBe(document.body);
    expect(card?.getAttribute("aria-hidden")).toBe("true");
    expect(card?.style.translate).toBe("12px 40px");
    expect(card?.style.width).toBe("220px");
    expect(card?.textContent).toContain("Weekly plan");
    const hint = card?.querySelector(".notes-navigation-drag-hint");
    expect(hint?.textContent?.trim()).toBe("Move to Archive");
    expect(hint?.classList.contains("opacity-0")).toBe(false);
  });

  it("fades over a destination it can never take", () => {
    expect(render("lifted", { forbidden: true })?.classList.contains("opacity-60")).toBe(true);
  });

  it("drops its lift and hint while settling into a row", () => {
    const card = render("settling", { hint: "Move to Archive" });
    expect(card?.dataset.phase).toBe("settling");
    expect(card?.classList.contains("shadow-xl")).toBe(false);
    expect(card?.querySelector(".notes-navigation-drag-hint")?.classList.contains("opacity-0")).toBe(true);
  });
});
