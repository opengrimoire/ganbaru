// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatChannelRead } from "$lib/chat/contracts";
import { getChat } from "$lib/stores/chat.svelte";
import ChatChannelArchive from "./ChatChannelArchive.svelte";

const ARCHIVED_AT = "2026-08-12T20:00:00.000Z";

/** An archived channel with the given id. */
function archivedChannel(id: string): ChatChannelRead {
  return {
    id,
    conversationId: `conversation:${id}`,
    projectId: "project:test",
    name: id,
    topic: "",
    isDefault: false,
    memberships: [],
    messageCount: 0,
    unreadCount: 0,
    unreadReplyCount: 0,
    unseenFailure: false,
    latestPreview: null,
    lastActivityAt: ARCHIVED_AT,
    attentionState: null,
    revision: 1,
    archivedAt: ARCHIVED_AT,
    createdAt: ARCHIVED_AT,
    updatedAt: ARCHIVED_AT,
  };
}

describe("ChatChannelArchive", () => {
  const chat = getChat();
  const originalArchived = chat.archivedChannels;
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    chat.archivedChannels = [archivedChannel("first"), archivedChannel("second")];
    chat.archivedChannelsLoading = false;
    chat.archivedChannelsError = null;
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    chat.archivedChannels = originalArchived;
    vi.restoreAllMocks();
  });

  /** The restore button of the row whose channel has the given name. */
  function restoreButton(root: HTMLElement, name: string): HTMLButtonElement | null {
    const row = [...root.querySelectorAll<HTMLElement>(".max-w-3xl > div")]
      .find((candidate) => candidate.textContent?.includes(name));
    return row?.querySelector<HTMLButtonElement>("button") ?? null;
  }

  it("keeps a channel's restore busy while an earlier restore of another channel finishes", async () => {
    const settle = new Map<string, () => void>();
    const restore = vi.spyOn(chat, "restoreChannel").mockImplementation((channel) => new Promise<void>((resolve) => {
      settle.set(channel.id, resolve);
    }));
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatChannelArchive, { target });
    await tick();
    const root = target;

    restoreButton(root, "first")?.click();
    restoreButton(root, "second")?.click();
    await tick();
    settle.get("first")?.();
    await new Promise((resolve) => setTimeout(resolve, 0));
    await tick();

    expect(restoreButton(root, "first")?.disabled).toBe(false);

    expect(restoreButton(root, "second")?.disabled).toBe(true);
    restoreButton(root, "second")?.click();
    expect(restore).toHaveBeenCalledTimes(2);
  });
});
