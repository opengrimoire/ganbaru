// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  ChatAssignmentInputRead,
  ChatFollowupRouting,
  ChatMessageReference,
  ChatParticipantRead,
  ChatReplyThreadPageRead,
  PostChatMessageResult,
} from "$lib/chat/contracts";
import * as chatApi from "$lib/api/chat";
import { getChat } from "$lib/stores/chat.svelte";
import ChatMessageComposer from "./ChatMessageComposer.svelte";
import { openSlashMenu } from "./composer-test-store";

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
      review: null,
      localAcceptance: null,
      latestRunProviderThreadId: null,
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

  /** A post result for a reply that became an input of active work. */
  function routedResult(page: ChatReplyThreadPageRead, routedAs: ChatFollowupRouting | null): PostChatMessageResult {
    return {
      message: { ...page.rootMessage, itemId: "item:reply", replyThreadId: page.thread.id },
      replyThreadId: page.thread.id,
      assignment: page.assignment,
      assignmentInputQueued: routedAs !== null,
      routedAs,
    };
  }

  function waitingInput(overrides: Partial<ChatAssignmentInputRead> = {}): ChatAssignmentInputRead {
    return {
      inputId: "input:1",
      messageItemId: "item:reply",
      ordinal: 1,
      routingKind: "queued_continuation",
      deliveryState: "pending",
      deliveredAt: null,
      deliveredRevisionId: null,
      currentRevisionId: "revision:reply",
      preview: "Use the staging bucket",
      previewTruncated: false,
      authorParticipantId: null,
      ...overrides,
    };
  }

  /** Mounts the reply composer of a thread whose work is running, with a unique draft. */
  function mountActiveWork(page: ChatReplyThreadPageRead = workingReplyThread()): HTMLDivElement {
    chat.replyThread = page;
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatMessageComposer, {
      target,
      props: {
        destination: `reply-thread:${crypto.randomUUID()}`,
        placeholder: "Reply",
        threadComposer: true,
      },
    });
    return target;
  }

  async function typeReply(root: HTMLElement, text: string): Promise<void> {
    const editor = root.querySelector<HTMLDivElement>("[data-chat-composer]");
    const line = editor?.querySelector<HTMLElement>("[data-chat-composer-line]");
    if (!editor || !line) throw new Error("Expected a rich composer line");
    line.textContent = text;
    editor.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText" }));
    await tick();
  }

  it("stops active work from its own control and keeps Send for replies", async () => {
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([]);
    const page = workingReplyThread();
    const cancel = vi.spyOn(chat, "cancelAssignment").mockResolvedValue();
    const post = vi.spyOn(chat, "postOrganizationalMessage").mockResolvedValue(routedResult(page, "steer"));
    const root = mountActiveWork(page);
    await tick();

    root.querySelector<HTMLButtonElement>('.stop-button[data-action="stop"]')?.click();
    await vi.waitFor(() => expect(cancel).toHaveBeenCalledWith(page.assignment?.id));
    expect(post).not.toHaveBeenCalled();

    await typeReply(root, "Use the staging bucket");
    const send = root.querySelector<HTMLButtonElement>('.send-button[data-action="send"]');
    expect(send?.disabled).toBe(false);
    send?.click();
    await vi.waitFor(() => expect(post).toHaveBeenCalledOnce());
    expect(post.mock.calls[0]?.[1]).toMatchObject({ followupRouting: "steer" });
    expect(post.mock.calls[0]?.[1]).not.toHaveProperty("modelOverride");
  });

  it("queues a reply when the user chose Queue and explains a steer the work queued", async () => {
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([]);
    const page = workingReplyThread();
    const post = vi.spyOn(chat, "postOrganizationalMessage").mockResolvedValue(routedResult(page, "queue"));
    const root = mountActiveWork(page);
    await tick();

    await typeReply(root, "Then update the docs");
    root.querySelector<HTMLButtonElement>(".send-button")?.click();
    await vi.waitFor(() => expect(root.textContent).toContain("Queued, because the teammate was not in the middle of a turn"));
    expect(post.mock.calls[0]?.[1]).toMatchObject({ followupRouting: "steer" });

    const destination = post.mock.calls[0]?.[0];
    if (!destination) throw new Error("The post named no destination");
    chat.setOrganizationalFollowupRouting(destination, "queue");
    await typeReply(root, "And the changelog");
    root.querySelector<HTMLButtonElement>(".send-button")?.click();
    await vi.waitFor(() => expect(post).toHaveBeenCalledTimes(2));
    expect(post.mock.calls[1]?.[1]).toMatchObject({ followupRouting: "queue" });
    expect(root.textContent).not.toContain("Queued, because the teammate");
  });

  it("disables the model choice while the thread's work is active", async () => {
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([]);
    const initialTeammates = chat.teammates;
    chat.teammates = [{
      participant: teammate,
      role: "Coding teammate",
      instructions: "",
      configurationState: "healthy",
      latestPolicy: {
        id: "policy:atlas:1",
        teammateId: teammate.id,
        revision: 1,
        providerInstanceId: "codex-local",
        safetyMode: "ask_for_approval",
        providerManagedModel: false,
        modelId: "gpt-5.6-terra",
        modelOptions: [],
        effort: null,
        speed: null,
        providerOptions: { schemaVersion: 1, value: {} },
        createdAt: "2026-08-16T12:00:00.000Z",
      },
      channelCount: 1,
      activeAssignmentCount: 1,
    }];
    try {
      const root = mountActiveWork();
      await tick();

      const control = root.querySelector<HTMLElement>("[data-chat-model-override]");
      expect(control?.getAttribute("title")).toBe("Stop the current work to choose another model");
      expect(control?.querySelector("button")?.disabled).toBe(true);
    } finally {
      chat.teammates = initialTeammates;
    }
  });

  it("edits a waiting reply in the composer and restores the draft after saving", async () => {
    const page = workingReplyThread();
    const reply = { ...page.rootMessage, itemId: "item:reply", replyThreadId: page.thread.id, normalizedMarkdown: "Use the staging bucket", richContent: { schemaVersion: 1, value: {} } };
    const inputs = vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([waitingInput()]);
    const edit = vi.spyOn(chatApi, "editChatAssignmentInput").mockResolvedValue(waitingInput({ currentRevisionId: "revision:edited" }));
    vi.spyOn(chat, "loadReplyThread").mockResolvedValue();
    const post = vi.spyOn(chat, "postOrganizationalMessage");
    const root = mountActiveWork({ ...page, replies: [reply] });
    await tick();
    await typeReply(root, "My unsent draft");

    await vi.waitFor(() => expect(root.querySelector('[data-pending-action="edit"]')).not.toBeNull());
    root.querySelector<HTMLButtonElement>('[data-pending-action="edit"]')?.click();
    await tick();
    const editor = root.querySelector<HTMLDivElement>("[data-chat-composer]");
    expect(editor?.textContent).toBe("Use the staging bucket");
    expect(root.querySelector('.send-button[data-action="save"]')).not.toBeNull();

    await typeReply(root, "Use the production bucket");
    root.querySelector<HTMLButtonElement>('[data-pending-action="save"]')?.click();
    await vi.waitFor(() => expect(edit).toHaveBeenCalledOnce());
    expect(edit.mock.calls[0]?.[0]).toBe("input:1");
    expect(edit.mock.calls[0]?.[1].normalizedMarkdown).toBe("Use the production bucket");
    await vi.waitFor(() => expect(editor?.textContent).toBe("My unsent draft"));
    expect(post).not.toHaveBeenCalled();
    expect(inputs.mock.calls.length).toBeGreaterThan(1);
  });

  it("keeps a waiting reply edit to its text so no attachment or reference is dropped on save", async () => {
    const page = workingReplyThread();
    const mention: ChatMessageReference = {
      kind: "participant",
      participantId: teammate.id,
      participantKind: "ai_teammate",
      metadata: { referenceId: "reference:atlas", labelSnapshot: "Atlas", startOffset: 0, endOffset: 6, plainTextProjection: "@Atlas" },
    };
    const reply = { ...page.rootMessage, itemId: "item:reply", replyThreadId: page.thread.id, normalizedMarkdown: "@Atlas use staging", references: [mention] };
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([waitingInput()]);
    const pick = vi.spyOn(chatApi, "pickChatImages").mockResolvedValue([]);
    const edit = vi.spyOn(chatApi, "editChatAssignmentInput").mockResolvedValue(waitingInput({ currentRevisionId: "revision:edited" }));
    vi.spyOn(chat, "loadReplyThread").mockResolvedValue();
    const root = mountActiveWork({ ...page, replies: [reply] });

    await vi.waitFor(() => expect(root.querySelector('[data-pending-action="edit"]')).not.toBeNull());
    const imageTool = () => root.querySelector<HTMLButtonElement>('button[aria-label="Attach images"]');
    expect(imageTool()?.disabled).toBe(false);
    root.querySelector<HTMLButtonElement>('[data-pending-action="edit"]')?.click();
    await tick();

    expect(root.querySelector(".chat-reference-atom")).toBeNull();
    expect(imageTool()?.disabled).toBe(true);
    expect(imageTool()?.title).toBe("Only the text of a waiting reply can be edited");
    expect(root.querySelector<HTMLButtonElement>('button[aria-label="Schedule message"]')?.disabled).toBe(true);
    expect(root.querySelector("[data-chat-followup-routing]")).toBeNull();
    imageTool()?.click();
    await typeReply(root, "@At");
    await tick();
    expect(root.querySelector("#chat-reference-picker")).toBeNull();
    expect(pick).not.toHaveBeenCalled();

    await typeReply(root, "Atlas use production");
    root.querySelector<HTMLButtonElement>('[data-pending-action="save"]')?.click();
    await vi.waitFor(() => expect(edit).toHaveBeenCalledOnce());
    expect(edit.mock.calls[0]?.[1].normalizedMarkdown).toBe("Atlas use production");
    await vi.waitFor(() => expect(imageTool()?.disabled).toBe(false));
  });

  it("summarizes the running work's latest plan above the composer until every step is done", async () => {
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([]);
    const page = workingReplyThread();
    const planReply = (itemId: string, statuses: string[]) => ({
      ...page.rootMessage,
      itemId,
      replyThreadId: page.thread.id,
      richContent: { schemaVersion: 1, value: {
        type: "agent_update",
        updateKind: "plan",
        assignmentId: "assignment:working",
        agentRunId: "run:1",
        payload: { steps: statuses.map((status, index) => ({ id: `step-${index}`, text: `Step ${index + 1}`, status })) },
      } },
    });
    const root = mountActiveWork({ ...page, replies: [planReply("item:plan-1", ["active", "pending", "pending"]), planReply("item:plan-2", ["completed", "active", "pending"])] });
    await tick();

    const card = root.querySelector("[data-chat-task-progress]");
    expect(card?.textContent).toContain("Tasks 1 of 3");
    expect(card?.querySelector("[data-chat-task-active-step]")?.textContent).toBe("Step 2");
    expect(card?.querySelectorAll(".task-progress-segment")).toHaveLength(3);
    card?.querySelector<HTMLButtonElement>("button[aria-expanded]")?.click();
    await tick();
    expect(card?.querySelector("[data-chat-task-active-step]")).toBeNull();

    chat.replyThread = { ...page, replies: [planReply("item:plan-3", ["completed", "completed", "completed"])] };
    await tick();
    expect(root.querySelector("[data-chat-task-progress]")).toBeNull();
  });

  it("explains a waiting reply that reached the teammate before the edit was saved", async () => {
    const page = workingReplyThread();
    const reply = { ...page.rootMessage, itemId: "item:reply", replyThreadId: page.thread.id, normalizedMarkdown: "Use the staging bucket" };
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([waitingInput()]);
    vi.spyOn(chatApi, "editChatAssignmentInput").mockRejectedValue({ code: "conflict", message: "This reply was already delivered", recoverable: true, details: { reason: "input_already_delivered" } });
    vi.spyOn(chat, "loadReplyThread").mockResolvedValue();
    const root = mountActiveWork({ ...page, replies: [reply] });

    await vi.waitFor(() => expect(root.querySelector('[data-pending-action="edit"]')).not.toBeNull());
    root.querySelector<HTMLButtonElement>('[data-pending-action="edit"]')?.click();
    await tick();
    root.querySelector<HTMLButtonElement>('[data-pending-action="save"]')?.click();

    await vi.waitFor(() => expect(root.textContent).toContain("This reply already reached the teammate"));
    expect(root.querySelector('.send-button[data-action="send"]')).not.toBeNull();
  });

  it("offers the teammate commands on a line-start slash and runs the chosen one", async () => {
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([]);
    const root = mountActiveWork();
    const editor = root.querySelector<HTMLDivElement>("[data-chat-composer]");
    if (!editor) throw new Error("The composer did not render");
    await openSlashMenu(editor);

    const options = [...root.querySelectorAll<HTMLButtonElement>("#chat-composer-menu [role=option]")];
    expect(options.map((option) => option.textContent)).toEqual(expect.arrayContaining([
      expect.stringContaining("Schedule"),
      expect.stringContaining("Stop work"),
      expect.stringContaining("Status"),
    ]));
    expect(editor.getAttribute("aria-controls")).toBe("chat-composer-menu");
    options.find((option) => option.textContent?.includes("Status"))?.click();

    await vi.waitFor(() => expect(root.querySelector("#chat-composer-menu")).toBeNull());
    expect(editor.textContent?.replace(/\u200b/gu, "")).toBe("");
    expect(root.querySelector(".composer-info-panel")?.getAttribute("aria-label")).toBe("Status");
  });

  it("cancels a waiting reply and offers no edit before its text is loaded", async () => {
    const inputs = vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([waitingInput()]);
    const cancel = vi.spyOn(chatApi, "cancelChatAssignmentInput").mockResolvedValue(waitingInput({ deliveryState: "ignored" }));
    vi.spyOn(chat, "loadReplyThread").mockResolvedValue();
    const root = mountActiveWork();

    await vi.waitFor(() => expect(root.querySelector('[data-pending-action="cancel"]')).not.toBeNull());
    expect(root.querySelector<HTMLButtonElement>('[data-pending-action="edit"]')?.disabled).toBe(true);
    expect(root.textContent).toContain("Load this reply in the thread to edit it");
    inputs.mockResolvedValue([]);
    root.querySelector<HTMLButtonElement>('[data-pending-action="cancel"]')?.click();

    await vi.waitFor(() => expect(cancel).toHaveBeenCalledWith("input:1"));
    await vi.waitFor(() => expect(root.querySelector("[data-chat-pending-inputs]")).toBeNull());
  });

  it("stops the work a reply started instead of editing or cancelling the reply", async () => {
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([waitingInput({ routingKind: "follow_up" })]);
    const cancelInput = vi.spyOn(chatApi, "cancelChatAssignmentInput");
    const stop = vi.spyOn(chat, "cancelAssignment").mockResolvedValue();
    const page = workingReplyThread();
    const root = mountActiveWork(page);

    await vi.waitFor(() => expect(root.querySelector('[data-pending-action="cancel"]')).not.toBeNull());
    expect(root.querySelector('[data-pending-action="edit"]')).toBeNull();
    expect(root.textContent).not.toContain("Load this reply in the thread to edit it");
    root.querySelector<HTMLButtonElement>('[data-pending-action="cancel"]')?.click();

    await vi.waitFor(() => expect(stop).toHaveBeenCalledWith(page.assignment?.id));
    expect(cancelInput).not.toHaveBeenCalled();
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

  /** The reply draft of the composer `mountReviewedWork` mounts, unique per test so drafts never carry over. */
  let reviewedDestination = "";

  function mountReviewedWork(): HTMLElement {
    const page = workingReplyThread();
    if (!page.assignment) throw new Error("The fixture holds an assignment");
    chat.replyThread = { ...page, assignment: { ...page.assignment, state: "ready_for_review" } };
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    target = document.createElement("div");
    document.body.append(target);
    reviewedDestination = `reply-thread:${crypto.randomUUID()}`;
    component = mount(ChatMessageComposer, {
      target,
      props: {
        destination: reviewedDestination,
        placeholder: "Reply",
        threadComposer: true,
      },
    });
    const card = target.querySelector<HTMLElement>(".review-card");
    if (!card) throw new Error("The review card did not render");
    return card;
  }

  function buttonLabeled(container: HTMLElement, label: string): HTMLButtonElement {
    const button = [...container.querySelectorAll<HTMLButtonElement>("button")]
      .find((entry) => entry.textContent?.trim() === label);
    if (!button) throw new Error(`No button labeled ${label}`);
    return button;
  }

  async function typeNote(card: HTMLElement, text: string): Promise<HTMLTextAreaElement> {
    buttonLabeled(card, "Request changes").click();
    await tick();
    const note = card.querySelector<HTMLTextAreaElement>("textarea");
    if (!note) throw new Error("The review note did not open");
    note.value = text;
    note.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    return note;
  }

  it("offers review only while work is ready for review", () => {
    const page = workingReplyThread();
    chat.replyThread = page;
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatMessageComposer, {
      target,
      props: { destination: `reply-thread:${page.thread.id}`, placeholder: "Reply", threadComposer: true },
    });

    expect(target.querySelector(".review-card")).toBeNull();
  });

  it("accepts reviewed work and disables both actions while the review runs", async () => {
    let finish: (reviewed: boolean) => void = () => {};
    const review = vi.spyOn(chat, "reviewAssignment").mockImplementation(() => new Promise((resolve) => {
      finish = resolve;
    }));
    const card = mountReviewedWork();

    buttonLabeled(card, "Accept").click();
    await tick();

    expect(review).toHaveBeenCalledWith("assignment:working", { kind: "accept" });
    expect(buttonLabeled(card, "Accept").disabled).toBe(true);
    expect(buttonLabeled(card, "Request changes").disabled).toBe(true);
    finish(true);
    await vi.waitFor(() => expect(buttonLabeled(card, "Accept").disabled).toBe(false));
  });

  it("sends the trimmed note only once it has text", async () => {
    const review = vi.spyOn(chat, "reviewAssignment").mockResolvedValue(true);
    const card = mountReviewedWork();

    const note = await typeNote(card, "   ");
    expect(buttonLabeled(card, "Send").disabled).toBe(true);
    note.value = "  Cover the empty case  ";
    note.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(buttonLabeled(card, "Send").disabled).toBe(false);
    buttonLabeled(card, "Send").click();

    await vi.waitFor(() => expect(review).toHaveBeenCalledWith(
      "assignment:working",
      { kind: "requestChanges", note: "Cover the empty case" },
    ));
    await vi.waitFor(() => expect(card.querySelector("textarea")).toBeNull());
  });

  it("keeps the note and explains a review refused because the work changed", async () => {
    vi.spyOn(chat, "reviewAssignment").mockResolvedValue(false);
    const card = mountReviewedWork();

    await typeNote(card, "Cover the empty case");
    buttonLabeled(card, "Send").click();

    await vi.waitFor(() => expect(target?.textContent).toContain("This work changed before your review"));
    expect(card.querySelector("textarea")?.value).toBe("Cover the empty case");
  });

  it("cancels the note without reviewing", async () => {
    const review = vi.spyOn(chat, "reviewAssignment").mockResolvedValue(true);
    const card = mountReviewedWork();

    await typeNote(card, "Cover the empty case");
    buttonLabeled(card, "Cancel").click();
    await tick();

    expect(card.querySelector("textarea")).toBeNull();
    expect(review).not.toHaveBeenCalled();
  });

  it("explains an Accept refused because replies still wait on the work", async () => {
    vi.spyOn(chatApi, "reviewChatAssignment").mockRejectedValue({
      code: "conflict",
      message: "Replies still wait on this work, so it cannot be accepted yet",
      recoverable: true,
      details: { reason: "pending_replies" },
    });
    const card = mountReviewedWork();

    buttonLabeled(card, "Accept").click();

    await vi.waitFor(() => expect(target?.textContent).toContain("Replies still wait for this agent"));
    expect(target?.querySelector(".review-card")).not.toBeNull();
  });

  it("keeps the note and explains a Request changes refused because no device can run the agent", async () => {
    vi.spyOn(chatApi, "reviewChatAssignment").mockRejectedValue({
      code: "conflict",
      message: "No device can run this teammate, so the changes cannot be requested yet",
      recoverable: true,
      details: { reason: "no_executing_device" },
    });
    const card = mountReviewedWork();

    await typeNote(card, "Cover the empty case");
    buttonLabeled(card, "Send").click();

    await vi.waitFor(() => expect(target?.textContent).toContain("No device can run this agent right now"));
    expect(card.querySelector("textarea")?.value).toBe("Cover the empty case");
  });

  /** Settles the reviewed work elsewhere, as another device's review or a sync would. */
  function settleReviewedWork(): void {
    const page = chat.replyThread;
    if (!page?.assignment) throw new Error("The reviewed work is not loaded");
    chat.replyThread = {
      ...page,
      assignment: { ...page.assignment, state: "completed", revision: page.assignment.revision + 1 },
    };
  }

  it("moves the note into the reply draft when the work changes during Request changes", async () => {
    vi.spyOn(chatApi, "reviewChatAssignment").mockImplementation(() => {
      settleReviewedWork();
      return Promise.reject({
        code: "stale_revision",
        message: "The assignment changed before the review",
        recoverable: true,
      });
    });
    const card = mountReviewedWork();

    await typeNote(card, "  Cover the empty case  ");
    buttonLabeled(card, "Send").click();

    await vi.waitFor(() => expect(target?.querySelector(".review-card")).toBeNull());
    expect(chat.organizationalDraft(reviewedDestination).normalizedMarkdown).toBe("Cover the empty case");
    expect(target?.textContent).toContain("your note moved to the reply box");
  });

  it("moves the note into the reply draft when the work changes while it is typed", async () => {
    const review = vi.spyOn(chatApi, "reviewChatAssignment");
    const card = mountReviewedWork();

    await typeNote(card, "Cover the empty case");
    settleReviewedWork();
    await tick();

    expect(target?.querySelector(".review-card")).toBeNull();
    expect(chat.organizationalDraft(reviewedDestination).normalizedMarkdown).toBe("Cover the empty case");
    expect(review).not.toHaveBeenCalled();
  });

  it("keeps the note in the reply draft when the composer closes before it is sent", async () => {
    const card = mountReviewedWork();

    await typeNote(card, "Cover the empty case");
    if (component) await unmount(component);
    component = undefined;

    expect(chat.organizationalDraft(reviewedDestination).normalizedMarkdown).toBe("Cover the empty case");
  });

  it("clears the note once the changes are requested", async () => {
    vi.spyOn(chat, "reviewAssignment").mockResolvedValue(true);
    const card = mountReviewedWork();

    await typeNote(card, "Cover the empty case");
    buttonLabeled(card, "Send").click();
    await vi.waitFor(() => expect(card.querySelector("textarea")).toBeNull());
    settleReviewedWork();
    await tick();

    expect(chat.organizationalDraft(reviewedDestination).normalizedMarkdown).toBe("");
  });
});
