// @vitest-environment jsdom

import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { getChat } from "$lib/stores/chat.svelte";
import ChatHeaderActions from "./ChatHeaderActions.svelte";

describe("ChatHeaderActions", () => {
  const mounted: { target: HTMLDivElement; component: ReturnType<typeof mount> }[] = [];

  afterEach(async () => {
    while (mounted.length > 0) {
      const entry = mounted.pop();
      if (!entry) continue;
      await unmount(entry.component);
      entry.target.remove();
    }
  });

  it("keeps one persistent action cluster without inspector maximize or close controls", () => {
    const chat = getChat();
    chat.inspectorOpen = true;
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatHeaderActions, {
      target,
      props: {
        bottomPanelOpen: false,
        projectName: "Ganbaru",
        projectSettingsOpen: false,
        onToggleBottomPanel: vi.fn(),
        onToggleProjectSettings: vi.fn(),
      },
    });
    mounted.push({ target, component });

    expect(target.querySelectorAll("[data-chat-header-actions]")).toHaveLength(1);
    expect(target.querySelector("[aria-label='Maximize inspector']")).toBeNull();
    const inspectorToggle = [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.getAttribute("aria-pressed") === "true");
    inspectorToggle?.click();
    expect(chat.inspectorOpen).toBe(false);
  });

  it("uses the project settings trigger instead of the Chat actions menu", () => {
    const onToggleProjectSettings = vi.fn();
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatHeaderActions, {
      target,
      props: {
        bottomPanelOpen: false,
        projectName: "Ganbaru",
        projectSettingsOpen: false,
        onToggleBottomPanel: vi.fn(),
        onToggleProjectSettings,
      },
    });
    mounted.push({ target, component });

    const trigger = target.querySelector<HTMLButtonElement>("[data-chat-project-settings-trigger]");
    expect(trigger?.getAttribute("aria-label")).toBe("Chat settings for Ganbaru");
    expect(target.querySelector("details")).toBeNull();
    trigger?.click();
    expect(onToggleProjectSettings).toHaveBeenCalledWith(trigger);
  });
});
