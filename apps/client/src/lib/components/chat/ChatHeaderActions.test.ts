// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
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

  it("opens the secondary panel only in the selected channel and restores it when switching back", () => {
    const chat = getChat();
    const previousChannelId = chat.selectedChannelId;
    chat.selectedChannelId = "channel-a";
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
    const inspectorToggle = () => target.querySelector<HTMLButtonElement>("[data-chat-inspector-action]");

    inspectorToggle()?.click();
    flushSync();
    expect(inspectorToggle()?.getAttribute("aria-pressed")).toBe("true");

    chat.selectedChannelId = "channel-b";
    flushSync();
    expect(chat.inspectorOpen).toBe(false);
    expect(inspectorToggle()?.getAttribute("aria-pressed")).toBe("false");

    chat.selectedChannelId = "channel-a";
    flushSync();
    expect(chat.inspectorOpen).toBe(true);
    expect(inspectorToggle()?.getAttribute("aria-pressed")).toBe("true");

    chat.inspectorOpen = false;
    chat.selectedChannelId = previousChannelId;
  });

  it("keeps only the panel toggles in a reply thread shown in its own tab", () => {
    const chat = getChat();
    const previousChannelId = chat.selectedChannelId;
    chat.selectedChannelId = "channel-a";
    chat.replyThreadTabId = "reply-a";
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

    expect(target.querySelector("[data-chat-project-settings-trigger]")).toBeNull();
    expect(target.querySelector("[data-chat-reply-thread-new-tab]")).toBeNull();
    expect(target.querySelector("[aria-label='Open in a new window']")).toBeNull();

    chat.replyThreadTabId = null;
    flushSync();
    expect(target.querySelector("[data-chat-project-settings-trigger]")).not.toBeNull();
    chat.selectedChannelId = previousChannelId;
  });
});
