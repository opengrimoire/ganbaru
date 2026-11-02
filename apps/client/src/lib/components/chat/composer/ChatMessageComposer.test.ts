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
      orderKey: "c001",
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
      localAcceptance: null,
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

  function mountWaitingWork(): { notice: HTMLElement; startHere: HTMLButtonElement } {
    const page = workingReplyThread();
    if (!page.assignment) throw new Error("The fixture holds an assignment");
    chat.replyThread = {
      ...page,
      assignment: {
        ...page.assignment,
        state: "queued",
        localAcceptance: {
          digest: "digest-seen",
          providerInstanceId: "codex-phone",
          folders: [{ displayName: "Website", capability: "edit", runtimeApproval: "ask" }],
          scratchRuntimeApproval: "autoApprove",
        },
      },
    };
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
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
    const notice = target.querySelector<HTMLElement>(".assignment-notice");
    const startHere = notice?.querySelector<HTMLButtonElement>("button");
    if (!notice || !startHere) throw new Error("The waiting work notice did not render");
    return { notice, startHere };
  }

  it("names the provider and access of work waiting to start", () => {
    const { notice } = mountWaitingWork();

    expect(notice.textContent).toContain("It would run with codex-phone");
    expect(notice.textContent).toContain("Edit in Website (");
    expect(notice.textContent).toContain("Private scratch (");
  });

  it("starts waiting work with the digest the user saw", async () => {
    const accept = vi.spyOn(chat, "acceptAssignmentHere").mockResolvedValue(true);
    const { startHere } = mountWaitingWork();

    startHere.click();

    await vi.waitFor(() => expect(accept).toHaveBeenCalledWith("assignment:working", "digest-seen"));
  });

  it("explains a refused start when the work changed after it rendered", async () => {
    vi.spyOn(chat, "acceptAssignmentHere").mockResolvedValue(false);
    const { startHere } = mountWaitingWork();

    startHere.click();

    await vi.waitFor(() => expect(target?.textContent).toContain("This work changed before it started"));
  });
});
