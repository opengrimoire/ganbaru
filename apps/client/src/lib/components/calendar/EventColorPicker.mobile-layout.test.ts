// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { lightTheme } from "$lib/themes";
import EventColorPicker from "./EventColorPicker.svelte";

describe("EventColorPicker mobile layout", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  async function openPicker(onSelect = vi.fn()): Promise<HTMLButtonElement> {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(EventColorPicker, {
      target,
      props: {
        color: 2,
        theme: lightTheme,
        mobileLayout: true,
        onSelect,
      },
    });
    const trigger = target.querySelector<HTMLButtonElement>("button")!;
    trigger.click();
    await tick();
    return trigger;
  }

  it("anchors a touch-sized palette beside its trigger and closes after selection", async () => {
    const onSelect = vi.fn();
    const trigger = await openPicker(onSelect);
    expect(trigger.classList.contains("size-12")).toBe(true);

    const dialog = document.body.querySelector<HTMLElement>("[role='dialog']");
    const swatch = dialog?.querySelector<HTMLButtonElement>("[data-color-index='3']");
    expect(dialog?.style.left).toMatch(/^\d+(\.\d+)?px$/);
    expect(dialog?.style.top).toMatch(/^\d+(\.\d+)?px$/);
    expect(swatch?.classList.contains("size-10")).toBe(true);

    swatch?.click();
    await tick();
    expect(onSelect).toHaveBeenCalledWith(3);
    expect(document.body.querySelector("[role='dialog']")).toBeNull();
  });

  it("keeps the backdrop through the press so a tap on the trigger area only closes", async () => {
    await openPicker();
    const backdrop = document.body.querySelector<HTMLElement>("[role='presentation']")!;

    backdrop.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await tick();
    expect(document.body.querySelector("[role='dialog']")).not.toBeNull();

    backdrop.click();
    await tick();
    expect(document.body.querySelector("[role='dialog']")).toBeNull();
    expect(document.body.querySelector("[role='presentation']")).toBeNull();
  });
});
