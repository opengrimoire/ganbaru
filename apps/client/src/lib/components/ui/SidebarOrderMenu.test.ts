// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import SidebarOrderMenu from "./SidebarOrderMenu.svelte";

describe("SidebarOrderMenu", () => {
  let target: HTMLElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.restoreAllMocks();
  });

  async function open(props: Record<string, unknown>): Promise<HTMLButtonElement> {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(SidebarOrderMenu, { target, props: { label: "Sort", ...props } });
    await tick();
    const button = target.querySelector<HTMLButtonElement>("button[aria-haspopup='menu']");
    if (!button) throw new Error("Missing order button");
    button.click();
    flushSync();
    return button;
  }

  function items(): HTMLButtonElement[] {
    return Array.from(document.querySelectorAll<HTMLButtonElement>("[data-sidebar-order-menu] [role='menuitemradio']"));
  }

  function checkedLabel(): string | undefined {
    return items().find((item) => item.getAttribute("aria-checked") === "true")?.textContent?.trim();
  }

  it("checks the order in effect and reports the chosen one", async () => {
    const onSelect = vi.fn();
    const button = await open({ order: "created-desc", onSelect });
    expect(button.getAttribute("aria-expanded")).toBe("true");
    expect(items().map((item) => item.textContent?.trim())).toEqual([
      "Name (A to Z)", "Name (Z to A)", "Modified time (new to old)", "Modified time (old to new)",
      "Created time (new to old)", "Created time (old to new)", "Custom",
    ]);
    expect(checkedLabel()).toBe("Created time (new to old)");
    expect(document.querySelectorAll("[data-sidebar-order-menu] [role='separator']")).toHaveLength(3);
    items().at(-1)?.click();
    flushSync();
    expect(onSelect).toHaveBeenCalledWith("custom");
    expect(document.querySelector("[data-sidebar-order-menu]")).toBeNull();
  });

  it("offers the global setting first for a scope and reports it as null", async () => {
    const onSelect = vi.fn();
    await open({ order: "name-asc", override: null, allowGlobal: true, onSelect });
    expect(items()[0]?.textContent?.trim()).toBe("Use global setting");
    expect(checkedLabel()).toBe("Use global setting");
    items()[0]?.click();
    expect(onSelect).toHaveBeenCalledWith(null);
  });

  it("checks the scope's own order over the global one", async () => {
    await open({ order: "name-desc", override: "name-desc", allowGlobal: true, onSelect: vi.fn() });
    expect(checkedLabel()).toBe("Name (Z to A)");
  });

  it("closes on Escape without letting it reach the panel underneath, and on a press outside", async () => {
    const panelKeydown = vi.fn();
    window.addEventListener("keydown", panelKeydown);
    const button = await open({ order: "name-asc", onSelect: vi.fn() });
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    window.removeEventListener("keydown", panelKeydown);
    expect(panelKeydown).not.toHaveBeenCalled();
    expect(document.querySelector("[data-sidebar-order-menu]")).toBeNull();
    expect(document.activeElement).toBe(button);

    button.click();
    flushSync();
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    flushSync();
    expect(document.querySelector("[data-sidebar-order-menu]")).toBeNull();
  });

  it("opens above the button when only the space above fits the menu", async () => {
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(240);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      return this.getAttribute("aria-haspopup") === "menu"
        ? DOMRect.fromRect({ x: 20, y: window.innerHeight - 40, width: 24, height: 24 })
        : DOMRect.fromRect({ width: 160, height: 240 });
    });
    await open({ order: "name-asc", onSelect: vi.fn() });
    await tick();
    const menu = document.querySelector<HTMLElement>("[data-sidebar-order-menu]");
    expect(menu?.style.bottom).toBe("44px");
    expect(menu?.style.top).toBe("");
  });
});
