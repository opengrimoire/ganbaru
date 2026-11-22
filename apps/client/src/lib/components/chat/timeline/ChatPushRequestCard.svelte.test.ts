// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatPushRequest, ChatPushRequestStateRead } from "$lib/chat/contracts";
import ChatPushRequestCard from "./ChatPushRequestCard.svelte";

interface ReactiveChat {
  pushRequestsVersion: number;
  confirmPushRequest: (request: { requestItemId: string }, workingFolderId: string) => Promise<unknown>;
}

const store = vi.hoisted(() => ({ chat: null as ReactiveChat | null }));
vi.mock("$lib/stores/chat.svelte", () => ({
  useChat: () => {
    if (!store.chat) throw new Error("The chat store is set up");
    return store.chat;
  },
}));
const api = vi.hoisted(() => ({
  readChatPushRequestStates: vi.fn<(ids: readonly string[]) => Promise<ChatPushRequestStateRead[]>>(),
  confirmPushRequest: vi.fn<(request: { requestItemId: string }, workingFolderId: string) => Promise<unknown>>(),
}));
vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  chatPushRequestsAvailable: true,
  readChatPushRequestStates: api.readChatPushRequestStates,
}));

const chat = $state<ReactiveChat>({
  pushRequestsVersion: 0,
  confirmPushRequest: (request, workingFolderId) => api.confirmPushRequest(request, workingFolderId),
});
store.chat = chat;

const itemId = "item:push";
const push: ChatPushRequest = {
  workingFolderId: "folder:1",
  executionEnvironmentId: null,
  localBranch: "feature",
  remote: "origin",
  destinationBranch: "feature",
  pushUrl: "https://example.invalid/repo.git",
  objectId: "0123456789abcdef0123456789abcdef01234567",
  gitConfigDigest: "digest",
};

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((onResolve) => { resolve = onResolve; });
  return { promise, resolve };
}

async function settle(): Promise<void> {
  for (let index = 0; index < 4; index += 1) {
    await Promise.resolve();
    await tick();
  }
}

describe("ChatPushRequestCard rereads", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.resetAllMocks();
    chat.pushRequestsVersion = 0;
    api.confirmPushRequest.mockResolvedValue({});
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  async function render(): Promise<HTMLElement> {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatPushRequestCard, { target, props: { requestItemId: itemId, push } });
    await settle();
    const card = target.querySelector<HTMLElement>("[data-push-request]");
    if (!card) throw new Error("The card renders");
    return card;
  }

  it("keeps a pushed request pushed when a reread started before the push finishes after it", async () => {
    const reread = deferred<ChatPushRequestStateRead[]>();
    api.readChatPushRequestStates
      .mockResolvedValueOnce([{ requestItemId: itemId, state: "open" }])
      .mockReturnValueOnce(reread.promise);
    const card = await render();
    chat.pushRequestsVersion += 1;
    flushSync();
    await settle();

    card.querySelector<HTMLButtonElement>(".push-confirm")?.click();
    await settle();
    expect(api.confirmPushRequest).toHaveBeenCalledWith(expect.objectContaining({ requestItemId: itemId }), "folder:1");
    expect(card.querySelector(".push-confirm")).toBeNull();

    reread.resolve([{ requestItemId: itemId, state: "open" }]);
    await settle();
    expect(card.querySelector(".push-confirm")).toBeNull();
    expect(card.querySelector('[role="status"]')).not.toBeNull();
  });

  it("clears a failed read once a later read succeeds", async () => {
    api.readChatPushRequestStates
      .mockRejectedValueOnce({ code: "storage", message: "busy" })
      .mockResolvedValueOnce([{ requestItemId: itemId, state: "open" }]);
    const card = await render();
    expect(card.querySelector(".push-error")).not.toBeNull();

    chat.pushRequestsVersion += 1;
    flushSync();
    await settle();
    expect(card.querySelector(".push-confirm")).not.toBeNull();
    expect(card.querySelector(".push-error")).toBeNull();
  });

  it("keeps a refused push visible after the reread that follows it", async () => {
    api.readChatPushRequestStates.mockResolvedValue([{ requestItemId: itemId, state: "open" }]);
    api.confirmPushRequest.mockRejectedValue({ code: "conflict", message: "push failed" });
    const card = await render();

    card.querySelector<HTMLButtonElement>(".push-confirm")?.click();
    await settle();
    expect(api.readChatPushRequestStates).toHaveBeenCalledTimes(2);
    expect(card.querySelector(".push-error")).not.toBeNull();
  });
});
