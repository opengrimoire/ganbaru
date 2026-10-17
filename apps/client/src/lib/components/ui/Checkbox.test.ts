// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import Checkbox from "./Checkbox.svelte";

describe("Checkbox", () => {
  let target: HTMLElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  /** Mount the checkbox inside a wrapping label, as labeled rows do. */
  async function mountLabeled(props: Record<string, unknown>): Promise<{ input: HTMLInputElement; text: HTMLSpanElement }> {
    target = document.createElement("label");
    const text = document.createElement("span");
    text.textContent = "Show resolved";
    document.body.append(target);
    component = mount(Checkbox, { target, props });
    target.append(text);
    await tick();
    const input = target.querySelector<HTMLInputElement>('input[type="checkbox"]');
    if (!input) throw new Error("Missing checkbox input");
    return { input, text };
  }

  it.each([
    { checked: false, clickTarget: "box" },
    { checked: false, clickTarget: "label" },
    { checked: true, clickTarget: "box" },
    { checked: true, clickTarget: "label" },
  ] as const)("reports one change from the $clickTarget and keeps the parent's value when checked is $checked", async ({ checked, clickTarget }) => {
    const onChange = vi.fn();
    const { input, text } = await mountLabeled({ checked, onChange });

    (clickTarget === "box" ? input : text).click();
    await tick();

    expect(onChange).toHaveBeenCalledOnce();
    expect(onChange).toHaveBeenCalledWith(!checked);
    expect(input.checked).toBe(checked);
  });

  it("toggles its own value when no change handler owns it", async () => {
    const { input } = await mountLabeled({ checked: false });

    input.click();
    await tick();

    expect(input.checked).toBe(true);
    expect(input.closest("span")?.querySelector("svg")).not.toBeNull();
  });

  it("exposes the mixed state natively and ignores clicks while disabled", async () => {
    const onChange = vi.fn();
    const { input } = await mountLabeled({ checked: false, indeterminate: true, disabled: true, label: "Project", onChange });

    expect(input.indeterminate).toBe(true);
    expect(input.getAttribute("aria-label")).toBe("Project");
    input.click();
    await tick();

    expect(onChange).not.toHaveBeenCalled();
  });
});
