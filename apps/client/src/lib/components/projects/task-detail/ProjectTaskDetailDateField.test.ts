// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ProjectTaskDetailDateField from "./ProjectTaskDetailDateField.svelte";

describe("task date field floating calendar", () => {
  let root: HTMLDivElement;
  let form: HTMLFormElement;
  let component: ReturnType<typeof mount> | undefined;
  const onCancel = vi.fn();
  const onSelect = vi.fn();

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", class {
      observe(): void {}
      disconnect(): void {}
    });
    root = document.createElement("div");
    root.dataset.floatingRoot = "";
    form = document.createElement("form");
    root.append(form);
    document.body.append(root);
  });

  afterEach(async () => {
    if (component) await unmount(component);
    root.remove();
    component = undefined;
    vi.clearAllMocks();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  /** Mount an open date field with an existing day selected. */
  async function openCalendar(): Promise<void> {
    component = mount(ProjectTaskDetailDateField, {
      target: form,
      props: {
        label: "Due date", value: "2026-09-22", noDateLabel: "No date", clearLabel: "Clear due date",
        pickerOpen: true, selectedDate: "2026-09-22", onSelect, onCancel,
        onToggle: vi.fn(), onClear: vi.fn(),
      },
    });
    await tick();
  }

  it("floats outside the form so calendar navigation cannot submit or resize the task form", async () => {
    const onSubmit = vi.fn((event: SubmitEvent) => event.preventDefault());
    form.addEventListener("submit", onSubmit);
    await openCalendar();
    const popup = root.querySelector<HTMLElement>('[data-app-floating-surface]');
    expect(popup?.parentElement).toBe(root);
    expect(form.querySelector('[data-app-floating-surface]')).toBeNull();
    expect(popup?.classList.contains("fixed")).toBe(true);
    popup?.querySelector<HTMLButtonElement>('[data-date="2026-09-23"]')?.click();
    expect(onSelect).toHaveBeenCalledWith("2026-09-23");
    expect(onSubmit).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(form.querySelector("button"));
  });

  it("dismisses the calendar with Escape without closing the surrounding task dialog", async () => {
    const parentKeydown = vi.fn();
    root.addEventListener("keydown", parentKeydown);
    await openCalendar();
    const popup = root.querySelector<HTMLElement>('[data-app-floating-surface]');
    popup?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(onCancel).toHaveBeenCalledOnce();
    expect(parentKeydown).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(form.querySelector("button"));
  });

  it("ignores clicks in the calendar and dismisses on an outside pointer press", async () => {
    await openCalendar();
    const popup = root.querySelector<HTMLElement>('[data-app-floating-surface]');
    popup?.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(onCancel).not.toHaveBeenCalled();
    form.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(onCancel).toHaveBeenCalledOnce();
  });
  it("positions the calendar above a low field instead of pushing the editor down", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      return this.hasAttribute("data-app-floating-surface")
        ? new DOMRect(0, 0, 288, 300)
        : new DOMRect(700, 650, 120, 32);
    });
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(300);
    await openCalendar();
    const popup = root.querySelector<HTMLElement>("[data-app-floating-surface]");
    expect(popup?.style.top).toBe("344px");
    expect(popup?.style.left).toBe("532px");
  });

});
