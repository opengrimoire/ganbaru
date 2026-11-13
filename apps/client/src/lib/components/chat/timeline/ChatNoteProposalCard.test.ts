// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatNoteProposalStateRead } from "$lib/chat/contracts";
import type { ConfirmationRequest } from "$lib/components/ui/confirmation.svelte";
import ChatNoteProposalCard from "./ChatNoteProposalCard.svelte";

const chat = vi.hoisted(() => ({
  channelNotesVersion: 0,
  selectedChannelId: "channel:general" as string | null,
  channelNotesChanged: vi.fn<(channelId: string) => void>(),
}));
vi.mock("$lib/stores/chat.svelte", () => ({ useChat: () => chat }));
const api = vi.hoisted(() => ({
  readChatNoteProposalStates: vi.fn<(ids: readonly string[]) => Promise<ChatNoteProposalStateRead[]>>(),
  acceptChatNoteProposal: vi.fn<(request: { itemId: string; body?: string }) => Promise<unknown>>(),
  dismissChatNoteProposal: vi.fn<(request: { itemId: string }) => Promise<void>>(),
}));
vi.mock("$lib/api/chat/channel-notes", () => api);
const confirmation = vi.hoisted(() => ({
  requestConfirmation: vi.fn<(request: ConfirmationRequest) => Promise<boolean>>(),
}));
vi.mock("$lib/components/ui/confirmation.svelte", () => confirmation);

const itemId = "item:proposal";

async function settle(): Promise<void> {
  for (let index = 0; index < 4; index += 1) {
    await Promise.resolve();
    await tick();
  }
}

describe("ChatNoteProposalCard", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.resetAllMocks();
    confirmation.requestConfirmation.mockResolvedValue(false);
    api.readChatNoteProposalStates.mockResolvedValue([{ proposalItemId: itemId, state: "pending" }]);
    api.acceptChatNoteProposal.mockResolvedValue({});
    api.dismissChatNoteProposal.mockResolvedValue();
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  async function render(body = "Reply in English"): Promise<HTMLElement> {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatNoteProposalCard, { target, props: { proposalItemId: itemId, body } });
    await settle();
    const card = target.querySelector<HTMLElement>("[data-note-proposal]");
    if (!card) throw new Error("The card renders");
    return card;
  }

  function button(root: HTMLElement, label: string): HTMLButtonElement {
    const found = [...root.querySelectorAll<HTMLButtonElement>("button")].find((candidate) => candidate.textContent?.trim() === label);
    if (!found) throw new Error(`No ${label} button`);
    return found;
  }

  it("offers save, edit, and dismiss while the proposal is pending", async () => {
    const card = await render();

    expect(api.readChatNoteProposalStates).toHaveBeenCalledWith([itemId]);
    expect(card.dataset.noteProposalState).toBe("pending");
    expect([...card.querySelectorAll("button")].map((entry) => entry.textContent?.trim())).toEqual(["Save note", "Edit and save", "Dismiss"]);
  });

  it("saves the suggestion as written and settles on every view", async () => {
    const card = await render();
    button(card, "Save note").click();
    await settle();

    expect(api.acceptChatNoteProposal).toHaveBeenCalledWith(expect.objectContaining({ itemId }));
    expect(api.acceptChatNoteProposal.mock.calls[0]?.[0].body).toBeUndefined();
    expect(card.dataset.noteProposalState).toBe("saved");
    expect(card.querySelector('[role="status"]')?.textContent).toContain("Saved as a channel note");
    expect(chat.channelNotesChanged).toHaveBeenCalledWith("channel:general");
  });

  it("saves reworded text from a prefilled field", async () => {
    const card = await render("  Reply in English  ");
    button(card, "Edit and save").click();
    flushSync();
    const field = card.querySelector("textarea");
    if (!field) throw new Error("The card shows an edit field");
    expect(field.value).toBe("Reply in English");
    field.value = "Reply in Spanish";
    field.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    button(card, "Save").click();
    await settle();

    expect(api.acceptChatNoteProposal).toHaveBeenCalledWith(expect.objectContaining({ itemId, body: "Reply in Spanish" }));
    expect(card.dataset.noteProposalState).toBe("saved");
  });

  it("dismisses only after the person confirms", async () => {
    const card = await render();
    button(card, "Dismiss").click();
    await settle();
    expect(api.dismissChatNoteProposal).not.toHaveBeenCalled();
    expect(card.dataset.noteProposalState).toBe("pending");

    confirmation.requestConfirmation.mockResolvedValue(true);
    button(card, "Dismiss").click();
    await settle();
    expect(api.dismissChatNoteProposal).toHaveBeenCalledWith(expect.objectContaining({ itemId }));
    expect(card.dataset.noteProposalState).toBe("dismissed");
  });

  it("shows a settled proposal without actions", async () => {
    api.readChatNoteProposalStates.mockResolvedValue([{ proposalItemId: itemId, state: "dismissed" }]);
    const card = await render();

    expect(card.dataset.noteProposalState).toBe("dismissed");
    expect(card.querySelectorAll("button")).toHaveLength(0);
    expect(card.textContent).toContain("Dismissed or deleted");
  });

  it("keeps the actions and shows the refusal when saving fails", async () => {
    api.acceptChatNoteProposal.mockRejectedValue({ code: "validation", message: "channel_note_limit" });
    const card = await render();
    button(card, "Save note").click();
    await settle();

    expect(card.dataset.noteProposalState).toBe("pending");
    expect(card.querySelectorAll(".proposal-actions button")).toHaveLength(3);
    expect(card.textContent).not.toContain("Saved as a channel note");
  });
});
