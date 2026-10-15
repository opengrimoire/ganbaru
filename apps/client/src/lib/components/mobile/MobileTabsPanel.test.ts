// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
import { createWorkspaceTab, kindLocation, type WorkspaceTab } from "$lib/workspace-tabs/model";
import MobileTabsPanel, { type MobileTabsPanelTabs } from "./MobileTabsPanel.svelte";

function fakeTabs(tabs: WorkspaceTab[]): MobileTabsPanelTabs {
  return {
    tabs,
    activeTabId: tabs[0]?.id ?? "",
    activate: vi.fn(),
    openKind: vi.fn(),
    close: vi.fn(),
    closeOthers: vi.fn(),
    setLocked: vi.fn(),
    move: vi.fn(),
  };
}

const calendarTab = createWorkspaceTab("calendar-tab", kindLocation("calendar"));
const roadmapTab = createWorkspaceTab(
  "roadmap-tab",
  { kind: "projects", projectId: "project-1", item: null },
  { level: "project", locked: true, title: "Roadmap" },
);
const chatTab = createWorkspaceTab("chat-tab", kindLocation("chat"));

describe("MobileTabsPanel", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    document.body.replaceChildren();
    component = null;
    target = null;
  });

  async function render(tabs: MobileTabsPanelTabs) {
    const onClose = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MobileTabsPanel, { target, props: { tabs, onClose } });
    await tick();
    return { onClose };
  }

  function row(id: string): HTMLElement {
    const element = document.querySelector<HTMLElement>(`[data-mobile-tab-row="${id}"]`);
    if (!element) throw new Error(`Missing row ${id}`);
    return element;
  }

  function menuItem(id: string, label: string): HTMLButtonElement {
    const item = [...row(id).querySelectorAll<HTMLButtonElement>("[role=menuitem]")]
      .find((button) => button.textContent?.trim() === label);
    if (!item) throw new Error(`Missing menu item ${label}`);
    return item;
  }

  function openMenu(id: string): void {
    row(id).querySelector<HTMLButtonElement>("[aria-haspopup=menu]")?.click();
    flushSync();
  }

  function press(element: Element): void {
    element.dispatchEvent(new Event("pointerdown", { bubbles: true }));
  }

  it("activates a tab and closes the panel", async () => {
    const tabs = fakeTabs([calendarTab, roadmapTab]);
    const { onClose } = await render(tabs);

    expect(row("calendar-tab").querySelector("[aria-current=page]")).not.toBeNull();
    expect(row("roadmap-tab").querySelector<HTMLButtonElement>("button")?.getAttribute("aria-label")).toBe("Roadmap (locked)");

    row("roadmap-tab").querySelector<HTMLButtonElement>("button")?.click();

    expect(tabs.activate).toHaveBeenCalledWith("roadmap-tab");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("is a panel over the content rather than a modal", async () => {
    await render(fakeTabs([calendarTab]));

    const panel = document.querySelector("[data-mobile-tabs-panel]");
    expect(panel?.getAttribute("role")).toBe("dialog");
    expect(panel?.hasAttribute("aria-modal")).toBe(false);
    expect(document.querySelector(".surface-backdrop")).toBeNull();
  });

  it("closes a tab in place but offers no close on the only tab", async () => {
    const tabs = fakeTabs([calendarTab, chatTab]);
    const { onClose } = await render(tabs);

    row("chat-tab").querySelector<HTMLButtonElement>("[data-mobile-tab-close]")?.click();
    expect(tabs.close).toHaveBeenCalledWith("chat-tab");
    expect(onClose).not.toHaveBeenCalled();

    await unmount(component!);
    component = null;
    document.body.replaceChildren();
    await render(fakeTabs([calendarTab]));

    expect(row("calendar-tab").querySelector("[data-mobile-tab-close]")).toBeNull();
    openMenu("calendar-tab");
    expect([...row("calendar-tab").querySelectorAll("[role=menuitem]")].map((item) => item.textContent?.trim()))
      .toEqual(["Move up", "Move down", "Lock tab"]);
  });

  it("moves a tab past its neighbor and keeps the ends disabled", async () => {
    const tabs = fakeTabs([calendarTab, roadmapTab, chatTab]);
    await render(tabs);

    openMenu("calendar-tab");
    expect(menuItem("calendar-tab", "Move up").getAttribute("aria-disabled")).toBe("true");
    menuItem("calendar-tab", "Move down").click();
    expect(tabs.move).toHaveBeenCalledWith("calendar-tab", "roadmap-tab", "after");

    openMenu("chat-tab");
    expect(menuItem("chat-tab", "Move down").getAttribute("aria-disabled")).toBe("true");
    menuItem("chat-tab", "Move up").click();
    expect(tabs.move).toHaveBeenLastCalledWith("chat-tab", "roadmap-tab", "before");
  });

  it("toggles a lock and closes the other tabs from a row menu", async () => {
    const tabs = fakeTabs([calendarTab, roadmapTab]);
    await render(tabs);

    openMenu("roadmap-tab");
    menuItem("roadmap-tab", "Unlock tab").click();
    expect(tabs.setLocked).toHaveBeenCalledWith("roadmap-tab", false);

    openMenu("roadmap-tab");
    menuItem("roadmap-tab", "Close other tabs").click();
    expect(tabs.closeOthers).toHaveBeenCalledWith("roadmap-tab");
  });

  it("lists only open tabs and keeps new tab kinds behind their own level", async () => {
    const tabs = fakeTabs([calendarTab]);
    const { onClose } = await render(tabs);

    expect(document.querySelector("[data-mobile-new-tab]")).toBeNull();
    expect(document.body.textContent).not.toContain("Recently closed");

    document.querySelector<HTMLButtonElement>("[data-mobile-new-tab-level]")?.click();
    flushSync();
    expect(document.querySelector("[data-mobile-tab-row]")).toBeNull();

    document.querySelector<HTMLButtonElement>("[data-mobile-new-tab=chat]")?.click();
    expect(tabs.openKind).toHaveBeenCalledWith("chat");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("returns from the new tab level with Back before closing", async () => {
    const { onClose } = await render(fakeTabs([calendarTab]));
    const backStack = getMobileBackStack();

    document.querySelector<HTMLButtonElement>("[data-mobile-new-tab-level]")?.click();
    flushSync();
    expect(backStack.consume()).toBe(true);
    flushSync();
    expect(document.querySelector("[data-mobile-tab-row]")).not.toBeNull();
    expect(onClose).not.toHaveBeenCalled();

    expect(backStack.consume()).toBe(true);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("closes on a press outside the panel but leaves the tabs button to toggle it", async () => {
    const { onClose } = await render(fakeTabs([calendarTab]));
    const tabsButton = document.createElement("button");
    tabsButton.dataset.mobileTabsButton = "";
    const icon = document.createElement("span");
    tabsButton.append(icon);
    document.body.append(tabsButton);

    press(row("calendar-tab"));
    press(icon);
    expect(onClose).not.toHaveBeenCalled();

    press(document.body);
    expect(onClose).toHaveBeenCalledOnce();
  });
});
