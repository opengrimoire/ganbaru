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

  it("uses touch targets and closes its dialog after pointer selection", async () => {
    target = document.createElement("div");
    document.body.append(target);
    const onSelect = vi.fn();
    component = mount(EventColorPicker, {
      target,
      props: {
        color: 2,
        theme: lightTheme,
        mobileLayout: true,
        onSelect,
      },
    });

    const trigger = target.querySelector<HTMLButtonElement>("button");
    expect(trigger?.classList.contains("size-12")).toBe(true);
    trigger?.click();
    await tick();

    const dialog = document.body.querySelector<HTMLElement>("[role='dialog']");
    const swatch = dialog?.querySelector<HTMLButtonElement>("[data-color-index='3']");
    expect(dialog?.style.left).toContain("--visual-viewport-offset-left");
    expect(swatch?.classList.contains("min-h-12")).toBe(true);
    expect(swatch?.classList.contains("min-w-12")).toBe(true);

    swatch?.click();
    await tick();
    expect(onSelect).toHaveBeenCalledWith(3);
    expect(document.body.querySelector("[role='dialog']")).toBeNull();
  });
});
