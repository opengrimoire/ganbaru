// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ChatCommandMenu from "./ChatCommandMenu.svelte";

describe("ChatCommandMenu", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  it("restores its opener before opening Settings", async () => {
    target = document.createElement("div");
    document.body.append(target);
    const opener = document.createElement("button");
    target.append(opener);
    opener.focus();
    let activeElementAtOpen: Element | null = null;
    let dialogAtOpen: Element | null = null;
    const onOpenSettings = vi.fn(() => {
      activeElementAtOpen = document.activeElement;
      dialogAtOpen = target?.querySelector("[role='dialog']") ?? null;
    });
    component = mount(ChatCommandMenu, {
      target,
      props: {
        bottomPanelOpen: false,
        onClose: () => {
          const mounted = component;
          component = undefined;
          if (mounted) void unmount(mounted);
        },
        onNewChannel: vi.fn(),
        onSearch: vi.fn(),
        onToggleBottomPanel: vi.fn(),
        onToggleInspector: vi.fn(),
        onOpenSettings,
      },
    });
    await tick();

    const settingsButton = [...target.querySelectorAll<HTMLButtonElement>(".chat-command")].at(-1);
    settingsButton?.click();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(onOpenSettings).toHaveBeenCalledOnce();
    expect(activeElementAtOpen).toBe(opener);
    expect(dialogAtOpen).toBeNull();
  });
});
