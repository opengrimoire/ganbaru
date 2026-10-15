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
  });

  it("keeps each row to switching and closing, without a row menu", async () => {
    await render(fakeTabs([calendarTab, roadmapTab, chatTab]));

    for (const id of ["calendar-tab", "roadmap-tab", "chat-tab"]) {
      expect(row(id).querySelectorAll("button")).toHaveLength(2);
      expect(row(id).querySelector("[aria-haspopup=menu]")).toBeNull();
    }
    expect(row("roadmap-tab").querySelector("[data-mobile-tab-close]")).not.toBeNull();
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
