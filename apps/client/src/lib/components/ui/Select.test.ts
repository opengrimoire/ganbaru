// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { activateModalFocus } from "$lib/modal-focus";
import Select from "./Select.svelte";

describe("Select floating interaction", () => {
  let root: HTMLDivElement;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    root?.remove();
    component = undefined;
  });

  /** Create a dialog with a scrollable form and an explicit floating root. */
  function createHost(): HTMLFormElement {
    root = document.createElement("div");
    root.dataset.floatingRoot = "";
    root.setAttribute("role", "dialog");
    const form = document.createElement("form");
    root.append(form);
    document.body.append(root);
    return form;
  }

  it("keeps menus outside the form flow but inside the dialog and consumes Escape", async () => {
    const form = createHost();
    const onChange = vi.fn();
    const parentKeydown = vi.fn();
    root.addEventListener("keydown", parentKeydown);
    component = mount(Select, {
      target: form,
      props: {
        inline: true, ariaLabel: "Status", value: "todo", onChange,
        options: [{ value: "todo", label: "To do" }, { value: "done", label: "Done" }],
      },
    });
    const trigger = form.querySelector<HTMLButtonElement>("button");
    trigger?.click();
    await tick();
    const menu = root.querySelector('[role="listbox"]');
    expect(menu?.parentElement).toBe(root);
    expect(form.querySelector('[role="listbox"]')).toBeNull();
    await vi.waitFor(() => expect(document.activeElement).toBe(menu?.querySelector('[aria-selected="true"]')));

    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(menu?.querySelector('[aria-selected="false"]'));
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await tick();
    expect(root.querySelector('[role="listbox"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(parentKeydown).not.toHaveBeenCalled();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("filters in the menu and selects the matching option with Enter", async () => {
    const form = createHost();
    const onChange = vi.fn();
    component = mount(Select, {
      target: form,
      props: {
        inline: true, ariaLabel: "Parent task", value: "", onChange,
        triggerLabel: "Choose a task", searchPlaceholder: "Search tasks", emptyLabel: "No tasks",
        options: [{ value: "one", label: "Write notes" }, { value: "two", label: "Review design" }],
      },
    });
    form.querySelector<HTMLButtonElement>("button")?.click();
    await tick();
    const search = root.querySelector<HTMLInputElement>('[aria-label="Search tasks"]');
    if (!search) throw new Error("Missing menu search field");
    await vi.waitFor(() => expect(document.activeElement).toBe(search));
    search.value = "design";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    const options = root.querySelectorAll('[role="option"]');
    expect(options).toHaveLength(1);
    expect(options[0].textContent).toContain("Review design");
    search.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await tick();
    expect(onChange).toHaveBeenCalledWith("two");
    expect(root.querySelector('[role="listbox"]')).toBeNull();
  });

  it("forwards searches to the caller before filtering bounded result sets", async () => {
    const form = createHost();
    const onSearchChange = vi.fn();
    component = mount(Select, {
      target: form,
      props: {
        inline: true, value: "", onChange: vi.fn(), ariaLabel: "Tags",
        searchPlaceholder: "Find a tag", onSearchChange,
        options: [{ value: "cached", label: "Initial result" }],
      },
    });
    form.querySelector<HTMLButtonElement>("button")?.click();
    await tick();
    const search = root.querySelector<HTMLInputElement>("input");
    if (!search) throw new Error("Missing search field");
    search.value = "another tag";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(onSearchChange).toHaveBeenCalledWith("another tag");
    expect(root.querySelectorAll('[role="option"]')).toHaveLength(1);
  });

  it("skips unavailable options when opening and navigating a selected unavailable value", async () => {
    const form = createHost();
    const onChange = vi.fn();
    component = mount(Select, {
      target: form,
      props: {
        inline: true, ariaLabel: "Provider", value: "unavailable", onChange,
        options: [
          { value: "automatic", label: "Automatic" },
          { value: "unavailable", label: "Unconfigured", disabled: true },
          { value: "ready", label: "Ready" },
          { value: "last", label: "Unavailable", disabled: true },
        ],
      },
    });
    form.querySelector<HTMLButtonElement>("button")?.click();
    await vi.waitFor(() => expect(document.activeElement?.textContent).toContain("Automatic"));
    const unavailable = root.querySelector<HTMLButtonElement>('[aria-selected="true"]')!;
    expect(unavailable.disabled).toBe(true);
    unavailable.click();
    expect(onChange).not.toHaveBeenCalled();
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    expect(document.activeElement?.textContent).toContain("Ready");
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    expect(document.activeElement?.textContent).toContain("Automatic");
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true, cancelable: true }));
    expect(document.activeElement?.textContent).toContain("Ready");
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true, cancelable: true }));
    expect(document.activeElement?.textContent).toContain("Automatic");
  });

  it("does not select an unavailable search result with Enter", async () => {
    const form = createHost();
    const onChange = vi.fn();
    component = mount(Select, {
      target: form,
      props: {
        inline: true, ariaLabel: "Provider", value: "automatic", onChange,
        searchPlaceholder: "Search providers",
        options: [
          { value: "automatic", label: "Automatic" },
          { value: "unavailable", label: "Unconfigured", disabled: true },
        ],
      },
    });
    form.querySelector<HTMLButtonElement>("button")?.click();
    await vi.waitFor(() => expect(document.activeElement?.tagName).toBe("INPUT"));
    const search = root.querySelector<HTMLInputElement>("input")!;
    search.value = "Unconfigured";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    search.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    expect(onChange).not.toHaveBeenCalled();
    expect(root.querySelector('[role="listbox"]')).not.toBeNull();
  });
  it("preserves grid metadata and label association while opening from the keyboard", async () => {
    const form = createHost();
    const onfocus = vi.fn();
    const onkeydown = vi.fn((event: KeyboardEvent) => event.stopPropagation());
    component = mount(Select, {
      target: form,
      props: {
        inline: true, value: "todo", onChange: vi.fn(), ariaLabel: "Status",
        options: [{ value: "todo", label: "To do" }, { value: "done", label: "Done" }],
        triggerProps: { id: "status-cell", "data-table-cell": "true", "data-row-index": 2, "data-column-index": 3, onfocus, onkeydown },
      },
    });
    const trigger = form.querySelector<HTMLButtonElement>('[data-table-cell="true"][data-row-index="2"][data-column-index="3"]')!;
    expect(trigger.id).toBe("status-cell");
    trigger.focus();
    expect(onfocus).toHaveBeenCalledOnce();
    const arrow = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
    trigger.dispatchEvent(arrow);
    expect(arrow.defaultPrevented).toBe(true);
    expect(onkeydown).toHaveBeenCalledOnce();
    await vi.waitFor(() => expect(document.activeElement?.getAttribute("role")).toBe("option"));
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    expect(document.activeElement?.textContent).toContain("Done");
    expect(onkeydown).toHaveBeenCalledOnce();
  });

  it("closes on Tab before the surrounding modal computes its focus boundary", async () => {
    const form = createHost();
    component = mount(Select, {
      target: form,
      props: { inline: true, value: "todo", onChange: vi.fn(), options: [{ value: "todo", label: "To do" }] },
    });
    const releaseFocus = activateModalFocus(root);
    try {
      const trigger = form.querySelector<HTMLButtonElement>("button");
      trigger?.click();
      await vi.waitFor(() => expect(document.activeElement?.getAttribute("role")).toBe("option"));
      const tab = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
      document.activeElement?.dispatchEvent(tab);
      expect(root.querySelector('[role="listbox"]')).toBeNull();
      expect(document.activeElement).toBe(trigger);
      expect(tab.defaultPrevented).toBe(true);
    } finally {
      releaseFocus();
    }
  });

});
