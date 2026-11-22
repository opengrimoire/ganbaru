// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { chatPaneEvents } from "$lib/chat/pane-events";
import { CHAT_OPEN_WORKSPACE_PANEL_EVENT, type ChatWorkspaceRequest } from "$lib/chat/workspace/events";
import { chatPanelVisibility } from "$lib/chat/workspace/panel-visibility.svelte";
import { getChat } from "$lib/stores/chat.svelte";
import ChatWorkspacePanel from "./ChatWorkspacePanel.svelte";

const api = vi.hoisted(() => ({
  readChatThreadExecutionEnvironment: vi.fn<(threadId: string) => Promise<string | null>>(),
}));
vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  readChatThreadExecutionEnvironment: api.readChatThreadExecutionEnvironment,
}));
vi.mock("$lib/components/chat/review/ChatReviewPanel.svelte", () => ({ default: () => undefined }));
vi.mock("./ChatFilesPanel.svelte", () => ({ default: () => undefined }));
vi.mock("./ChatTerminalView.svelte", () => ({ default: () => undefined }));

async function settle(): Promise<void> {
  for (let round = 0; round < 6; round += 1) {
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  flushSync();
}

function requestSessionReview(): void {
  const request: ChatWorkspaceRequest = {
    source: "review",
    detail: {
      source: { kind: "working_tree", mode: "unstaged" },
      relativePath: "src/main.ts",
      sourceThreadId: "thread-elsewhere",
      sourceWorkingFolderId: "folder-a",
    },
  };
  chatPaneEvents("primary").dispatchEvent(new CustomEvent(CHAT_OPEN_WORKSPACE_PANEL_EVENT, { detail: request }));
}

describe("ChatWorkspacePanel scope changes", () => {
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
    api.readChatThreadExecutionEnvironment.mockReset();
  });

  async function mountInspector(channelId: string): Promise<HTMLDivElement> {
    const chat = getChat();
    chat.selectedChannelId = channelId;
    chat.selectedWorkingFolderId = "folder-a";
    chat.selectedThreadId = null;
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatWorkspacePanel, { target, props: { placement: "inspector", visible: true, onClose: vi.fn() } });
    mounted.push({ target, component });
    await settle();
    return target;
  }

  function tabLabels(target: HTMLElement): string[] {
    return [...target.querySelectorAll("[role='tab']")].map((tab) => tab.textContent?.trim() ?? "");
  }

  it("keeps a review still resolving out of the channel selected meanwhile", async () => {
    let finishRead = null as ((environmentId: string | null) => void) | null;
    api.readChatThreadExecutionEnvironment.mockImplementation(() => new Promise((resolve) => { finishRead = resolve; }));
    const target = await mountInspector("channel-review-a");

    requestSessionReview();
    await settle();
    expect(finishRead).not.toBeNull();
    getChat().selectedChannelId = "channel-review-b";
    await settle();
    finishRead?.(null);
    await settle();

    expect(tabLabels(target)).not.toContain("Review");
  });

  it("clears an error shown for the previous channel's panel", async () => {
    api.readChatThreadExecutionEnvironment.mockRejectedValue(new Error("Execution environment unreadable"));
    const target = await mountInspector("channel-error-a");

    requestSessionReview();
    await settle();
    expect(target.querySelector(".notice.border-b")).not.toBeNull();
    getChat().selectedChannelId = "channel-error-b";
    await settle();

    expect(target.querySelector(".notice.border-b")).toBeNull();
  });
});
