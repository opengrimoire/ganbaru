// @vitest-environment jsdom
import { createRawSnippet, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import Select from "$lib/components/ui/Select.svelte";
import CollectionMenu from "./CollectionMenu.svelte";

let component: ReturnType<typeof mount> | undefined;
let nested: ReturnType<typeof mount> | undefined;
const submenus: ReturnType<typeof mount>[] = [];

afterEach(async () => {
  if (nested) await unmount(nested);
  if (component) await unmount(component);
  component = undefined;
  nested = undefined;
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** Create a clipped collection inside a page preview's floating boundary. */
function host() {
  const dialog = document.createElement("div");
  dialog.dataset.floatingRoot = "";
  const row = document.createElement("div");
  row.style.overflow = "hidden";
  dialog.append(row);
  document.body.append(dialog);
  return { dialog, row };
}

/** Wait for the panel to position itself and focus its first field. */
async function open(row: HTMLElement): Promise<HTMLButtonElement> {
  const trigger = row.querySelector<HTMLButtonElement>("button")!;
  trigger.click();
  await tick();
  await tick();
  return trigger;
}

describe("Shared collection menus", () => {
  it.each([
    { borderHeight: 2, top: "84px" },
    { borderHeight: 2.5, top: "83px" },
  ])("positions fractional content including $borderHeight px of borders and constrains longer menus", async ({ borderHeight, top }) => {
    vi.stubGlobal("innerHeight", 230);
    let contentHeight = 80.25;
    const borderStyle = document.createElement("div").style;
    borderStyle.borderTopWidth = `${borderHeight / 2}px`;
    borderStyle.borderBottomWidth = `${borderHeight / 2}px`;
    const getComputedStyle = window.getComputedStyle.bind(window);
    vi.spyOn(window, "getComputedStyle").mockImplementation((element) => element.classList.contains("collection-panel")
      ? borderStyle : getComputedStyle(element));
    const { dialog, row } = host();
    const children = createRawSnippet(() => ({ render: () => '<div><button type="button">Duplicate</button></div>' }));
    component = mount(CollectionMenu, { target: row, props: { label: "Actions", kind: "actions", children } });
    const trigger = row.querySelector<HTMLButtonElement>("button")!;
    vi.spyOn(trigger, "getBoundingClientRect").mockReturnValue(new DOMRect(40, 170, 32, 28));
    await open(row);
    const panel = dialog.querySelector<HTMLElement>('[role="dialog"]')!;
    vi.spyOn(panel.querySelector<HTMLElement>("[data-collection-menu-content]")!, "getBoundingClientRect")
      .mockImplementation(() => new DOMRect(0, 0, 240, contentHeight));
    window.dispatchEvent(new Event("resize"));
    expect(panel.style.maxHeight).toBe("158px");
    expect(panel.style.top).toBe(top);

    contentHeight = 1_000;
    window.dispatchEvent(new Event("resize"));
    expect(panel.style.maxHeight).toBe("158px");
    expect(panel.style.top).toBe("8px");
    expect(panel.querySelector("button")?.textContent).toBe("Duplicate");
  });

  it("measures at the final width and keeps repeated opens and resize notifications stable", async () => {
    vi.stubGlobal("innerHeight", 230);
    const callbacks: (() => void)[] = [];
    vi.stubGlobal("ResizeObserver", class implements ResizeObserver {
      constructor(callback: ResizeObserverCallback) { callbacks.push(() => callback([], this)); }
      observe = vi.fn<(target: Element) => void>();
      unobserve = vi.fn<(target: Element) => void>();
      disconnect = vi.fn<() => void>();
    });
    const measuredWidths: string[] = [];
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.hasAttribute("data-collection-menu-content")) {
        const width = this.closest<HTMLElement>('[role="dialog"]')?.style.width ?? "";
        measuredWidths.push(width);
        return new DOMRect(0, 0, 240, width === "240px" ? 80.25 : 400);
      }
      return new DOMRect(40, 170, 32, 28);
    });
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(400);
    const { dialog, row } = host();
    const children = createRawSnippet(() => ({ render: () => '<div><button type="button">Calculate</button></div>' }));
    component = mount(CollectionMenu, { target: row, props: { label: "Name", kind: "property", children } });
    const trigger = await open(row);
    const first = dialog.querySelector<HTMLElement>('[role="dialog"]')!;
    const initialStyle = first.style.cssText;
    const mutations = new MutationObserver(() => {});
    mutations.observe(first, { attributes: true, attributeFilter: ["style"] });
    for (let index = 0; index < 5; index += 1) callbacks[0]?.();
    expect(first.style.top).toBe("86px");
    expect(first.style.maxHeight).toBe("158px");
    expect(first.style.cssText).toBe(initialStyle);
    expect(mutations.takeRecords()).toHaveLength(0);
    mutations.disconnect();

    trigger.click();
    await tick();
    await open(row);
    const second = dialog.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(second.style.cssText).toBe(initialStyle);
    expect(measuredWidths.length).toBeGreaterThan(1);
    expect(measuredWidths.every((width) => width === "240px")).toBe(true);
  });

  it("escapes clipped content, focuses the first field and dismisses without stealing outside focus", async () => {
    const { dialog, row } = host();
    const children = createRawSnippet(() => ({ render: () => '<div><input aria-label="Filter value" /></div>' }));
    component = mount(CollectionMenu, { target: row, props: { label: "Filter", kind: "filter", children } });
    const trigger = await open(row);
    const panel = dialog.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(panel.parentElement).toBe(dialog);
    expect(row.querySelector('[role="dialog"]')).toBeNull();
    expect(panel.style.position).toBe("fixed");
    expect(document.activeElement).toBe(panel.querySelector("input"));

    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await tick();
    expect(dialog.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);

    await open(row);
    const outside = document.createElement("button");
    document.body.append(outside);
    outside.focus();
    await tick();
    expect(dialog.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(outside);
  });

  it("keeps nested dropdown selection inside the panel and consumes Escape one level at a time", async () => {
    const { dialog, row } = host();
    const onChange = vi.fn();
    const children = createRawSnippet(() => ({
      render: () => "<div></div>",
      setup: (element) => {
        nested = mount(Select, { target: element, props: {
          inline: true, value: "todo", ariaLabel: "Status", onChange,
          options: [{ value: "todo", label: "To do" }, { value: "done", label: "Done" }],
        } });
      },
    }));
    component = mount(CollectionMenu, { target: row, props: { label: "Layout", children } });
    const trigger = await open(row);
    const panel = dialog.querySelector<HTMLElement>('[role="dialog"]')!;
    const select = panel.querySelector<HTMLButtonElement>('[aria-haspopup="listbox"]')!;
    select.click();
    await vi.waitFor(() => expect(document.activeElement?.getAttribute("role")).toBe("option"));
    const listbox = panel.querySelector('[role="listbox"]')!;
    expect(listbox.parentElement).toBe(panel);
    const escape = () => document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    escape();
    await tick();
    expect(panel.isConnected).toBe(true);
    expect(panel.querySelector('[role="listbox"]')).toBeNull();
    expect(document.activeElement).toBe(select);

    select.click();
    await vi.waitFor(() => expect(document.activeElement?.getAttribute("role")).toBe("option"));
    panel.querySelector<HTMLButtonElement>('[role="option"][aria-selected="false"]')?.click();
    await tick();
    expect(onChange).toHaveBeenCalledWith("done");
    expect(panel.isConnected).toBe(true);
    escape();
    await tick();
    expect(panel.isConnected).toBe(false);
    expect(document.activeElement).toBe(trigger);
  });

  it("commits a scalar field before outside background dismissal without stealing focus", async () => {
    const { dialog, row } = host();
    const commit = vi.fn();
    const connectedAtCommit: boolean[] = [];
    const children = createRawSnippet(() => ({
      render: () => '<div><input aria-label="Filter value" /></div>',
      setup: (element) => {
        const input = element.querySelector<HTMLInputElement>("input")!;
        input.addEventListener("blur", () => {
          connectedAtCommit.push(input.isConnected);
          commit(input.value);
        });
      },
    }));
    component = mount(CollectionMenu, { target: row, props: { label: "Filter", kind: "filter", children } });
    const trigger = await open(row);
    const triggerFocus = vi.spyOn(trigger, "focus");
    const panel = dialog.querySelector<HTMLElement>('[role="dialog"]')!;
    panel.querySelector<HTMLInputElement>("input")!.value = "High";
    const background = document.createElement("div");
    document.body.append(background);
    background.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    background.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(commit).toHaveBeenCalledExactlyOnceWith("High");
    expect(connectedAtCommit).toEqual([true]);
    await tick();
    expect(panel.isConnected).toBe(false);
    expect(triggerFocus).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(document.body);
  });

  it("handles Escape after its last focused field becomes disabled and rejects unrelated Escape", async () => {
    const { dialog, row } = host();
    const children = createRawSnippet(() => ({ render: () => '<div><fieldset><input aria-label="Filter value" /></fieldset></div>' }));
    component = mount(CollectionMenu, { target: row, props: { label: "Filter", kind: "filter", children } });
    const trigger = await open(row);
    const panel = dialog.querySelector<HTMLElement>('[role="dialog"]')!;
    const input = panel.querySelector<HTMLInputElement>("input")!;
    input.blur();
    const enabledEscape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    document.body.dispatchEvent(enabledEscape);
    await tick();
    expect(enabledEscape.defaultPrevented).toBe(false);
    expect(panel.isConnected).toBe(true);

    input.focus();
    input.blur();
    panel.querySelector<HTMLFieldSetElement>("fieldset")!.disabled = true;
    expect(input.matches(":disabled")).toBe(true);
    const unrelated = document.createElement("input");
    document.body.append(unrelated);
    const unrelatedEscape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    unrelated.dispatchEvent(unrelatedEscape);
    expect(unrelatedEscape.defaultPrevented).toBe(false);
    expect(panel.isConnected).toBe(true);

    const disabledEscape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    document.body.dispatchEvent(disabledEscape);
    await tick();
    expect(disabledEscape.defaultPrevented).toBe(true);
    expect(panel.isConnected).toBe(false);
    expect(document.activeElement).toBe(trigger);
  });

  it("runs an action before dismissing and removes the floating panel when unmounted", async () => {
    const { dialog, row } = host();
    const action = vi.fn();
    const children = createRawSnippet(() => ({
      render: () => '<div><button type="button">Duplicate</button></div>',
      setup: (element) => { element.querySelector("button")?.addEventListener("click", action); },
    }));
    component = mount(CollectionMenu, { target: row, props: { label: "Actions", kind: "actions", children } });
    const trigger = await open(row);
    dialog.querySelector<HTMLButtonElement>("[data-collection-menu-body] button")?.click();
    await tick();
    await tick();
    expect(action).toHaveBeenCalledOnce();
    expect(dialog.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);

    await open(row);
    await unmount(component!);
    component = undefined;
    expect(dialog.querySelector('[role="dialog"]')).toBeNull();
  });

  it("lets a press only select through beforeOpen and opens from the context menu instead of the browser menu", async () => {
    const { dialog, row } = host();
    const children = createRawSnippet(() => ({ render: () => '<button type="button">Rename</button>' }));
    let active = false;
    const beforeOpen = vi.fn((source: "press" | "context") => {
      const wasActive = active;
      active = true;
      return wasActive || source === "context";
    });
    component = mount(CollectionMenu, { target: row, props: { label: "Board", kind: "actions", contextMenu: true, pressed: false, beforeOpen, children } });
    const trigger = row.querySelector<HTMLButtonElement>("button")!;
    expect(trigger.getAttribute("aria-pressed")).toBe("false");

    trigger.click();
    await tick();
    expect(beforeOpen).toHaveBeenLastCalledWith("press");
    expect(dialog.querySelector('[role="dialog"]')).toBeNull();

    trigger.click();
    await tick();
    expect(dialog.querySelector('[role="dialog"]')).not.toBeNull();
    trigger.click();
    await tick();
    expect(dialog.querySelector('[role="dialog"]')).toBeNull();

    active = false;
    const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    trigger.dispatchEvent(event);
    await tick();
    expect(event.defaultPrevented).toBe(true);
    expect(beforeOpen).toHaveBeenLastCalledWith("context");
    expect(dialog.querySelector('[role="dialog"]')).not.toBeNull();
  });

  it("leaves the browser context menu alone unless the menu opts in", async () => {
    const { dialog, row } = host();
    const children = createRawSnippet(() => ({ render: () => '<button type="button">Rename</button>' }));
    component = mount(CollectionMenu, { target: row, props: { label: "Actions", kind: "actions", children } });
    const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    row.querySelector("button")!.dispatchEvent(event);
    await tick();
    expect(event.defaultPrevented).toBe(false);
    expect(dialog.querySelector('[role="dialog"]')).toBeNull();
  });

  it("keeps the panel open when an action reveals an inline editor", async () => {
    const { dialog, row } = host();
    const children = createRawSnippet(() => ({
      render: () => '<button type="button" data-collection-menu-keep-open>Edit title</button>',
    }));
    component = mount(CollectionMenu, { target: row, props: { label: "View options", kind: "actions", children } });
    await open(row);
    dialog.querySelector<HTMLButtonElement>("[data-collection-menu-keep-open]")?.click();
    await tick();
    expect(dialog.querySelector('[role="dialog"]')).not.toBeNull();
  });

  it("dismisses immediate property actions while keeping nested filter actions inside their owner", async () => {
    const { dialog, row } = host();
    const move = vi.fn();
    const addFilter = vi.fn();
    const filterChildren = createRawSnippet(() => ({
      render: () => '<div><button type="button">Add filter</button></div>',
      setup: (element) => { element.querySelector("button")?.addEventListener("click", addFilter); },
    }));
    const children = createRawSnippet(() => ({
      render: () => '<div><button type="button">Move right</button><div data-filter-target></div></div>',
      setup: (element) => {
        element.querySelector("button")?.addEventListener("click", move);
        nested = mount(CollectionMenu, { target: element.querySelector("[data-filter-target]")!, props: {
          label: "Filter", kind: "filter", children: filterChildren,
        } });
      },
    }));
    component = mount(CollectionMenu, { target: row, props: {
      label: "Priority", kind: "property", dismissOnAction: true, children,
    } });
    const trigger = await open(row);
    const property = dialog.querySelector<HTMLElement>('[role="dialog"][aria-label="Priority"]')!;
    property.querySelector<HTMLButtonElement>('[aria-label="Filter"]')!.click();
    await vi.waitFor(() => expect(property.querySelector('[role="dialog"][aria-label="Filter"]')).not.toBeNull());
    const filter = property.querySelector<HTMLElement>('[role="dialog"][aria-label="Filter"]')!;
    filter.querySelector<HTMLButtonElement>("[data-collection-menu-body] button")!.click();
    await tick();
    expect(addFilter).toHaveBeenCalledOnce();
    expect(filter.isConnected).toBe(true);
    expect(property.isConnected).toBe(true);
    filter.focus();
    filter.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await tick();
    expect(filter.isConnected).toBe(false);
    property.querySelector<HTMLButtonElement>("[data-collection-menu-body] button")!.click();
    await tick();
    await tick();
    expect(move).toHaveBeenCalledOnce();
    expect(property.isConnected).toBe(false);
    expect(document.activeElement).toBe(trigger);
  });

  it("closes the owning property menu when a nested creator that dismisses on action completes", async () => {
    const { dialog, row } = host();
    const create = vi.fn();
    const creatorChildren = createRawSnippet(() => ({
      render: () => '<div><button type="button">Number</button></div>',
      setup: (element) => { element.querySelector("button")?.addEventListener("click", create); },
    }));
    const children = createRawSnippet(() => ({
      render: () => '<div data-insert-target></div>',
      setup: (element) => {
        nested = mount(CollectionMenu, { target: element, props: {
          label: "Insert right", kind: "new", showHeader: false, dismissOnAction: true, children: creatorChildren,
        } });
      },
    }));
    component = mount(CollectionMenu, { target: row, props: {
      label: "Priority", kind: "property", dismissOnAction: true, children,
    } });
    const trigger = await open(row);
    const property = dialog.querySelector<HTMLElement>('[role="dialog"][aria-label="Priority"]')!;
    property.querySelector<HTMLButtonElement>('[aria-label="Insert right"]')!.click();
    await vi.waitFor(() => expect(property.querySelector('[role="dialog"][aria-label="Insert right"]')).not.toBeNull());
    property.querySelector<HTMLElement>('[role="dialog"][aria-label="Insert right"]')!.querySelector<HTMLButtonElement>("[data-collection-menu-body] button")!.click();
    await tick();
    await tick();
    expect(create).toHaveBeenCalledOnce();
    expect(property.isConnected).toBe(false);
    expect(document.activeElement).toBe(trigger);
  });

  it("closes the New template picker after a template is chosen", async () => {
    const { dialog, row } = host();
    const applyTemplate = vi.fn();
    const children = createRawSnippet(() => ({
      render: () => '<div><button type="button">Weekly review</button></div>',
      setup: (element) => { element.querySelector("button")?.addEventListener("click", applyTemplate); },
    }));
    component = mount(CollectionMenu, { target: row, props: {
      label: "New page options", kind: "new-options", iconOnly: true, primary: true, showHeader: false,
      dismissOnAction: true, children,
    } });
    const trigger = await open(row);
    dialog.querySelector<HTMLButtonElement>("[data-collection-menu-body] button")?.click();
    await tick();
    expect(applyTemplate).toHaveBeenCalledOnce();
    expect(dialog.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });
});

/** Mount a column menu whose rows open the Edit property and Sort submenus. */
async function openWithSubmenus(): Promise<{ dialog: HTMLElement; parent: HTMLElement; rows: Record<"edit" | "sort", HTMLButtonElement> }> {
  const { dialog, row } = host();
  const submenuChildren = (action: string) => createRawSnippet(() => ({ render: () => `<div><button type="button">${action}</button></div>` }));
  const children = createRawSnippet(() => ({
    render: () => '<div><button type="button">Hide</button><div data-edit-target></div><div data-sort-target></div></div>',
    setup: (element) => {
      submenus.push(
        mount(CollectionMenu, { target: element.querySelector("[data-edit-target]")!, props: { label: "Edit property", kind: "properties", fullWidth: true, children: submenuChildren("Number") } }),
        mount(CollectionMenu, { target: element.querySelector("[data-sort-target]")!, props: { label: "Sort", kind: "sort", fullWidth: true, children: submenuChildren("Ascending") } }),
      );
    },
  }));
  component = mount(CollectionMenu, { target: row, props: { label: "Priority", kind: "property", children } });
  await open(row);
  const parent = dialog.querySelector<HTMLElement>('[role="dialog"][aria-label="Priority"]')!;
  return {
    dialog, parent,
    rows: {
      edit: parent.querySelector<HTMLButtonElement>('button[aria-label="Edit property"]')!,
      sort: parent.querySelector<HTMLButtonElement>('button[aria-label="Sort"]')!,
    },
  };
}

/** Rest the mouse on an element inside a menu. */
function hover(element: Element): void {
  element.dispatchEvent(new PointerEvent("pointerenter", { pointerType: "mouse" }));
  element.dispatchEvent(new PointerEvent("pointerover", { pointerType: "mouse", bubbles: true }));
}

function submenuPanel(parent: HTMLElement, label: string): HTMLElement | null {
  return parent.querySelector<HTMLElement>(`[role="dialog"][aria-label="${label}"]`);
}

describe("Collection submenus", () => {
  afterEach(async () => {
    for (const submenu of submenus.splice(0)) await unmount(submenu);
    vi.useRealTimers();
  });

  it("opens beside its row and narrower than its menu after the mouse rests on it without moving focus", async () => {
    const { parent, rows } = await openWithSubmenus();
    vi.useFakeTimers();
    vi.spyOn(rows.edit, "getBoundingClientRect").mockReturnValue(new DOMRect(106, 90, 228, 32));
    vi.spyOn(parent, "getBoundingClientRect").mockReturnValue(new DOMRect(100, 80, 240, 300));
    const focused = document.activeElement;
    hover(rows.edit);
    await vi.advanceTimersByTimeAsync(50);
    expect(submenuPanel(parent, "Edit property")).toBeNull();
    await vi.advanceTimersByTimeAsync(100);
    const panel = submenuPanel(parent, "Edit property")!;
    expect(panel).not.toBeNull();
    expect(panel.style.left).toBe("338px");
    expect(panel.style.width).toBe("208px");
    expect(rows.edit.getAttribute("aria-expanded")).toBe("true");
    expect(document.activeElement).toBe(focused);
    expect(panel.querySelector('button[aria-label="Close"]')).toBeNull();
  });

  it("keeps a submenu row highlighted across menu padding and moves the highlight to another hovered row", async () => {
    const { parent, rows } = await openWithSubmenus();
    vi.useFakeTimers();
    hover(rows.edit);
    await vi.advanceTimersByTimeAsync(150);
    const edit = submenuPanel(parent, "Edit property")!;
    expect(rows.edit.classList.contains("bg-accent")).toBe(true);

    hover(rows.edit.parentElement!);
    await vi.advanceTimersByTimeAsync(0);
    expect(rows.edit.classList.contains("bg-accent")).toBe(true);

    hover(parent.querySelector("button")!);
    await vi.advanceTimersByTimeAsync(0);
    expect(edit.isConnected).toBe(true);
    expect(rows.edit.classList.contains("bg-accent")).toBe(false);

    hover(edit.querySelector("button")!);
    await vi.advanceTimersByTimeAsync(0);
    expect(rows.edit.classList.contains("bg-accent")).toBe(true);
  });

  it("closes a hover-opened submenu when the mouse settles on another row and replaces it with a sibling", async () => {
    const { parent, rows } = await openWithSubmenus();
    vi.useFakeTimers();
    hover(rows.edit);
    await vi.advanceTimersByTimeAsync(150);
    const edit = submenuPanel(parent, "Edit property")!;
    hover(edit.querySelector("button")!);
    hover(parent.querySelector("button")!);
    await vi.advanceTimersByTimeAsync(100);
    hover(edit.querySelector("button")!);
    await vi.advanceTimersByTimeAsync(300);
    expect(edit.isConnected).toBe(true);

    hover(parent.querySelector("button")!);
    await vi.advanceTimersByTimeAsync(300);
    expect(edit.isConnected).toBe(false);

    hover(rows.edit);
    await vi.advanceTimersByTimeAsync(150);
    rows.sort.click();
    await vi.advanceTimersByTimeAsync(0);
    expect(submenuPanel(parent, "Edit property")).toBeNull();
    expect(submenuPanel(parent, "Sort")).not.toBeNull();
    expect(parent.isConnected).toBe(true);
  });

  it("moves focus into a hover-opened submenu when its row is clicked and keeps it open while focused", async () => {
    const { parent, rows } = await openWithSubmenus();
    vi.useFakeTimers();
    hover(rows.edit);
    await vi.advanceTimersByTimeAsync(150);
    rows.edit.click();
    await vi.advanceTimersByTimeAsync(0);
    const panel = submenuPanel(parent, "Edit property")!;
    expect(document.activeElement).toBe(panel.querySelector("button"));
    hover(parent.querySelector("button")!);
    await vi.advanceTimersByTimeAsync(300);
    expect(panel.isConnected).toBe(true);
  });

  it("enters with ArrowRight and returns to its row with ArrowLeft while the parent stays open", async () => {
    const { parent, rows } = await openWithSubmenus();
    rows.edit.focus();
    rows.edit.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    await tick();
    expect(document.activeElement).toBe(rows.sort);
    expect(submenuPanel(parent, "Edit property")).toBeNull();

    rows.edit.focus();
    rows.edit.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }));
    await tick();
    await tick();
    const panel = submenuPanel(parent, "Edit property")!;
    expect(document.activeElement).toBe(panel.querySelector("button"));
    document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true, cancelable: true }));
    await tick();
    expect(panel.isConnected).toBe(false);
    expect(parent.isConnected).toBe(true);
    expect(document.activeElement).toBe(rows.edit);
  });

  it("ignores touch hover so taps open submenus by click", async () => {
    const { parent, rows } = await openWithSubmenus();
    vi.useFakeTimers();
    rows.edit.dispatchEvent(new PointerEvent("pointerenter", { pointerType: "touch" }));
    await vi.advanceTimersByTimeAsync(300);
    expect(submenuPanel(parent, "Edit property")).toBeNull();
    rows.edit.click();
    await vi.advanceTimersByTimeAsync(0);
    expect(document.activeElement).toBe(submenuPanel(parent, "Edit property")?.querySelector("button"));
  });
});
