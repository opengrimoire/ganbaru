// @vitest-environment jsdom

import { mount, tick, unmount, type ComponentProps } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createWorkspaceTab, kindLocation, type WorkspaceTab } from "$lib/workspace-tabs/model";
import MobileNavigation from "./MobileNavigation.svelte";

const tabs: WorkspaceTab[] = [
  createWorkspaceTab("calendar-tab", kindLocation("calendar")),
  createWorkspaceTab("projects-tab", kindLocation("projects")),
  createWorkspaceTab("notes-tab", kindLocation("notes")),
  createWorkspaceTab("roadmap-tab", { kind: "projects", projectId: "project-1", item: null }, { level: "project", locked: true, title: "Roadmap" }),
  createWorkspaceTab("chat-tab", kindLocation("chat")),
  createWorkspaceTab("journal-tab", kindLocation("notes")),
];

describe("MobileNavigation", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
  });

  async function render(props: Partial<ComponentProps<typeof MobileNavigation>> = {}) {
    const handlers = { onSelect: vi.fn(), onOpenTabs: vi.fn() };
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MobileNavigation, {
      target,
      props: { tabs, activeTabId: "projects-tab", presentation: "bottom", ...handlers, ...props },
    });
    await tick();
    const tabButtons = [...target.querySelectorAll<HTMLButtonElement>("button:not([data-mobile-tabs-button])")];
    const tabsButton = target.querySelector<HTMLButtonElement>("[data-mobile-tabs-button]");
    return { ...handlers, tabButtons, tabsButton };
  }

  it("shows the first four tabs, marks the active one, and selects a tab by id", async () => {
    const { tabButtons, onSelect } = await render();

    expect(tabButtons.map((button) => button.textContent?.trim())).toEqual(["Calendar", "Projects", "Notes", "Roadmap"]);
    expect(tabButtons.filter((button) => button.getAttribute("aria-current") === "page")).toEqual([tabButtons[1]]);
    expect(tabButtons[1]?.querySelector("[data-mobile-navigation-indicator]")).not.toBeNull();
    expect(target?.querySelectorAll("[data-mobile-navigation-indicator]")).toHaveLength(1);

    tabButtons[2]?.click();
    expect(onSelect).toHaveBeenCalledWith("notes-tab");
  });

  it("keeps an active tab past the bar in its last slot", async () => {
    const { tabButtons } = await render({ activeTabId: "journal-tab" });

    expect(tabButtons.map((button) => button.textContent?.trim())).toEqual(["Calendar", "Projects", "Notes", "Notes"]);
    expect(tabButtons[3]?.getAttribute("aria-current")).toBe("page");
  });

  it("labels locked tabs and marks them with a lock", async () => {
    const { tabButtons } = await render();

    expect(tabButtons[3]?.getAttribute("aria-label")).toBe("Roadmap (locked)");
    expect(tabButtons[3]?.querySelector("[data-mobile-tab-locked]")).not.toBeNull();
    expect(tabButtons[0]?.querySelector("[data-mobile-tab-locked]")).toBeNull();
  });

  it("opens the tabs sheet from a button that counts every open tab", async () => {
    const { tabsButton, onOpenTabs } = await render();

    expect(tabsButton?.getAttribute("aria-label")).toBe("Tabs, 6 open");
    expect(tabsButton?.querySelector("[data-mobile-tab-count]")?.textContent).toBe("6");
    expect(tabsButton?.getAttribute("aria-expanded")).toBe("false");

    tabsButton?.click();
    expect(onOpenTabs).toHaveBeenCalledOnce();
  });

  it("sizes the bottom bar to the tabs it shows plus the tabs button", async () => {
    await render({ tabs: tabs.slice(0, 2), activeTabId: "calendar-tab" });

    const nav = target?.querySelector<HTMLElement>("nav");
    expect(nav?.style.gridTemplateColumns).toBe("repeat(3, minmax(0, 1fr))");
  });

  it("keeps tab labels in the larger-window rail without the bottom indicator", async () => {
    await render({ presentation: "rail" });

    expect(target?.textContent).toContain("Calendar");
    expect(target?.textContent).toContain("Roadmap");
    expect(target?.textContent).toContain("Tabs");
    expect(target?.querySelector("[data-mobile-navigation-indicator]")).toBeNull();
  });
});
