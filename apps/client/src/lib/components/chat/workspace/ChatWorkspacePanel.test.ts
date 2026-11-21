// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { ChatTerminalRead } from "$lib/chat/contracts";
import * as chatApi from "$lib/api/chat";
import { chatPanelVisibility } from "$lib/chat/workspace/panel-visibility.svelte";
import { getChat } from "$lib/stores/chat.svelte";
import ChatWorkspacePanel from "./ChatWorkspacePanel.svelte";

function terminalRead(id: string): ChatTerminalRead {
  return {
    id,
    channelId: "channel-a",
    workingFolderId: "folder-a",
    name: id,
    shell: "sh",
    columns: 80,
    rows: 24,
    running: true,
    exitCode: null,
    generation: 1,
    lastSequence: 0,
  };
}

vi.mock("$lib/api/chat", async (importOriginal) => {
  const original = await importOriginal<typeof import("$lib/api/chat")>();
  return {
    ...original,
    listChatTerminals: vi.fn(async () => [terminalRead("terminal-1"), terminalRead("terminal-2")]),
    readChatTerminalLayout: vi.fn(async () => ({
      channelId: "channel-a",
      workingFolderId: "folder-a",
      schemaVersion: 1,
      groups: [],
      updatedAt: null,
    })),
    saveChatTerminalPanelLayout: vi.fn(async () => undefined),
  };
});

vi.mock("./ChatTerminalView.svelte", () => ({ default: () => undefined }));

async function settle(): Promise<void> {
  for (let round = 0; round < 6; round += 1) {
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  flushSync();
}

describe("ChatWorkspacePanel", () => {
  const mounted: { target: HTMLDivElement; component: ReturnType<typeof mount> }[] = [];

  beforeAll(() => {
    globalThis.ResizeObserver ??= class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    };
    Element.prototype.scrollIntoView ??= function scrollIntoView(): void {};
  });

  afterEach(async () => {
    while (mounted.length > 0) {
      const entry = mounted.pop();
      if (!entry) continue;
      await unmount(entry.component);
      entry.target.remove();
    }
    chatPanelVisibility.clear();
  });

  it("keeps the bottom panel open while switching between its terminal and panel tabs", async () => {
    const chat = getChat();
    chat.selectedChannelId = "channel-a";
    chat.selectedWorkingFolderId = "folder-a";
    chat.bottomPanelOpen = true;
    const onClose = vi.fn();
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatWorkspacePanel, { target, props: { placement: "bottom", visible: true, onClose } });
    mounted.push({ target, component });
    await settle();

    target.querySelector<HTMLButtonElement>(".panel-add-button")?.click();
    await settle();
    const filesItem = [...document.querySelectorAll<HTMLButtonElement>(".panel-picker .menu-item")]
      .find((button) => button.textContent?.includes("Files"));
    expect(filesItem).toBeDefined();
    filesItem?.click();
    await settle();

    const tabs = () => [...target.querySelectorAll<HTMLButtonElement>("[role='tab']")];
    expect(tabs()).toHaveLength(3);
    for (const index of [0, 2, 1, 2, 0]) {
      const tab = tabs()[index];
      tab?.closest(".panel-tab-slot")?.dispatchEvent(new PointerEvent("pointerdown", { button: 0, bubbles: true }));
      window.dispatchEvent(new PointerEvent("pointerup", { button: 0 }));
      tab?.click();
      await settle();
      expect(tabs()[index]?.getAttribute("aria-selected")).toBe("true");
    }

    expect(onClose).not.toHaveBeenCalled();
    expect(chat.bottomPanelOpen).toBe(true);
  });

  it("gives a reply thread tab terminals and panels apart from its channel's", async () => {
    const chat = getChat();
    chat.selectedChannelId = "channel-a";
    chat.selectedWorkingFolderId = "folder-a";
    chat.replyThreadTabId = "reply-a";
    chat.bottomPanelOpen = true;
    vi.mocked(chatApi.listChatTerminals).mockClear();
    vi.mocked(chatApi.readChatTerminalLayout).mockClear();
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatWorkspacePanel, { target, props: { placement: "bottom", visible: true, onClose: vi.fn() } });
    mounted.push({ target, component });
    await settle();

    expect(chatApi.listChatTerminals).toHaveBeenCalledWith("channel-a", "folder-a", "reply-a");
    expect(chatApi.readChatTerminalLayout).not.toHaveBeenCalled();
    chat.replyThreadTabId = null;
    expect(chat.bottomPanelOpen).toBe(false);
  });
});
