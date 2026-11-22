// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatNoteProposalStateRead } from "$lib/chat/contracts";
import ChatNoteProposalCard from "./ChatNoteProposalCard.svelte";

interface ReactiveChat {
  channelNotesVersion: number;
  selectedChannelId: string | null;
  channelNotesChanged: (channelId: string) => void;
}

const store = vi.hoisted(() => ({ chat: null as ReactiveChat | null }));
vi.mock("$lib/stores/chat.svelte", () => ({
  useChat: () => {
    if (!store.chat) throw new Error("The chat store is set up");
    return store.chat;
  },
}));
const api = vi.hoisted(() => ({
  readChatNoteProposalStates: vi.fn<(ids: readonly string[]) => Promise<ChatNoteProposalStateRead[]>>(),
  acceptChatNoteProposal: vi.fn<(request: { itemId: string; body?: string }) => Promise<unknown>>(),
  dismissChatNoteProposal: vi.fn<(request: { itemId: string }) => Promise<void>>(),
}));
vi.mock("$lib/api/chat/channel-notes", () => api);

const chat = $state<ReactiveChat>({
  channelNotesVersion: 0,
  selectedChannelId: "channel:general",
  channelNotesChanged: () => undefined,
});
store.chat = chat;

const itemId = "item:proposal";

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void; reject: (cause: unknown) => void } {
  let resolve: (value: T) => void = () => undefined;
  let reject: (cause: unknown) => void = () => undefined;
  const promise = new Promise<T>((onResolve, onReject) => { resolve = onResolve; reject = onReject; });
  return { promise, resolve, reject };
}

async function settle(): Promise<void> {
  for (let index = 0; index < 4; index += 1) {
    await Promise.resolve();
    await tick();
  }
}

describe("ChatNoteProposalCard rereads", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.resetAllMocks();
    chat.channelNotesVersion = 0;
    api.acceptChatNoteProposal.mockResolvedValue({});
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
    component = mount(ChatNoteProposalCard, { target, props: { proposalItemId: itemId, body: "Reply in English" } });
    await settle();
    const card = target.querySelector<HTMLElement>("[data-note-proposal]");
    if (!card) throw new Error("The card renders");
    return card;
  }

  it("keeps a saved proposal saved when a reread started before the save finishes after it", async () => {
    const reread = deferred<ChatNoteProposalStateRead[]>();
    api.readChatNoteProposalStates
      .mockResolvedValueOnce([{ proposalItemId: itemId, state: "pending" }])
      .mockReturnValueOnce(reread.promise);
    const card = await render();
    chat.channelNotesVersion += 1;
    flushSync();
    await settle();

    const saveButton = [...card.querySelectorAll("button")].find((entry) => entry.textContent?.trim() === "Save note");
    if (!saveButton) throw new Error("The card offers save");
    saveButton.click();
    await settle();
    expect(card.dataset.noteProposalState).toBe("saved");

    reread.resolve([{ proposalItemId: itemId, state: "pending" }]);
    await settle();
    expect(card.dataset.noteProposalState).toBe("saved");
    expect(card.querySelectorAll(".proposal-actions button")).toHaveLength(0);
  });

  it("clears a failed read once a later read succeeds", async () => {
    api.readChatNoteProposalStates
      .mockRejectedValueOnce({ code: "storage", message: "busy" })
      .mockResolvedValueOnce([{ proposalItemId: itemId, state: "pending" }]);
    const card = await render();
    expect(card.querySelector(".proposal-error")).not.toBeNull();

    chat.channelNotesVersion += 1;
    flushSync();
    await settle();
    expect(card.dataset.noteProposalState).toBe("pending");
    expect(card.querySelector(".proposal-error")).toBeNull();
  });
});
