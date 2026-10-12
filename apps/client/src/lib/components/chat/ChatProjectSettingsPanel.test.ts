// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ChatProjectSettingsPanel from "./ChatProjectSettingsPanel.svelte";

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

  it("closes with Escape", async () => {
    const onClose = vi.fn();
    component = mount(ChatProjectSettingsPanel, {
      target,
      props: {
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
