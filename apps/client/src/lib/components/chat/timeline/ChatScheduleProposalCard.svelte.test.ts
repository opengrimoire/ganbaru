// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatScheduleProposal, ChatScheduleProposalStateRead } from "$lib/chat/contracts";
import type { ConfirmationRequest } from "$lib/components/ui/confirmation.svelte";
import ChatScheduleProposalCard from "./ChatScheduleProposalCard.svelte";

interface ReactiveChat {
  scheduledMessagesVersion: number;
}

const store = vi.hoisted(() => ({ chat: null as ReactiveChat | null }));
vi.mock("$lib/stores/chat.svelte", () => ({
  useChat: () => {
    if (!store.chat) throw new Error("The chat store is set up");
    return store.chat;
  },
}));
const api = vi.hoisted(() => ({
  readChatScheduleProposalStates: vi.fn<(ids: readonly string[]) => Promise<ChatScheduleProposalStateRead[]>>(),
  dismissChatScheduleProposal: vi.fn<(itemId: string) => Promise<void>>(),
}));
vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  ...api,
}));
const confirmation = vi.hoisted(() => ({
  requestConfirmation: vi.fn<(request: ConfirmationRequest) => Promise<boolean>>(),
}));
vi.mock("$lib/components/ui/confirmation.svelte", () => confirmation);

const chat = $state<ReactiveChat>({ scheduledMessagesVersion: 0 });
store.chat = chat;

const itemId = "item:proposal";
const proposal: ChatScheduleProposal = {
  message: "Check the build",
  firstAt: "2099-01-01T09:00:00.000Z",
  recurrence: null,
  reason: "Nightly check",
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

describe("ChatScheduleProposalCard rereads", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.resetAllMocks();
    chat.scheduledMessagesVersion = 0;
    api.dismissChatScheduleProposal.mockResolvedValue();
    confirmation.requestConfirmation.mockResolvedValue(true);
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  async function render(): Promise<HTMLDivElement> {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatScheduleProposalCard, { target, props: { proposalItemId: itemId, proposal } });
    await settle();
    return target;
  }

  it("keeps a dismissed proposal hidden when a reread started before the dismissal finishes after it", async () => {
    const reread = deferred<ChatScheduleProposalStateRead[]>();
    api.readChatScheduleProposalStates
      .mockResolvedValueOnce([{ proposalItemId: itemId, state: "pending" }])
      .mockReturnValueOnce(reread.promise);
    const root = await render();
    chat.scheduledMessagesVersion += 1;
    flushSync();
    await settle();

    root.querySelector<HTMLButtonElement>(".proposal-dismiss")?.click();
    await settle();
    expect(api.dismissChatScheduleProposal).toHaveBeenCalledWith(itemId);
    expect(root.querySelector("[data-schedule-proposal]")).toBeNull();

    reread.resolve([{ proposalItemId: itemId, state: "pending" }]);
    await settle();
    expect(root.querySelector("[data-schedule-proposal]")).toBeNull();
  });

  it("clears a failed read once a later read succeeds", async () => {
    api.readChatScheduleProposalStates
      .mockRejectedValueOnce({ code: "storage", message: "busy" })
      .mockResolvedValueOnce([{ proposalItemId: itemId, state: "pending" }]);
    const root = await render();
    expect(root.querySelector(".proposal-error")).not.toBeNull();

    chat.scheduledMessagesVersion += 1;
    flushSync();
    await settle();
    expect(root.querySelector(".proposal-dismiss")).not.toBeNull();
    expect(root.querySelector(".proposal-error")).toBeNull();
  });
});
