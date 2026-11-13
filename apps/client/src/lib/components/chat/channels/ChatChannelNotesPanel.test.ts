// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  CHAT_MAX_ACTIVE_CHANNEL_NOTES,
  type ChatChannelNoteRead,
  type ChatChannelRead,
} from "$lib/chat/contracts";
import type { ConfirmationRequest } from "$lib/components/ui/confirmation.svelte";
import ChatChannelNotesPanel from "./ChatChannelNotesPanel.svelte";

const timestamp = "2026-11-12T12:00:00.000Z";
const chat = vi.hoisted(() => ({
  channelNotesVersion: 0,
  teammates: [{ participant: { id: "participant:atlas", kind: "ai_teammate", displayName: "Atlas" } }],
  channelNotesChanged: vi.fn<(channelId: string) => void>(),
}));
vi.mock("$lib/stores/chat.svelte", () => ({ useChat: () => chat }));
const api = vi.hoisted(() => ({
  listChatChannelNotes: vi.fn<(channelId: string) => Promise<ChatChannelNoteRead[]>>(),
  createChatChannelNote: vi.fn<(request: { channelId: string; body: string }) => Promise<ChatChannelNoteRead>>(),
  updateChatChannelNote: vi.fn<(request: { noteId: string; expectedRevision: number; body: string }) => Promise<ChatChannelNoteRead>>(),
  deleteChatChannelNote: vi.fn<(request: { noteId: string; expectedRevision: number }) => Promise<void>>(),
}));
vi.mock("$lib/api/chat/channel-notes", () => api);
const confirmation = vi.hoisted(() => ({
  requestConfirmation: vi.fn<(request: ConfirmationRequest) => Promise<boolean>>(),
}));
vi.mock("$lib/components/ui/confirmation.svelte", () => confirmation);

const channel: ChatChannelRead = {
  id: "channel:general",
  conversationId: "conversation:general",
  projectId: "project:alpha",
  name: "general",
  topic: "",
  isDefault: true,
  memberships: [],
  messageCount: 0,
  unreadCount: 0,
  latestPreview: null,
  lastActivityAt: timestamp,
  attentionState: null,
  revision: 1,
  archivedAt: null,
  createdAt: timestamp,
  updatedAt: timestamp,
};

function note(id: string, body: string, proposedBy: string | null = null): ChatChannelNoteRead {
  return {
    id,
    channelId: channel.id,
    body,
    sourceItemId: proposedBy ? `item:${id}` : null,
    proposedBy,
    editedBy: null,
    revision: 3,
    createdAt: timestamp,
    updatedAt: timestamp,
  };
}

async function settle(): Promise<void> {
  for (let index = 0; index < 4; index += 1) {
    await Promise.resolve();
    await tick();
  }
}

describe("ChatChannelNotesPanel", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.resetAllMocks();
    confirmation.requestConfirmation.mockResolvedValue(false);
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
    const anchor = document.createElement("button");
    target.append(anchor);
    component = mount(ChatChannelNotesPanel, { target, props: { anchor, channel, onClose: vi.fn() } });
    await settle();
    const panel = document.querySelector<HTMLElement>("[data-chat-channel-notes-panel]");
    if (!panel) throw new Error("The panel renders, portaled out of its anchor");
    return panel;
  }

  function type(field: HTMLTextAreaElement, value: string): void {
    field.value = value;
    field.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  }

  function button(root: HTMLElement, label: string): HTMLButtonElement {
    const found = [...root.querySelectorAll<HTMLButtonElement>("button")].find(
      (candidate) => candidate.textContent?.trim() === label || candidate.getAttribute("aria-label") === label,
    );
    if (!found) throw new Error(`No ${label} button`);
    return found;
  }

  it("lists notes with who suggested them and the count against the limit", async () => {
    api.listChatChannelNotes.mockResolvedValue([note("note:a", "Reply in English", "participant:atlas"), note("note:b", "Use pnpm")]);
    const root = await render();

    expect(api.listChatChannelNotes).toHaveBeenCalledWith(channel.id);
    expect(root.querySelectorAll("[data-channel-note]")).toHaveLength(2);
    expect(root.querySelector('[data-channel-note="note:a"]')?.textContent).toContain("Suggested by Atlas");
    expect(root.querySelector('[data-channel-note="note:b"]')?.textContent).not.toContain("Suggested by");
    expect(root.querySelector("[data-channel-notes-count]")?.textContent).toContain(`2 of ${CHAT_MAX_ACTIVE_CHANNEL_NOTES} notes`);
    expect(root.querySelector("[data-channel-notes-empty]")).toBeNull();
  });

  it("adds a note, rereads the list, and tells other views", async () => {
    api.listChatChannelNotes.mockResolvedValueOnce([]).mockResolvedValue([note("note:new", "Use pnpm")]);
    api.createChatChannelNote.mockResolvedValue(note("note:new", "Use pnpm"));
    const root = await render();
    expect(root.querySelector("[data-channel-notes-empty]")).not.toBeNull();

    const add = button(root, "Add note");
    expect(add.disabled).toBe(true);
    const field = root.querySelector<HTMLTextAreaElement>('textarea[aria-label="Add note"]');
    if (!field) throw new Error("The panel shows an add field");
    type(field, "Use pnpm");
    add.click();
    await settle();

    expect(api.createChatChannelNote).toHaveBeenCalledWith(expect.objectContaining({ channelId: channel.id, body: "Use pnpm" }));
    expect(chat.channelNotesChanged).toHaveBeenCalledWith(channel.id);
    expect(field.value).toBe("");
    expect(root.querySelectorAll("[data-channel-note]")).toHaveLength(1);
  });

  it("refuses an oversize note before sending it", async () => {
    api.listChatChannelNotes.mockResolvedValue([]);
    const root = await render();
    const field = root.querySelector<HTMLTextAreaElement>('textarea[aria-label="Add note"]');
    if (!field) throw new Error("The panel shows an add field");
    type(field, "x".repeat(1_001));

    expect(root.querySelector('[role="alert"]')?.textContent).toContain("at most 1000 characters");
    expect(button(root, "Add note").disabled).toBe(true);
  });

  it("replaces the add field with the limit message when the channel is full", async () => {
    api.listChatChannelNotes.mockResolvedValue(
      Array.from({ length: CHAT_MAX_ACTIVE_CHANNEL_NOTES }, (_, index) => note(`note:${index}`, `Fact ${index}`)),
    );
    const root = await render();

    expect(root.querySelector('textarea[aria-label="Add note"]')).toBeNull();
    expect(root.querySelector('footer [role="status"]')?.textContent).toContain(`already holds ${CHAT_MAX_ACTIVE_CHANNEL_NOTES} notes`);
  });

  it("rewords a note at its current revision", async () => {
    api.listChatChannelNotes.mockResolvedValue([note("note:a", "Use npm")]);
    api.updateChatChannelNote.mockResolvedValue(note("note:a", "Use pnpm"));
    const root = await render();

    button(root, "Edit note").click();
    flushSync();
    const field = root.querySelector<HTMLTextAreaElement>('[data-channel-note="note:a"] textarea');
    if (!field) throw new Error("The note shows an edit field");
    expect(field.value).toBe("Use npm");
    type(field, "Use pnpm");
    button(root, "Save").click();
    await settle();

    expect(api.updateChatChannelNote).toHaveBeenCalledWith(
      expect.objectContaining({ noteId: "note:a", expectedRevision: 3, body: "Use pnpm" }),
    );
    expect(root.querySelector('[data-channel-note="note:a"] textarea')).toBeNull();
  });

  it("deletes only after the person confirms", async () => {
    api.listChatChannelNotes.mockResolvedValue([note("note:a", "Use npm")]);
    api.deleteChatChannelNote.mockResolvedValue();
    const root = await render();

    button(root, "Delete note").click();
    await settle();
    expect(confirmation.requestConfirmation).toHaveBeenCalledWith(expect.objectContaining({ danger: true }));
    expect(api.deleteChatChannelNote).not.toHaveBeenCalled();

    confirmation.requestConfirmation.mockResolvedValue(true);
    button(root, "Delete note").click();
    await settle();
    expect(api.deleteChatChannelNote).toHaveBeenCalledWith(expect.objectContaining({ noteId: "note:a", expectedRevision: 3 }));
    expect(chat.channelNotesChanged).toHaveBeenCalledWith(channel.id);
  });

  it("shows a refused command in words and keeps the draft", async () => {
    api.listChatChannelNotes.mockResolvedValue([]);
    api.createChatChannelNote.mockRejectedValue({ code: "validation", message: "channel_note_limit" });
    const root = await render();
    const field = root.querySelector<HTMLTextAreaElement>('textarea[aria-label="Add note"]');
    if (!field) throw new Error("The panel shows an add field");
    type(field, "Use pnpm");
    button(root, "Add note").click();
    await settle();

    expect(field.value).toBe("Use pnpm");
    expect(chat.channelNotesChanged).not.toHaveBeenCalled();
    expect(root.querySelector(".notice, [role=\"alert\"]")).not.toBeNull();
  });
});
