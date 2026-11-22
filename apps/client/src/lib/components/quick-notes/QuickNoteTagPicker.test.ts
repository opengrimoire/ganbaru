// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { EventColor } from "$lib/calendar/types";
import type { QuickNoteTag } from "$lib/quick-notes/types";
import { lightTheme } from "$lib/themes";
import QuickNoteTagPicker from "./QuickNoteTagPicker.svelte";

const existingTag: QuickNoteTag = {
  id: "work",
  name: "Work",
  color: 2,
  orderKey: "a",
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
};

class NoopResizeObserver {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

describe("QuickNoteTagPicker", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", NoopResizeObserver);
  });

  afterEach(async () => {
    vi.unstubAllGlobals();
    if (component) await unmount(component);
    target?.remove();
    target = undefined;
    component = undefined;
    document.body.innerHTML = "";
  });

  function mountPicker(props: {
    onSelect: (tagId: string | null) => void;
    onCreate: (name: string, color: EventColor) => Promise<QuickNoteTag>;
  }): HTMLDivElement {
    const host = document.createElement("div");
    document.body.append(host);
    target = host;
    component = mount(QuickNoteTagPicker, {
      target: host,
      props: { tagId: null, tags: [existingTag], theme: lightTheme, defaultColor: 0, ...props },
    });
    flushSync();
    return host;
  }

  function buttonWithText(text: string): HTMLButtonElement | undefined {
    return [...document.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent.includes(text));
  }

  async function startCreating(host: HTMLElement, name: string): Promise<void> {
    host.querySelector<HTMLButtonElement>("button")?.click();
    await vi.waitFor(() => expect(buttonWithText("New tag")).toBeDefined());
    buttonWithText("New tag")?.click();
    await vi.waitFor(() => expect(document.querySelector("form input")).not.toBeNull());
    const input = document.querySelector<HTMLInputElement>("form input");
    if (!input) throw new Error("Missing tag name input");
    input.value = name;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    document.querySelector("form")?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    flushSync();
  }

  it("assigns a created tag while the picker stays open", async () => {
    const onSelect = vi.fn();
    const created: QuickNoteTag = { ...existingTag, id: "home", name: "Home" };
    const host = mountPicker({ onSelect, onCreate: vi.fn(async () => created) });

    await startCreating(host, "Home");

    await vi.waitFor(() => expect(onSelect).toHaveBeenCalledWith("home"));
    expect(onSelect).toHaveBeenCalledOnce();
  });

  it("does not override a tag chosen while the new tag was still saving", async () => {
    const onSelect = vi.fn();
    let resolveCreate: (tag: QuickNoteTag) => void = () => {};
    const onCreate = vi.fn(() => new Promise<QuickNoteTag>((resolve) => { resolveCreate = resolve; }));
    const host = mountPicker({ onSelect, onCreate });

    await startCreating(host, "Home");
    expect(onCreate).toHaveBeenCalledOnce();
    buttonWithText("Work")?.click();
    expect(onSelect).toHaveBeenCalledWith("work");

    resolveCreate({ ...existingTag, id: "home", name: "Home" });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(onSelect).toHaveBeenCalledOnce();
  });

  it("leaves a created tag unassigned when the picker was dismissed while saving", async () => {
    const onSelect = vi.fn();
    let resolveCreate: (tag: QuickNoteTag) => void = () => {};
    const onCreate = vi.fn(() => new Promise<QuickNoteTag>((resolve) => { resolveCreate = resolve; }));
    const host = mountPicker({ onSelect, onCreate });

    await startCreating(host, "Home");
    document.querySelector<HTMLDivElement>("div.fixed.inset-0")?.click();
    flushSync();
    expect(document.querySelector("[role='dialog']")).toBeNull();

    resolveCreate({ ...existingTag, id: "home", name: "Home" });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(onSelect).not.toHaveBeenCalled();
  });
});
