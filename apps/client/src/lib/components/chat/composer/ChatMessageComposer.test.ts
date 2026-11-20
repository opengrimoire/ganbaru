// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  ChatAssignmentInputRead,
  ChatFollowupRouting,
  ChatMessageReference,
  ChatParticipantRead,
  ChatReplyThreadPageRead,
  ChatScheduledMessageRead,
  PostChatMessageResult,
} from "$lib/chat/contracts";
import * as chatApi from "$lib/api/chat";
import { chatPaneEvents } from "$lib/chat/pane-events";
import { getChat } from "$lib/stores/chat.svelte";
import ChatMessageComposer from "./ChatMessageComposer.svelte";
import { openSlashMenu } from "./composer-test-store";

const agent: ChatParticipantRead = {
  id: "participant:atlas",
  kind: "ai_agent",
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
      participants: [agent],
      unread: false,
      unseenFailure: false,
      workState: "working",
    },
    rootMessage: {
      itemId: "item:root",
      conversationId: "conversation:test",
      replyThreadId: null,
      revisionId: "revision:root",
      revision: 1,
      author: agent,
      authorLabelSnapshot: "Atlas",
      normalizedMarkdown: "Work on this",
      richContent: { schemaVersion: 1, value: {} },
      attachmentIds: [],
      references: [],
      replyThread: null,
      orderKey: "c001",
      awaitingDelivery: false,
      editedAt: null,
      createdAt: timestamp,
    },
    replies: [],
    assignment: {
      id: "assignment:working",
      replyThreadId: "reply-thread:working",
      agent,
      triggeringMessageItemId: "item:root",
      previousAssignmentId: null,
      state: "working",
      stateReason: null,
      revision: 1,
      settledAt: null,
      createdAt: timestamp,
      updatedAt: timestamp,
      review: null,
      delegation: null,
      latestRunProviderThreadId: null,
      modelSelection: { providerManagedModel: true, modelId: null, modelOptions: [] },
      instructions: "",
      modelLabel: null,
    },
    agentRuns: [],
    delegatedChildren: [],
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
      held: false,
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

  /** Opens the more menu of the first waiting reply once the queue lists it. */
  async function openReplyMenu(root: HTMLElement): Promise<HTMLElement> {
    await vi.waitFor(() => expect(root.querySelector('[data-pending-action="more"]')).not.toBeNull());
    root.querySelector<HTMLButtonElement>('[data-pending-action="more"]')?.click();
    await tick();
    const menu = document.querySelector<HTMLElement>("[data-chat-queued-reply-menu]");
    if (!menu) throw new Error("Expected the waiting reply menu");
    return menu;
  }

  it("stops active work from the pane shortcut without a stop button of its own and keeps Send for replies", async () => {
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([]);
    const page = workingReplyThread();
    const cancel = vi.spyOn(chat, "cancelAssignment").mockResolvedValue();
    const post = vi.spyOn(chat, "postOrganizationalMessage").mockResolvedValue(routedResult(page, "steer"));
    const root = mountActiveWork(page);
    await tick();

    expect(root.querySelector('[data-action="stop"]')).toBeNull();
    chatPaneEvents("primary").dispatchEvent(new Event("ganbaru:chat-stop-requested"));
    await vi.waitFor(() => expect(cancel).toHaveBeenCalledWith(page.assignment?.id));
    expect(post).not.toHaveBeenCalled();

    await typeReply(root, "Use the staging bucket");
    const send = root.querySelector<HTMLButtonElement>('.send-button[data-action="send"]');
    expect(send?.disabled).toBe(false);
    send?.click();
    await vi.waitFor(() => expect(post).toHaveBeenCalledOnce());
    expect(post.mock.calls[0]?.[1]).toEqual({ alsoSendToChannel: false, followupRouting: "queue" });
  });

  /** Presses Enter in the composer, with the platform shortcut modifier when `modified`. */
  async function pressEnter(root: HTMLElement, modified: boolean): Promise<KeyboardEvent> {
    const editor = root.querySelector<HTMLDivElement>("[data-chat-composer]");
    if (!editor) throw new Error("Expected the rich composer");
    const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "Enter", ctrlKey: modified });
    editor.dispatchEvent(event);
    await tick();
    return event;
  }

  it("queues a reply on Enter and steers on Mod+Enter while work runs, explaining a steer the work queued", async () => {
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([]);
    const page = workingReplyThread();
    const post = vi.spyOn(chat, "postOrganizationalMessage").mockResolvedValue(routedResult(page, "queue"));
    const root = mountActiveWork(page);
    await tick();

    expect(root.querySelector("button[data-chat-followup-routing]")).toBeNull();
    expect(root.querySelector(".send-button")?.getAttribute("data-app-tooltip")).toBe("Queue (Enter)\nSteer (Ctrl + Enter)");

    await typeReply(root, "Then update the docs");
    expect((await pressEnter(root, false)).defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(post).toHaveBeenCalledOnce());
    expect(post.mock.calls[0]?.[1]).toMatchObject({ followupRouting: "queue" });
    expect(root.textContent).not.toContain("Queued, because the agent");

    await typeReply(root, "Prefer the smaller fix");
    await pressEnter(root, true);
    await vi.waitFor(() => expect(post).toHaveBeenCalledTimes(2));
    expect(post.mock.calls[1]?.[1]).toMatchObject({ followupRouting: "steer" });
    await vi.waitFor(() => expect(root.textContent).toContain("Queued, because the agent was not in the middle of a turn"));
  });

  it("sends on Enter and Mod+Enter without routing when no work runs", async () => {
    const page = workingReplyThread();
    const idle = { ...page, assignment: page.assignment ? { ...page.assignment, state: "completed" as const } : null };
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([]);
    const post = vi.spyOn(chat, "postOrganizationalMessage").mockResolvedValue(routedResult(idle, null));
    const root = mountActiveWork(idle);
    await tick();

    expect(root.querySelector(".send-button")?.getAttribute("data-app-tooltip")).toBe("Send (Enter)");
    await typeReply(root, "Thanks");
    await pressEnter(root, false);
    await vi.waitFor(() => expect(post).toHaveBeenCalledOnce());
    await typeReply(root, "One more thing");
    await pressEnter(root, true);
    await vi.waitFor(() => expect(post).toHaveBeenCalledTimes(2));
    expect(post.mock.calls.map((call) => call[1])).toEqual([
      { alsoSendToChannel: false },
      { alsoSendToChannel: false },
    ]);
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

    (await openReplyMenu(root)).querySelector<HTMLButtonElement>('[data-pending-action="edit"]')?.click();
    await tick();
    expect(document.querySelector("[data-chat-queued-reply-menu]")).toBeNull();
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

  function pendingScheduled(overrides: Partial<ChatScheduledMessageRead> = {}): ChatScheduledMessageRead {
    return {
      id: "scheduled:1",
      channelId: "channel:1",
      replyThreadId: null,
      normalizedMarkdown: "Post the weekly status",
      richContent: { schemaVersion: 1, value: {} },
      attachmentIds: [],
      references: [],
      alsoSendToChannel: false,
      state: "scheduled",
      scheduledFor: "2099-01-05T09:00:00.000Z",
      lastError: null,
      createdAt: "2026-11-14T10:00:00.000Z",
      recurrence: null,
      ...overrides,
    };
  }

  /** Mounts a channel composer with `scheduled` pending and opens its edit from the manage list. */
  async function editScheduledFromList(scheduled: ChatScheduledMessageRead): Promise<HTMLDivElement> {
    const destination = `channel:${crypto.randomUUID()}`;
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([scheduled]);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatMessageComposer, { target, props: { destination, placeholder: "Message" } });
    const root = target;
    await tick();
    await typeReply(root, "My unsent draft");
    await vi.waitFor(() => expect(root.querySelector(".scheduled-summary button")).not.toBeNull());
    root.querySelector<HTMLButtonElement>(".scheduled-summary button")?.click();
    await vi.waitFor(() => expect(root.querySelector('[data-message-action="edit"]')).not.toBeNull());
    root.querySelector<HTMLButtonElement>('[data-message-action="edit"]')?.click();
    await tick();
    return root;
  }

  it("edits a scheduled message in place from the manage list and restores the draft after saving", async () => {
    const scheduled = pendingScheduled({
      recurrence: {
        rule: { kind: "interval", minutes: 120 },
        timeZone: "UTC",
        quietPolicy: "defer",
        nextOccurrenceAt: "2099-01-05T13:00:00.000Z",
        firedCount: 2,
        skippedOccurrences: 0,
        deferredUntil: null,
      },
    });
    const update = vi.spyOn(chat, "updateScheduledOrganizationalMessage")
      .mockImplementation((request) => Promise.resolve({ ...scheduled, normalizedMarkdown: request.normalizedMarkdown }));
    const schedule = vi.spyOn(chat, "scheduleOrganizationalMessage");
    const root = await editScheduledFromList(scheduled);
    const editor = root.querySelector<HTMLDivElement>("[data-chat-composer]");

    expect(root.querySelector('[role="dialog"][aria-label="Scheduled messages"]')).toBeNull();
    expect(editor?.textContent).toBe("Post the weekly status");
    expect(root.querySelector("[data-scheduled-edit]")?.textContent).toContain("Editing a scheduled message");
    expect(root.querySelector('.send-button[data-action="save"]')?.getAttribute("aria-label")).toBe("Save scheduled message");

    await typeReply(root, "Post the weekly status with the release");
    root.querySelector<HTMLButtonElement>('[data-scheduled-edit-action="save"]')?.click();
    await vi.waitFor(() => expect(update).toHaveBeenCalledOnce());
    const request = update.mock.calls[0]?.[0];
    expect(request?.scheduledMessageId).toBe("scheduled:1");
    expect(request?.normalizedMarkdown).toBe("Post the weekly status with the release");
    expect(request?.scheduledFor).toBe("2099-01-05T13:00:00.000Z");
    expect(request?.recurrence).toEqual({ rule: { kind: "interval", minutes: 120 }, timeZone: "UTC", quietPolicy: "defer" });
    await vi.waitFor(() => expect(editor?.textContent).toBe("My unsent draft"));
    expect(root.querySelector("[data-scheduled-edit]")).toBeNull();
    expect(root.querySelector('.send-button[data-action="send"]')).not.toBeNull();
    expect(schedule).not.toHaveBeenCalled();
  });

  it("discards a scheduled message edit and keeps the draft it set aside", async () => {
    const update = vi.spyOn(chat, "updateScheduledOrganizationalMessage");
    const root = await editScheduledFromList(pendingScheduled());
    await typeReply(root, "A change to throw away");

    root.querySelector<HTMLButtonElement>('[data-scheduled-edit-action="discard"]')?.click();
    await tick();

    expect(root.querySelector("[data-chat-composer]")?.textContent).toBe("My unsent draft");
    expect(root.querySelector("[data-scheduled-edit]")).toBeNull();
    expect(update).not.toHaveBeenCalled();
  });

  it("ends the edit and rereads the list when the scheduled message started sending meanwhile", async () => {
    vi.spyOn(chat, "updateScheduledOrganizationalMessage").mockRejectedValue({
      code: "busy",
      message: "This scheduled message is already being sent",
      retryable: true,
    });
    const root = await editScheduledFromList(pendingScheduled());
    const list = vi.mocked(chat.listScheduledOrganizationalMessages);
    const readsBefore = list.mock.calls.length;

    root.querySelector<HTMLButtonElement>('[data-scheduled-edit-action="save"]')?.click();

    await vi.waitFor(() => expect(root.querySelector("[data-chat-composer]")?.textContent).toBe("My unsent draft"));
    expect(root.querySelector("[data-scheduled-edit]")).toBeNull();
    expect(list.mock.calls.length).toBeGreaterThan(readsBefore);
  });

  it("keeps a waiting reply edit to its text so no attachment or reference is dropped on save", async () => {
    const page = workingReplyThread();
    const mention: ChatMessageReference = {
      kind: "participant",
      participantId: agent.id,
      participantKind: "ai_agent",
      metadata: { referenceId: "reference:atlas", labelSnapshot: "Atlas", startOffset: 0, endOffset: 6, plainTextProjection: "@Atlas" },
    };
    const reply = { ...page.rootMessage, itemId: "item:reply", replyThreadId: page.thread.id, normalizedMarkdown: "@Atlas use staging", references: [mention] };
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([waitingInput()]);
    const pick = vi.spyOn(chatApi, "pickChatImages").mockResolvedValue([]);
    const edit = vi.spyOn(chatApi, "editChatAssignmentInput").mockResolvedValue(waitingInput({ currentRevisionId: "revision:edited" }));
    vi.spyOn(chat, "loadReplyThread").mockResolvedValue();
    const root = mountActiveWork({ ...page, replies: [reply] });

    const imageTool = () => root.querySelector<HTMLButtonElement>('button[aria-label="Attach images"]');
    expect(imageTool()?.disabled).toBe(false);
    (await openReplyMenu(root)).querySelector<HTMLButtonElement>('[data-pending-action="edit"]')?.click();
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

  it("explains a waiting reply that reached the agent before the edit was saved", async () => {
    const page = workingReplyThread();
    const reply = { ...page.rootMessage, itemId: "item:reply", replyThreadId: page.thread.id, normalizedMarkdown: "Use the staging bucket" };
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([waitingInput()]);
    vi.spyOn(chatApi, "editChatAssignmentInput").mockRejectedValue({ code: "conflict", message: "This reply was already delivered", recoverable: true, details: { reason: "input_already_delivered" } });
    vi.spyOn(chat, "loadReplyThread").mockResolvedValue();
    const root = mountActiveWork({ ...page, replies: [reply] });

    (await openReplyMenu(root)).querySelector<HTMLButtonElement>('[data-pending-action="edit"]')?.click();
    await tick();
    root.querySelector<HTMLButtonElement>('[data-pending-action="save"]')?.click();

    await vi.waitFor(() => expect(root.textContent).toContain("This reply already reached the agent"));
    expect(root.querySelector('.send-button[data-action="send"]')).not.toBeNull();
  });

  it("offers the agent commands on a line-start slash and runs the chosen one", async () => {
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

    const editItem = (await openReplyMenu(root)).querySelector<HTMLButtonElement>('[data-pending-action="edit"]');
    expect(editItem?.getAttribute("aria-disabled")).toBe("true");
    expect(editItem?.dataset.appTooltip).toBe("Load this reply in the thread to edit it");
    editItem?.click();
    await tick();
    expect(root.querySelector('.send-button[data-action="save"]')).toBeNull();
    inputs.mockResolvedValue([]);
    root.querySelector<HTMLButtonElement>('[data-pending-action="cancel"]')?.click();

    await vi.waitFor(() => expect(cancel).toHaveBeenCalledWith("input:1"));
    await vi.waitFor(() => expect(root.querySelector("[data-chat-pending-inputs]")).toBeNull());
  });

  it("lists each waiting reply with steer, remove, and more actions, and an On hold chip that resumes it", async () => {
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([
      waitingInput(),
      waitingInput({ inputId: "input:2", messageItemId: "item:2", ordinal: 2, held: true, preview: "Compare two layouts" }),
      waitingInput({ inputId: "input:3", messageItemId: "item:3", ordinal: 3, deliveryState: "failed", preview: "Check the logs" }),
    ]);
    const setHeld = vi.spyOn(chatApi, "setChatAssignmentInputHeld").mockResolvedValue(waitingInput({ inputId: "input:2" }));
    vi.spyOn(chat, "loadReplyThread").mockResolvedValue();
    const root = mountActiveWork();

    await vi.waitFor(() => expect(root.querySelectorAll("[data-chat-pending-inputs] li")).toHaveLength(3));
    const [waiting, held, failed] = [...root.querySelectorAll<HTMLLIElement>("[data-chat-pending-inputs] li")];
    expect(root.querySelector("[data-chat-pending-inputs]")?.textContent).not.toContain("Waiting replies");
    expect(waiting?.querySelector(".queued-preview")?.textContent).toBe("Use the staging bucket");
    expect(waiting?.querySelector(".queued-icon")?.getAttribute("data-app-tooltip")).toBe("After this turn");
    expect(waiting?.querySelector('[data-pending-action="resume-chip"]')).toBeNull();
    expect(waiting?.querySelector('[data-pending-action="steer"]')?.getAttribute("aria-label")).toBe("Steer the current turn with reply: Use the staging bucket");
    expect(waiting?.querySelector('[data-pending-action="cancel"]')?.getAttribute("data-app-tooltip")).toBe("Remove from the queue");
    expect(waiting?.querySelector('[data-pending-action="more"]')?.getAttribute("aria-haspopup")).toBe("menu");

    const chip = held?.querySelector<HTMLButtonElement>('[data-pending-action="resume-chip"]');
    expect(chip?.textContent).toBe("On hold");
    expect(chip?.getAttribute("aria-label")).toBe("Resume reply: Compare two layouts");
    expect(chip?.dataset.appTooltip).toBe("Resume");
    expect(held?.querySelector('[data-pending-action="steer"]')).not.toBeNull();

    expect(failed?.querySelector(".queued-icon")?.getAttribute("data-app-tooltip")).toBe("Not delivered");
    expect(failed?.querySelector("button")).toBeNull();

    chip?.click();
    await vi.waitFor(() => expect(setHeld).toHaveBeenCalledWith("input:2", false));
  });

  it("holds, resumes, and steers a waiting reply from its row and menu", async () => {
    const inputs = vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([waitingInput()]);
    const setHeld = vi.spyOn(chatApi, "setChatAssignmentInputHeld").mockResolvedValue(waitingInput({ held: true }));
    const steer = vi.spyOn(chatApi, "steerChatAssignmentInput").mockResolvedValue(waitingInput({ routingKind: "steer" }));
    vi.spyOn(chat, "loadReplyThread").mockResolvedValue();
    const root = mountActiveWork();

    const menu = await openReplyMenu(root);
    expect(menu.querySelector('[data-pending-action="resume"]')).toBeNull();
    inputs.mockResolvedValue([waitingInput({ held: true })]);
    menu.querySelector<HTMLButtonElement>('[data-pending-action="hold"]')?.click();
    await vi.waitFor(() => expect(setHeld).toHaveBeenCalledWith("input:1", true));
    await vi.waitFor(() => expect(root.querySelector('[data-pending-action="resume-chip"]')).not.toBeNull());

    (await openReplyMenu(root)).querySelector<HTMLButtonElement>('[data-pending-action="resume"]')?.click();
    await vi.waitFor(() => expect(setHeld).toHaveBeenLastCalledWith("input:1", false));

    await vi.waitFor(() => expect(root.querySelector<HTMLButtonElement>('[data-pending-action="steer"]')?.disabled).toBe(false));
    root.querySelector<HTMLButtonElement>('[data-pending-action="steer"]')?.click();
    await vi.waitFor(() => expect(steer).toHaveBeenCalledWith("input:1"));
  });

  it("offers no steer for a reply that already steers the current turn", async () => {
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([waitingInput({ routingKind: "steer" })]);
    const root = mountActiveWork();

    await vi.waitFor(() => expect(root.querySelector('[data-pending-action="more"]')).not.toBeNull());
    expect(root.querySelector('[data-pending-action="steer"]')).toBeNull();
    expect(root.querySelector(".queued-icon")?.getAttribute("data-app-tooltip")).toBe("Steers the current turn");
  });

  it("moves a waiting reply down the queue from its menu and keeps the saved order", async () => {
    const page = workingReplyThread();
    const second = waitingInput({ inputId: "input:2", messageItemId: "item:2", ordinal: 2, preview: "Then the docs" });
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([waitingInput(), second]);
    const reorder = vi.spyOn(chatApi, "reorderChatAssignmentInputs").mockResolvedValue([
      { ...second, ordinal: 1 },
      waitingInput({ ordinal: 2 }),
    ]);
    const root = mountActiveWork(page);

    const menu = await openReplyMenu(root);
    const items = [...menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
    const moveUp = items.find((item) => item.textContent?.includes("Move up"));
    const moveDown = items.find((item) => item.textContent?.includes("Move down"));
    expect(moveUp?.getAttribute("aria-disabled")).toBe("true");
    moveDown?.click();

    await vi.waitFor(() => expect(reorder).toHaveBeenCalledWith(page.assignment?.id, ["input:2", "input:1"]));
    await vi.waitFor(() => expect([...root.querySelectorAll(".queued-preview")].map((preview) => preview.textContent))
      .toEqual(["Then the docs", "Use the staging bucket"]));
  });

  it("lists only the replies after the one that started the running work", async () => {
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([
      waitingInput({ inputId: "input:1", ordinal: 1, routingKind: "follow_up", preview: "Again." }),
      waitingInput({ inputId: "input:2", ordinal: 2, preview: "Then the docs" }),
    ]);
    const root = mountActiveWork();

    await vi.waitFor(() => expect([...root.querySelectorAll(".queued-preview")].map((preview) => preview.textContent))
      .toEqual(["Then the docs"]));
  });

  it("explains and rereads a waiting reply that started its own work after the list loaded", async () => {
    const inputs = vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([waitingInput()]);
    vi.spyOn(chatApi, "cancelChatAssignmentInput").mockRejectedValue({ code: "conflict", message: "This reply started its own work; stop that work instead", recoverable: true, details: { reason: "input_starts_work" } });
    vi.spyOn(chat, "loadReplyThread").mockResolvedValue();
    const root = mountActiveWork();

    await vi.waitFor(() => expect(root.querySelector('[data-pending-action="cancel"]')).not.toBeNull());
    const loads = inputs.mock.calls.length;
    root.querySelector<HTMLButtonElement>('[data-pending-action="cancel"]')?.click();

    await vi.waitFor(() => expect(root.textContent).toContain("This reply started its own work, so stop that work to cancel it"));
    expect(inputs.mock.calls.length).toBeGreaterThan(loads);
  });

  it("shows no queue for the reply that started the running work", async () => {
    vi.spyOn(chatApi, "listChatAssignmentInputs").mockResolvedValue([waitingInput({ routingKind: "follow_up" })]);
    const root = mountActiveWork();

    await tick();
    await vi.waitFor(() => expect(chatApi.listChatAssignmentInputs).toHaveBeenCalled());
    await tick();
    expect(root.querySelector("[data-chat-pending-inputs]")).toBeNull();
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
      message: "No device can run this agent, so the changes cannot be requested yet",
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
