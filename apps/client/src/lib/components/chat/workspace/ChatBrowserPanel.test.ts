// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { BrowserTabRead } from "$lib/chat/contracts";
import { getChat } from "$lib/stores/chat.svelte";
import ChatBrowserPanel from "./ChatBrowserPanel.svelte";

const api = vi.hoisted(() => ({
  readBrowserStatus: vi.fn(async (): Promise<BrowserTabRead[]> => []),
  readPreviewModelOrigins: vi.fn(async () => []),
  readPendingBrowserApproval: vi.fn(async () => null),
  openBrowserTab: vi.fn<(request: { threadId: string; tabId: string; url: string }) => Promise<BrowserTabRead>>(),
  startBrowserRecording: vi.fn<(threadId: string, tabId: string) => Promise<void>>(),
  stopBrowserRecording: vi.fn(async () => ({ displayName: "recording.webm" })),
  setBrowserTabVisible: vi.fn(async (threadId: string, tabId: string, visible: boolean): Promise<BrowserTabRead> => ({
    threadId, tabId, currentUrl: "", title: "", visible, loading: false, viewportWidth: 1, viewportHeight: 1, externalOrigin: false,
  })),
}));
vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  ...api,
}));
vi.mock("$lib/components/ui/confirmation.svelte", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/components/ui/confirmation.svelte")>(),
  requestConfirmation: async () => true,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

function openedTab(threadId: string, tabId: string, currentUrl: string): BrowserTabRead {
  return { threadId, tabId, currentUrl, title: "Preview", visible: true, loading: false, viewportWidth: 800, viewportHeight: 600, externalOrigin: false };
}

async function settle(): Promise<void> {
  for (let round = 0; round < 6; round += 1) {
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  flushSync();
}

describe("ChatBrowserPanel", () => {
  const mounted: { target: HTMLDivElement; component: ReturnType<typeof mount> }[] = [];

  beforeAll(() => {
    globalThis.ResizeObserver ??= class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    };
  });

  afterEach(async () => {
    while (mounted.length > 0) {
      const entry = mounted.pop();
      if (!entry) continue;
      await unmount(entry.component);
      entry.target.remove();
    }
    vi.clearAllMocks();
  });

  it("keeps a page opened for an earlier thread out of the thread selected while it loaded", async () => {
    const chat = getChat();
    chat.selectedThreadId = "thread-a";
    let finishOpen = null as ((tab: BrowserTabRead) => void) | null;
    api.openBrowserTab.mockImplementation(() => new Promise((resolve) => { finishOpen = resolve; }));
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatBrowserPanel, { target, props: { channelId: null, workingFolderId: null } });
    mounted.push({ target, component });
    await settle();

    const input = target.querySelector<HTMLInputElement>("input[aria-label='Preview URL']");
    expect(input).not.toBeNull();
    if (!input) return;
    input.value = "localhost:5173";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    target.querySelector<HTMLFormElement>("form.browser-toolbar")?.requestSubmit();
    await settle();
    expect(finishOpen).not.toBeNull();

    chat.selectedThreadId = "thread-b";
    await settle();
    finishOpen?.(openedTab("thread-a", "preview:thread-a:main", "http://localhost:5173/"));
    await settle();

    expect(target.querySelectorAll("[role='tab']")).toHaveLength(0);
    expect(input.value).toBe("");
    expect(api.setBrowserTabVisible).toHaveBeenCalledWith("thread-a", "preview:thread-a:main", false);
  });

  it("stops a recording that started after another thread was selected", async () => {
    const chat = getChat();
    chat.selectedThreadId = "thread-a";
    api.readBrowserStatus.mockImplementation(async (threadId?: string) => (
      threadId ? [openedTab(threadId, `preview:${threadId}:main`, "http://localhost:5173/")] : []
    ));
    let finishStart = null as (() => void) | null;
    api.startBrowserRecording.mockImplementation(() => new Promise((resolve) => { finishStart = resolve; }));
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatBrowserPanel, { target, props: { channelId: null, workingFolderId: null } });
    mounted.push({ target, component });
    await settle();

    const record = () => target.querySelector<HTMLButtonElement>("button[aria-label='Start recording']");
    record()?.click();
    await settle();
    expect(api.startBrowserRecording).toHaveBeenCalledWith("thread-a", "preview:thread-a:main");

    chat.selectedThreadId = "thread-b";
    await settle();
    finishStart?.();
    await settle();

    expect(api.stopBrowserRecording).toHaveBeenCalledWith("thread-a", "preview:thread-a:main");
    expect(record()?.classList.contains("active")).toBe(false);
  });

  it("keeps a recording start failure for an earlier thread out of the thread selected after it", async () => {
    const chat = getChat();
    chat.selectedThreadId = "thread-a";
    api.readBrowserStatus.mockImplementation(async (threadId?: string) => (
      threadId ? [openedTab(threadId, `preview:${threadId}:main`, "http://localhost:5173/")] : []
    ));
    let failStart = null as ((reason: Error) => void) | null;
    api.startBrowserRecording.mockImplementation(() => new Promise((_resolve, reject) => { failStart = reject; }));
    const target = document.createElement("div");
    document.body.append(target);
    const component = mount(ChatBrowserPanel, { target, props: { channelId: null, workingFolderId: null } });
    mounted.push({ target, component });
    await settle();

    target.querySelector<HTMLButtonElement>("button[aria-label='Start recording']")?.click();
    await settle();
    chat.selectedThreadId = "thread-b";
    await settle();
    failStart?.(new Error("Recorder unavailable"));
    await settle();

    expect(target.querySelector(".error")).toBeNull();
  });
});
