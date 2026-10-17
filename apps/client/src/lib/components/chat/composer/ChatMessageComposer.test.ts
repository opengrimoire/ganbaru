// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  ChatParticipantRead,
  ChatReplyThreadPageRead,
  PostChatMessageResult,
} from "$lib/chat/contracts";
import { getChat } from "$lib/stores/chat.svelte";
import ChatMessageComposer from "./ChatMessageComposer.svelte";

const teammate: ChatParticipantRead = {
  id: "participant:atlas",
  kind: "ai_teammate",
  displayName: "Atlas",
  avatar: { schemaVersion: 1, value: {} },
  revision: 1,
  archivedAt: null,
};

function workingReplyThread(): ChatReplyThreadPageRead {
  const timestamp = "2026-08-05T16:00:00.000Z";
  return {
    thread: {
      id: "reply-thread:working",
      replyCount: 0,
      lastActivityAt: timestamp,
      participants: [teammate],
      unread: false,
      workState: "working",
    },
    rootMessage: {
      itemId: "item:root",
      conversationId: "conversation:test",
      replyThreadId: null,
      revisionId: "revision:root",
      revision: 1,
      author: teammate,
      authorLabelSnapshot: "Atlas",
      normalizedMarkdown: "Work on this",
      richContent: { schemaVersion: 1, value: {} },
      attachmentIds: [],
      references: [],
      replyThread: null,
      ordinal: 1,
      editedAt: null,
      createdAt: timestamp,
    },
    replies: [],
    assignment: {
      id: "assignment:working",
      replyThreadId: "reply-thread:working",
      teammate,
      triggeringMessageItemId: "item:root",
      previousAssignmentId: null,
      state: "working",
      stateReason: null,
      revision: 1,
      settledAt: null,
      createdAt: timestamp,
      updatedAt: timestamp,
    },
    agentRuns: [],
    previousCursor: null,
    revision: 1,
  };
}

describe("ChatMessageComposer", () => {
  const chat = getChat();
  const initialReplyThread = chat.replyThread;
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    vi.restoreAllMocks();
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    chat.replyThread = initialReplyThread;
  });

  it("uses the shared rich content editor and preserves its draft", async () => {
    const destination = `channel:${crypto.randomUUID()}`;
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatMessageComposer, {
      target,
      props: { destination, placeholder: "Message" },
    });
    await tick();

    const editor = target.querySelector<HTMLDivElement>("[data-chat-composer]");
    expect(editor?.getAttribute("contenteditable")).toBe("true");
    expect(editor?.getAttribute("role")).toBe("combobox");
    expect(target.querySelector("textarea")).toBeNull();
    const line = editor?.querySelector<HTMLElement>("[data-chat-composer-line]");
    if (!line) throw new Error("Expected a rich composer line");
    line.textContent = "Coordinate the release";
    editor?.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText" }));
    await tick();

    expect(chat.organizationalDraft(destination).normalizedMarkdown).toBe("Coordinate the release");
    expect(target.querySelector<HTMLButtonElement>(".send-button")?.disabled).toBe(false);
  });

  it("focuses the visible blocker when an oversized message is activated", async () => {
    const destination = `channel:${crypto.randomUUID()}`;
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatMessageComposer, {
      target,
      props: { destination, placeholder: "Message" },
    });
    await tick();

    const editor = target.querySelector<HTMLDivElement>("[data-chat-composer]");
    const line = editor?.querySelector<HTMLElement>("[data-chat-composer-line]");
    if (!line) throw new Error("Expected a rich composer line");
    line.textContent = "a".repeat(131_073);
    editor?.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText" }));
    await tick();

    const primary = target.querySelector<HTMLButtonElement>(".send-button");
    expect(primary?.getAttribute("aria-disabled")).toBe("true");
    primary?.click();
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(target?.querySelector(".composer-blocker"));
    });
  });

  it("keeps the primary action bound to active work cancellation", async () => {
    const page = workingReplyThread();
    chat.replyThread = page;
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    const cancel = vi.spyOn(chat, "cancelAssignment").mockResolvedValue();
    const post = vi.spyOn(chat, "postOrganizationalMessage").mockResolvedValue(
      undefined as unknown as PostChatMessageResult,
    );
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatMessageComposer, {
      target,
      props: {
        destination: `reply-thread:${page.thread.id}`,
        placeholder: "Reply",
        threadComposer: true,
      },
    });

    const primary = target.querySelector<HTMLButtonElement>('.send-button[data-action="stop"]');
    expect(primary?.disabled).toBe(false);
    primary?.click();

    await vi.waitFor(() => expect(cancel).toHaveBeenCalledWith(page.assignment?.id));
    expect(post).not.toHaveBeenCalled();
  });
});
