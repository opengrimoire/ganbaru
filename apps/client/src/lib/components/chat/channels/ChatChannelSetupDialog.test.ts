// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatChannelRead } from "$lib/chat/contracts";
import ChatChannelSetupDialog from "./ChatChannelSetupDialog.svelte";

const chat = vi.hoisted(() => ({
  agents: [],
  activeChannels: [],
  createChannel: vi.fn<(request: { id: string; projectId: string; name: string; topic: string }) => Promise<ChatChannelRead>>(),
  channelAgentsChanged: vi.fn<(channelId: string) => Promise<void>>(),
}));
vi.mock("$lib/stores/chat.svelte", () => ({ useChat: () => chat }));
vi.mock("$lib/stores/projects.svelte", () => ({ useProjects: () => ({ selectedProjectId: "project:alpha" }) }));

const timestamp = "2026-11-12T12:00:00.000Z";
const created: ChatChannelRead = {
  id: "channel:design",
  conversationId: "conversation:design",
  projectId: "project:alpha",
  name: "design",
  topic: "",
  isDefault: false,
  memberships: [],
  messageCount: 0,
  unreadCount: 0,
  unreadReplyCount: 0,
  unseenFailure: false,
  latestPreview: null,
  lastActivityAt: timestamp,
  attentionState: null,
  revision: 1,
  archivedAt: null,
  createdAt: timestamp,
  updatedAt: timestamp,
};

describe("ChatChannelSetupDialog", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    chat.createChannel.mockReset();
    chat.channelAgentsChanged.mockReset();
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  it("stays open while a save runs, so the save's report cannot close a dialog opened after it", async () => {
    let finishCreate: (channel: ChatChannelRead) => void = () => undefined;
    chat.createChannel.mockReturnValue(new Promise((resolve) => { finishCreate = resolve; }));
    const onSaved = vi.fn<(channel: ChatChannelRead, sectionId: string | null) => void>();
    const onCancel = vi.fn<() => void>();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatChannelSetupDialog, { target, props: { sections: [], onSaved, onCancel } });
    await tick();

    const name = target.querySelector<HTMLInputElement>("input[required]");
    if (!name) throw new Error("The dialog shows a name field");
    name.value = "design";
    name.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    target.querySelector<HTMLButtonElement>('button[type="submit"]')?.click();
    flushSync();
    expect(chat.createChannel).toHaveBeenCalledOnce();

    target.querySelector<HTMLButtonElement>(".chat-secondary-button")?.click();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    target.querySelector<HTMLElement>(".fixed.inset-0")?.click();
    flushSync();
    expect(onCancel).not.toHaveBeenCalled();

    finishCreate(created);
    for (let index = 0; index < 4; index += 1) await tick();
    expect(onSaved).toHaveBeenCalledWith(created, null);

    target.querySelector<HTMLButtonElement>(".chat-secondary-button")?.click();
    expect(onCancel).toHaveBeenCalledOnce();
  });
});
