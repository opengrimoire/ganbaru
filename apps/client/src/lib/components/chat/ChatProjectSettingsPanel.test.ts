// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SidebarSortOrder } from "$lib/utils/sidebar-order";
import ChatProjectSettingsPanel from "./ChatProjectSettingsPanel.svelte";

const order = vi.hoisted(() => ({
  overrides: {} as Record<string, SidebarSortOrder>,
  setProjectOrder: vi.fn(),
}));

vi.mock("$lib/stores/chat/sidebar-order.svelte", () => ({
  getChatSidebarOrder: () => ({
    orderOverride: (projectId: string | null) => (projectId ? order.overrides[projectId] ?? null : null),
    setProjectOrder: order.setProjectOrder,
  }),
}));

function panelButton(label: string): HTMLButtonElement | undefined {
  return [...document.querySelectorAll<HTMLButtonElement>("[data-chat-project-settings-panel] button")]
    .find((button) => button.textContent?.trim() === label);
}

class ResizeObserverMock implements ResizeObserver {
  constructor(_callback: ResizeObserverCallback) {}

  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

describe("ChatProjectSettingsPanel", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount> | null;
  let trigger: HTMLButtonElement;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    target = document.createElement("div");
    trigger = document.createElement("button");
    document.body.append(trigger, target);
    component = null;
    order.overrides = {};
    order.setProjectOrder.mockReset();
  });

  afterEach(async () => {
    if (component) await unmount(component);
    trigger.remove();
    target.remove();
    vi.unstubAllGlobals();
  });

  it("opens the channel archive while keeping Save unavailable without settings", async () => {
    const onOpenArchive = vi.fn();
    component = mount(ChatProjectSettingsPanel, {
      target,
      props: {
        projectId: "project-1",
        projectName: "Ganbaru",
        triggerElement: trigger,
        onClose: vi.fn(),
        onOpenArchive,
      },
    });
    await tick();

    const panel = document.querySelector<HTMLElement>("[data-chat-project-settings-panel]");
    expect(panel?.getAttribute("aria-label")).toBe("Chat settings for Ganbaru");
    expect(panel?.parentElement).toBe(document.body);
    const save = [...(panel?.querySelectorAll<HTMLButtonElement>("button") ?? [])]
      .find((button) => button.textContent?.trim() === "Save settings");
    expect(save?.disabled).toBe(true);
    const archive = [...(panel?.querySelectorAll<HTMLButtonElement>("button") ?? [])]
      .find((button) => button.textContent?.trim() === "Channel archive");
    archive?.click();
    expect(onOpenArchive).toHaveBeenCalledOnce();
  });

  it("saves a project sidebar order and returns to the global order", async () => {
    order.overrides = { "project-1": "name-desc" };
    component = mount(ChatProjectSettingsPanel, {
      target,
      props: {
        projectId: "project-1",
        projectName: "Ganbaru",
        triggerElement: trigger,
        onClose: vi.fn(),
        onOpenArchive: vi.fn(),
      },
    });
    await tick();

    const select = [...document.querySelectorAll<HTMLButtonElement>("[data-chat-project-settings-panel] button")]
      .find((button) => button.textContent?.includes("Name (Z to A)"));
    expect(select).toBeDefined();
    expect(panelButton("Save settings")?.disabled).toBe(true);

    select?.click();
    await tick();
    const globalOption = [...document.querySelectorAll<HTMLButtonElement>('[role="option"]')]
      .find((option) => option.textContent?.includes("Use global setting"));
    globalOption?.click();
    await tick();

    const save = panelButton("Save settings");
    expect(save?.disabled).toBe(false);
    save?.click();
    expect(order.setProjectOrder).toHaveBeenCalledWith("project-1", null);
  });

  it("closes with Escape", async () => {
    const onClose = vi.fn();
    component = mount(ChatProjectSettingsPanel, {
      target,
      props: {
        projectId: "project-1",
        projectName: "Ganbaru",
        triggerElement: trigger,
        onClose,
        onOpenArchive: vi.fn(),
      },
    });
    await tick();

    document.querySelector("[data-chat-project-settings-panel]")?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
    expect(onClose).toHaveBeenCalledOnce();
  });
});
