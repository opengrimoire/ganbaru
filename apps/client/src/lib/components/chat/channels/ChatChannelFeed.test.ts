// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as provenanceApi from "$lib/api/chat/provenance";
import type { ChatChannelRead, ChatMessageRead } from "$lib/chat/contracts";
import { getChat } from "$lib/stores/chat.svelte";
import { ChatChannelNavigationController } from "$lib/stores/chat/channel-navigation-controller.svelte";
import ChatChannelFeed from "./ChatChannelFeed.svelte";

const FAILED_LOOKUP = "Could not read which devices sent this message";

const message: ChatMessageRead = {
  itemId: "message:one",
  conversationId: "conversation:general",
  replyThreadId: null,
  revisionId: "revision:one",
  revision: 1,
  author: null,
  authorLabelSnapshot: "Atlas",
  normalizedMarkdown: "Posted message",
  richContent: { schemaVersion: 1, value: {} },
  attachmentIds: [],
  references: [],
  replyThread: null,
  orderKey: "c001",
  editedAt: null,
  createdAt: "2026-08-12T20:00:00.000Z",
};

/** An archived channel, so the feed mounts without the composer. */
function archivedChannel(id: string): ChatChannelRead {
  return {
    id,
    conversationId: `conversation:${id}`,
    projectId: "project:test",
    name: id,
    topic: "",
    isDefault: false,
    memberships: [],
    messageCount: 1,
    unreadCount: 0,
    latestPreview: null,
    lastActivityAt: message.createdAt,
    attentionState: null,
    revision: 1,
    archivedAt: message.createdAt,
    createdAt: message.createdAt,
    updatedAt: message.createdAt,
  };
}

describe("ChatChannelFeed", () => {
  const chat = getChat();
  const originalChannelMessages = chat.channelMessages;
  const originalSelectedChannelId = chat.selectedChannelId;
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.spyOn(ChatChannelNavigationController.prototype, "find").mockImplementation(archivedChannel);
    vi.spyOn(provenanceApi, "loadChatMessageProvenance").mockRejectedValue({
      code: "persistence",
      message: "Read Chat message provenance",
      retryable: true,
    });
    chat.channelMessages = [message];
    chat.selectedChannelId = "first";
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    chat.channelMessages = originalChannelMessages;
    chat.selectedChannelId = originalSelectedChannelId;
    vi.restoreAllMocks();
  });

  async function failLookup(): Promise<HTMLElement> {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatChannelFeed, { target });
    await tick();
    target.querySelector<HTMLButtonElement>("button[aria-label='More Chat actions']")?.click();
    const mounted = target;
    await vi.waitFor(() => {
      expect(mounted.querySelector("[role='alert']")?.textContent).toContain(FAILED_LOOKUP);
    });
    return mounted;
  }

  it("shows a failed Sent from lookup outside the scrolled messages", async () => {
    const mounted = await failLookup();

    expect(mounted.querySelector("[role='feed'] [role='alert']")).toBeNull();
  });

  it("closes the failed lookup notice on request", async () => {
    const mounted = await failLookup();

    mounted.querySelector<HTMLButtonElement>("[role='alert'] [data-notice-dismiss]")?.click();
    await tick();

    expect(mounted.querySelector("[role='alert']")).toBeNull();
  });

  it("leaves the failed lookup notice behind when the channel changes", async () => {
    const mounted = await failLookup();

    chat.selectedChannelId = "second";
    await tick();

    expect(mounted.querySelector("[role='alert']")).toBeNull();
  });
});
