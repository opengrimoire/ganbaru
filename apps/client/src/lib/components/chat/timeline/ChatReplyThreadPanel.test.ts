// @vitest-environment jsdom
import { en } from "$lib/i18n/messages/en";

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as chatApi from "$lib/api/chat";
import * as provenanceApi from "$lib/api/chat/provenance";
import type {
  ChatAgentRunRead,
  ChatMessageRead,
  ChatParticipantRead,
  ChatReplyThreadPageRead,
  ChatThreadShellRead,
  ChatTimelinePageRead,
} from "$lib/chat/contracts";
import { getChat } from "$lib/stores/chat.svelte";
import ChatReplyThreadPanel from "./ChatReplyThreadPanel.svelte";

const writeTextToClipboard = vi.hoisted(() => vi.fn<(value: string) => Promise<void>>());
vi.mock("$lib/utils/clipboard", () => ({ writeTextToClipboard }));

class ResizeObserverMock implements ResizeObserver {
  constructor(_callback: ResizeObserverCallback) {}

  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

const localParticipant: ChatParticipantRead = {
  id: "participant:local-owner",
  kind: "local_user",
  displayName: "You",
  avatar: { schemaVersion: 1, value: {} },
  revision: 1,
  archivedAt: null,
};

const agentParticipant: ChatParticipantRead = {
  id: "participant:atlas",
  kind: "ai_agent",
  displayName: "Atlas",
  avatar: { schemaVersion: 1, value: {} },
  revision: 1,
  archivedAt: null,
};

function message(
  itemId: string,
  author: ChatParticipantRead,
  markdown: string,
  position: number,
  agentRunId: string | null = author.kind === "ai_agent" ? "run:test" : null,
): ChatMessageRead {
  return {
    itemId,
    conversationId: "conversation:test",
    replyThreadId: position === 0 ? null : "reply-thread:test",
    revisionId: `revision:${itemId}`,
    revision: 1,
    author,
    authorLabelSnapshot: author.displayName,
    normalizedMarkdown: markdown,
    richContent: {
      schemaVersion: 1,
      value: agentRunId
        ? { type: "agent_update", agentRunId, updateKind: "result" }
        : { type: "message", content: [{ type: "text", text: markdown }] },
    },
    attachmentIds: [],
    references: [],
    replyThread: null,
    orderKey: `c${String(position).padStart(3, "0")}`,
    awaitingDelivery: false,
    editedAt: null,
    createdAt: `2026-08-04T17:0${position}:00.000Z`,
  };
}

const run: ChatAgentRunRead = {
  id: "run:test",
  assignmentId: "assignment:test",
  projectId: "project:test",
  workingFolderId: "folder:test",
  executionEnvironmentId: "environment:test",
  scratchGenerationId: null,
  providerExecutionTurnId: "turn:test",
  providerExecutionThreadId: "provider-thread:test",
  state: "completed",
  runOrdinal: 1,
  attemptReason: "fresh",
  handoffReason: null,
  createdAt: "2026-08-04T17:00:30.000Z",
  updatedAt: "2026-08-04T17:01:00.000Z",
};

function threadShell(
  id: string,
  overrides: Partial<ChatThreadShellRead> = {},
): ChatThreadShellRead {
  return {
    id,
    workingFolderId: "folder:test",
    executionEnvironmentId: "environment:test",
    scratchGenerationId: null,
    projectId: "project:test",
    title: "Execution",
    providerFamilyId: "codex",
    providerInstanceId: "provider:test",
    providerThreadId: null,
    modelId: null,
    modelOptions: [],
    modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
    state: "idle",
    latestTurnState: "completed",
    latestPreview: null,
    messageCount: 1,
    revision: 1,
    lastEventSequence: 3,
    lastActivityAt: "2026-08-04T17:01:00.000Z",
    unreadAt: null,
    archivedAt: null,
    ...overrides,
  };
}

function exactTimelinePage(threadId: string, turnId: string, markdown: string): ChatTimelinePageRead {
  return {
    threadId,
    items: [{
      activityId: `answer:${turnId}`,
      turnId,
      sequenceAnchor: 2,
      changeSequence: 2,
      kind: "message",
      data: {
        schemaVersion: 1,
        value: {
          role: "assistant",
          markdown,
          streamingState: "complete",
          providerItemId: null,
          metadata: { phase: "final_answer" },
          createdAt: "2026-08-04T17:01:00.000Z",
          updatedAt: "2026-08-04T17:01:00.000Z",
        },
      },
      sourceThreadId: threadId,
    }],
    turns: [{
      turnId,
      state: "completed",
      startedAt: "2026-08-04T17:00:30.000Z",
      completedAt: "2026-08-04T17:01:00.000Z",
      stopReason: null,
      errorCode: null,
      modelId: null,
      modelOptions: [],
      modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
      usage: null,
      changedFiles: [],
    }],
    previousCursor: null,
    nextCursor: null,
    threadRevision: 1,
    changeCursor: 0,
  };
}

const page: ChatReplyThreadPageRead = {
  thread: {
    id: "reply-thread:test",
    replyCount: 1,
    lastActivityAt: "2026-08-04T17:01:00.000Z",
    participants: [agentParticipant],
    unread: false,
    unseenFailure: false,
    workState: "ready_for_review",
  },
  rootMessage: message("root:test", localParticipant, "Original request", 0),
  replies: [message("reply:test", agentParticipant, "Projected answer", 1)],
  assignment: null,
  agentRuns: [run],
  delegatedChildren: [],
  previousCursor: null,
  revision: 1,
};

describe("ChatReplyThreadPanel", () => {
  const chat = getChat();
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;
  const originalOpenReplyThreadId = chat.openReplyThreadId;
  const originalReplyThread = chat.replyThread;
  const originalReplyThreadPages = chat.replyThreadPages;
  const originalSelectedExecutionRunId = chat.selectedExecutionRunId;
  const originalSelectedThreadId = chat.selectedThreadId;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    vi.spyOn(chatApi, "listChatSecretRequests").mockResolvedValue([]);
    vi.stubGlobal("CSS", { escape: (value: string) => value });
    vi.stubGlobal("matchMedia", vi.fn(() => ({
      matches: false,
      media: "",
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })));
  });

  afterEach(async () => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    chat.openReplyThreadId = originalOpenReplyThreadId;
    chat.replyThread = originalReplyThread;
    chat.replyThreadPages = originalReplyThreadPages;
    chat.selectedExecutionRunId = originalSelectedExecutionRunId;
    chat.selectedThreadId = originalSelectedThreadId;
  });

  it("never paints a projected agent answer while the exact run is loading", async () => {
    chat.openReplyThreadId = page.thread.id;
    chat.replyThread = page;
    chat.replyThreadPages = [page];
    chat.selectedExecutionRunId = null;
    chat.selectedThreadId = null;
    vi.spyOn(chat, "selectAssignmentExecution").mockReturnValue(new Promise(() => undefined));
    vi.spyOn(chatApi, "readChatTimelineTurn").mockReturnValue(new Promise(() => undefined));
    vi.spyOn(chatApi, "readChatThreadShell").mockReturnValue(new Promise(() => undefined));
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReplyThreadPanel, {
      target,
      props: { onClose: vi.fn() },
    });
    await tick();

    expect(target.textContent).not.toContain("Projected answer");
    expect(target.querySelector('.thread-scroll[aria-busy="true"]')).not.toBeNull();
    expect(target.querySelector('[data-message-item-id="reply:test"]')).toBeNull();
  });

  it("renders the root and replies as one normal dated message flow", async () => {
    const messageOnlyPage = { ...page, agentRuns: [] };
    chat.openReplyThreadId = messageOnlyPage.thread.id;
    chat.replyThread = messageOnlyPage;
    chat.replyThreadPages = [messageOnlyPage];
    chat.selectedExecutionRunId = null;
    chat.selectedThreadId = null;
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReplyThreadPanel, {
      target,
      props: { onClose: vi.fn() },
    });
    await tick();

    expect(target.querySelector(".root-message")).toBeNull();
    expect(target.querySelector(".reply-divider")).toBeNull();
    expect(target.querySelectorAll(".date-divider")).toHaveLength(1);
    expect(target.querySelectorAll('button[aria-label="Copy"]')).toHaveLength(2);
    expect(target.querySelectorAll('button[aria-label="Add reaction"]')).toHaveLength(2);
    expect(target.querySelectorAll('button[aria-label="More Chat actions"]')).toHaveLength(2);
    expect(target.textContent).not.toContain("1 reply");
  });

  it("shows a failed Sent from lookup above the thread until it is dismissed", async () => {
    const messageOnlyPage = { ...page, agentRuns: [] };
    chat.openReplyThreadId = messageOnlyPage.thread.id;
    chat.replyThread = messageOnlyPage;
    chat.replyThreadPages = [messageOnlyPage];
    chat.selectedExecutionRunId = null;
    chat.selectedThreadId = null;
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    vi.spyOn(provenanceApi, "loadChatMessageProvenance").mockRejectedValue({
      code: "persistence",
      message: "Read Chat message provenance",
      retryable: true,
    });
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReplyThreadPanel, {
      target,
      props: { onClose: vi.fn() },
    });
    await tick();

    target.querySelector<HTMLButtonElement>('button[aria-label="More Chat actions"]')?.click();

    await vi.waitFor(() => {
      expect(target?.querySelector('[role="alert"]')?.textContent).toContain(
        "Could not read which devices sent this message",
      );
    });
    expect(target.querySelector('.thread-scroll [role="alert"]')).toBeNull();

    target.querySelector<HTMLButtonElement>('[role="alert"] [data-notice-dismiss]')?.click();
    await tick();
    expect(target.querySelector('[role="alert"]')).toBeNull();
  });

  it("keeps the thread readable when a message action failure closes beside an execution failure", async () => {
    const activePage: ChatReplyThreadPageRead = {
      ...page,
      replies: [],
      agentRuns: [{ ...run, state: "working" }],
    };
    chat.openReplyThreadId = activePage.thread.id;
    chat.replyThread = activePage;
    chat.replyThreadPages = [activePage];
    chat.selectedExecutionRunId = null;
    chat.selectedThreadId = null;
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    vi.spyOn(chat, "selectAssignmentExecution").mockRejectedValue(new Error("The execution could not open"));
    vi.spyOn(provenanceApi, "loadChatMessageProvenance").mockRejectedValue(new Error("lookup failed"));
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReplyThreadPanel, {
      target,
      props: { onClose: vi.fn() },
    });

    await vi.waitFor(() => {
      expect(target?.querySelector('[role="alert"]')?.textContent).toContain(en.chatErrors.actions.workAction);
    });
    expect(target.querySelector("[data-notice-dismiss]")).toBeNull();

    target.querySelector<HTMLButtonElement>('button[aria-label="More Chat actions"]')?.click();
    await vi.waitFor(() => {
      expect(target?.textContent).toContain("Could not read which devices sent this message");
    });
    const alerts = [...target.querySelectorAll('[role="alert"]')];
    expect(alerts).toHaveLength(2);
    expect(target.querySelectorAll("[data-notice-dismiss]")).toHaveLength(1);

    target.querySelector<HTMLButtonElement>("[data-notice-dismiss]")?.click();
    await tick();

    const remaining = [...target.querySelectorAll('[role="alert"]')];
    expect(remaining.map((alert) => alert.textContent)).toEqual([expect.stringContaining(en.chatErrors.actions.workAction)]);
    expect(target.querySelector('[data-message-item-id="root:test"]')).not.toBeNull();
  });

  it("renders exact historical turns across provider thread handoffs", async () => {
    const firstRun = {
      ...run,
      id: "run:first",
      providerExecutionThreadId: "provider-thread:first",
      providerExecutionTurnId: "turn:first",
    };
    const secondRun = {
      ...run,
      id: "run:second",
      providerExecutionThreadId: "provider-thread:second",
      providerExecutionTurnId: "turn:second",
      createdAt: "2026-08-04T17:03:30.000Z",
      updatedAt: "2026-08-04T17:04:00.000Z",
    };
    const handoffPage: ChatReplyThreadPageRead = {
      ...page,
      replies: [
        message("reply:first", agentParticipant, "Projected first answer", 1, firstRun.id),
        message("reply:human", localParticipant, "Continue", 2),
        message("reply:second", agentParticipant, "Projected second answer", 3, secondRun.id),
      ],
      agentRuns: [firstRun, secondRun],
    };
    const timelinePages = new Map([
      [firstRun.providerExecutionTurnId, exactTimelinePage(firstRun.providerExecutionThreadId, firstRun.providerExecutionTurnId, "Exact first answer")],
      [secondRun.providerExecutionTurnId, exactTimelinePage(secondRun.providerExecutionThreadId, secondRun.providerExecutionTurnId, "Exact second answer")],
    ]);
    chat.openReplyThreadId = handoffPage.thread.id;
    chat.replyThread = handoffPage;
    chat.replyThreadPages = [handoffPage];
    chat.selectedExecutionRunId = null;
    chat.selectedThreadId = null;
    vi.spyOn(chat, "selectAssignmentExecution").mockResolvedValue();
    vi.spyOn(chatApi, "readChatTimelineTurn").mockImplementation(async (_threadId, turnId) => {
      const timelinePage = timelinePages.get(turnId);
      if (!timelinePage) throw new Error("Missing test timeline");
      return timelinePage;
    });
    vi.spyOn(chatApi, "readChatThreadShell").mockImplementation(async (threadId) => threadShell(threadId));
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReplyThreadPanel, {
      target,
      props: { onClose: vi.fn() },
    });

    await vi.waitFor(() => {
      expect(target?.textContent).toContain("Exact first answer");
      expect(target?.textContent).toContain("Exact second answer");
    });
    expect(target.textContent).not.toContain("Projected first answer");
    expect(target.textContent).not.toContain("Projected second answer");
    expect(target.querySelectorAll(".chat-execution-timeline")).toHaveLength(2);
    const identityTriggers = [...target.querySelectorAll(".chat-execution-timeline .name-trigger")];
    expect(identityTriggers).toHaveLength(2);
    expect(identityTriggers.map((trigger) => trigger.textContent)).toEqual(["Atlas", "Atlas"]);
    expect(target.querySelector(".agent-badge")).toBeNull();
  });

  it("names the agent and a localized reason when its run fails before any output", async () => {
    const diagnostic = "The provider did not authenticate its internal host tools in time";
    const failedRun: ChatAgentRunRead = { ...run, state: "failed" };
    const unstartedRun: ChatAgentRunRead = {
      ...run,
      id: "run:unstarted",
      providerExecutionThreadId: null,
      providerExecutionTurnId: "turn:unstarted",
      state: "cancelled",
      createdAt: "2026-08-04T17:03:30.000Z",
    };
    const failedPage: ChatReplyThreadPageRead = { ...page, replies: [], agentRuns: [failedRun, unstartedRun] };
    const userOnly = exactTimelinePage(failedRun.providerExecutionThreadId ?? "", failedRun.providerExecutionTurnId, "Hidden request");
    const failedTimeline: ChatTimelinePageRead = {
      ...userOnly,
      items: [{
        activityId: "request:failed",
        turnId: failedRun.providerExecutionTurnId,
        sequenceAnchor: 1,
        changeSequence: 1,
        kind: "message",
        data: {
          schemaVersion: 1,
          value: {
            role: "user",
            markdown: "Hidden request",
            streamingState: "complete",
            providerItemId: null,
            metadata: null,
            createdAt: "2026-08-04T17:00:30.000Z",
            updatedAt: "2026-08-04T17:00:30.000Z",
          },
        },
      }],
      turns: userOnly.turns.map((turn) => ({ ...turn, state: "failed", stopReason: diagnostic, errorCode: "timeout" })),
    };
    chat.openReplyThreadId = failedPage.thread.id;
    chat.replyThread = failedPage;
    chat.replyThreadPages = [failedPage];
    chat.selectedExecutionRunId = null;
    chat.selectedThreadId = null;
    vi.spyOn(chat, "selectAssignmentExecution").mockResolvedValue();
    vi.spyOn(chatApi, "readChatTimelineTurn").mockResolvedValue(failedTimeline);
    vi.spyOn(chatApi, "readChatThreadShell").mockImplementation(async (threadId) => threadShell(threadId));
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReplyThreadPanel, {
      target,
      props: { onClose: vi.fn() },
    });

    await vi.waitFor(() => {
      expect(target?.querySelector('[data-run-outcome="failed"]')).not.toBeNull();
    });
    const failure = target.querySelector('[data-run-outcome="failed"] .notice-message');
    expect(failure?.textContent).toBe(`${en.chat.runs.outcome.failedBeforeOutput("Atlas")} ${en.chatErrors.codes.timeout}`);
    expect(target.textContent).not.toContain(diagnostic);
    expect(target.textContent).not.toContain("Hidden request");
    expect(target.querySelector('[data-run-outcome="stopped"]')?.textContent)
      .toBe(en.chat.runs.outcome.stoppedBeforeOutput("Atlas"));
    expect(target.querySelectorAll(".chat-execution-timeline")).toHaveLength(1);
  });

  it("shows a failure at the bottom of the agent's message with Retry and the technical detail behind copy", async () => {
    const diagnostic = "Provider turn timed out after 600 seconds";
    const failedRun: ChatAgentRunRead = { ...run, providerExecutionThreadId: null, state: "failed" };
    const assignment = {
      id: run.assignmentId,
      replyThreadId: page.thread.id,
      agent: agentParticipant,
      triggeringMessageItemId: "root:test",
      previousAssignmentId: null,
      state: "failed",
      stateReason: "timeout",
      revision: 3,
      settledAt: "2026-08-04T17:02:00.000Z",
      createdAt: "2026-08-04T17:00:00.000Z",
      updatedAt: "2026-08-04T17:02:00.000Z",
      review: null,
      latestRunProviderThreadId: null,
      modelSelection: { providerManagedModel: true, modelId: null, modelOptions: [] },
      instructions: "",
      modelLabel: null,
      delegation: null,
    } satisfies ChatReplyThreadPageRead["assignment"];
    const failureMessage: ChatMessageRead = {
      ...message("failure:test", agentParticipant, diagnostic, 2),
      richContent: {
        schemaVersion: 1,
        value: {
          type: "agent_update",
          updateKind: "failure",
          assignmentId: assignment.id,
          agentRunId: failedRun.id,
          payload: { code: "timeout", safeDetails: null },
        },
      },
    };
    const failedPage: ChatReplyThreadPageRead = {
      ...page,
      replies: [message("progress:test", agentParticipant, "Looking at the build", 1), failureMessage],
      assignment,
      agentRuns: [failedRun],
    };
    chat.openReplyThreadId = failedPage.thread.id;
    chat.replyThread = failedPage;
    chat.replyThreadPages = [failedPage];
    chat.selectedExecutionRunId = null;
    chat.selectedThreadId = null;
    vi.spyOn(chat, "selectAssignmentExecution").mockResolvedValue();
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    const retry = vi.spyOn(chat, "retryAssignment").mockResolvedValue();
    writeTextToClipboard.mockResolvedValue();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReplyThreadPanel, {
      target,
      props: { onClose: vi.fn() },
    });

    await vi.waitFor(() => expect(target?.querySelector("[data-chat-failure]")).not.toBeNull());
    const failure = target.querySelector<HTMLElement>("[data-chat-failure]");
    const failureEntry = failure?.closest<HTMLElement>("[data-message-item-id]");
    expect(failureEntry?.dataset.messageItemId).toBe("failure:test");
    expect(failure?.querySelector(".notice-message")?.textContent)
      .toBe(`${en.chat.runs.outcome.failedAfterOutput("Atlas")} ${en.chatErrors.codes.timeout}`);
    expect(target.textContent).not.toContain(diagnostic);
    expect(target.textContent).not.toContain("The agent stopped because of an error.");
    expect(target.querySelector('[data-run-outcome="failed"]')).toBeNull();
    expect(target.querySelectorAll("[data-notice-retry]")).toHaveLength(1);

    failure?.querySelector<HTMLButtonElement>("[data-notice-copy]")?.click();
    await vi.waitFor(() => expect(writeTextToClipboard).toHaveBeenCalledWith(`timeout: ${diagnostic}`));
    failure?.querySelector<HTMLButtonElement>("[data-notice-retry]")?.click();
    await vi.waitFor(() => expect(retry).toHaveBeenCalledWith(assignment.id));
  });

  it("closes a working run with a Stop working control that cancels only its assignment", async () => {
    const startingRun: ChatAgentRunRead = {
      ...run,
      id: "run:starting",
      assignmentId: "assignment:starting",
      providerExecutionThreadId: null,
      providerExecutionTurnId: "turn:starting",
      state: "starting",
    };
    const workingPage: ChatReplyThreadPageRead = { ...page, replies: [], agentRuns: [startingRun] };
    chat.openReplyThreadId = workingPage.thread.id;
    chat.replyThread = workingPage;
    chat.replyThreadPages = [workingPage];
    chat.selectedExecutionRunId = null;
    chat.selectedThreadId = null;
    vi.spyOn(chat, "selectAssignmentExecution").mockResolvedValue();
    vi.spyOn(chatApi, "readChatThreadShell").mockImplementation(async (threadId) => threadShell(threadId));
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    const cancel = vi.spyOn(chat, "cancelAssignment").mockResolvedValue();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReplyThreadPanel, {
      target,
      props: { onClose: vi.fn() },
    });

    await vi.waitFor(() => {
      expect(target?.querySelector('[data-action="stop-run"]')).not.toBeNull();
    });
    const stop = target.querySelector<HTMLButtonElement>('[data-action="stop-run"]');
    expect(stop?.textContent).toBe(en.chat.runs.stopWorking);
    expect(stop?.title).toContain("Atlas");
    stop?.click();
    await vi.waitFor(() => expect(cancel).toHaveBeenCalledWith("assignment:starting"));
    expect(cancel).toHaveBeenCalledOnce();
    expect(target.querySelector("[data-run-outcome]")).toBeNull();
  });

  it("shows the state of work another linked device runs and stops it from the thread", async () => {
    const remoteWork = {
      id: "assignment:remote",
      replyThreadId: page.thread.id,
      agent: agentParticipant,
      triggeringMessageItemId: "root:test",
      previousAssignmentId: null,
      state: "working",
      stateReason: null,
      revision: 2,
      settledAt: null,
      createdAt: "2026-08-04T17:00:00.000Z",
      updatedAt: "2026-08-04T17:01:00.000Z",
      review: null,
      latestRunProviderThreadId: null,
      modelSelection: { providerManagedModel: true, modelId: null, modelOptions: [] },
      instructions: "",
      modelLabel: null,
      delegation: null,
    } satisfies ChatReplyThreadPageRead["assignment"];
    const remotePage: ChatReplyThreadPageRead = { ...page, replies: [], assignment: remoteWork, agentRuns: [] };
    chat.openReplyThreadId = remotePage.thread.id;
    chat.replyThread = remotePage;
    chat.replyThreadPages = [remotePage];
    chat.selectedExecutionRunId = null;
    chat.selectedThreadId = null;
    vi.spyOn(chat, "selectAssignmentExecution").mockResolvedValue();
    vi.spyOn(chatApi, "readChatThreadShell").mockImplementation(async (threadId) => threadShell(threadId));
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    const cancel = vi.spyOn(chat, "cancelAssignment").mockResolvedValue();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReplyThreadPanel, {
      target,
      props: { onClose: vi.fn() },
    });

    await vi.waitFor(() => {
      expect(target?.querySelector('[data-work-state="working"]')).not.toBeNull();
    });
    expect(target.querySelector('[data-work-state="working"] [role="status"]')?.textContent)
      .toBe(en.chat.delegation.states.working);
    const remoteWorkRow = target.querySelector('[data-work-state="working"]');
    expect(remoteWorkRow?.querySelector("header strong")?.textContent).toContain("Atlas");
    expect(remoteWorkRow?.querySelector("time")?.getAttribute("datetime")).toBe(remoteWork.createdAt);
    const stop = target.querySelector<HTMLButtonElement>('[data-work-state="working"] [data-action="stop-run"]');
    expect(stop?.title).toContain("Atlas");
    stop?.click();
    await vi.waitFor(() => expect(cancel).toHaveBeenCalledWith("assignment:remote"));
    expect(cancel).toHaveBeenCalledOnce();
  });

  it("exposes private scratch only when the reply thread has a scratch run", async () => {
    const scratchThreadId = "provider-thread:scratch";
    const scratchRun = {
      ...run,
      workingFolderId: null,
      scratchGenerationId: "scratch-generation:test",
      providerExecutionThreadId: scratchThreadId,
      state: "working" as const,
    };
    const scratchPage = { ...page, agentRuns: [scratchRun] };
    chat.openReplyThreadId = scratchPage.thread.id;
    chat.replyThread = scratchPage;
    chat.replyThreadPages = [scratchPage];
    chat.selectedExecutionRunId = null;
    chat.selectedThreadId = null;
    vi.spyOn(chat, "selectAssignmentExecution").mockResolvedValue();
    vi.spyOn(chatApi, "readChatTimelineTurn").mockResolvedValue(
      exactTimelinePage(
        scratchThreadId,
        scratchRun.providerExecutionTurnId,
        "Scratch result",
      ),
    );
    vi.spyOn(chatApi, "readChatThreadShell").mockResolvedValue(
      threadShell(scratchThreadId, {
        workingFolderId: null,
        executionEnvironmentId: scratchRun.executionEnvironmentId,
        scratchGenerationId: scratchRun.scratchGenerationId,
      }),
    );
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReplyThreadPanel, {
      target,
      props: { onClose: vi.fn() },
    });

    await vi.waitFor(() => {
      expect(target?.querySelector('button[aria-label="Inspect private scratch"]')).not.toBeNull();
      expect(target?.textContent).toContain("Scratch result");
    });
  });

  it("shows delegated work and where it came from, and fills the composer from a suggested answer", async () => {
    const assignment = {
      id: "assignment:test",
      replyThreadId: page.thread.id,
      agent: agentParticipant,
      triggeringMessageItemId: "root:test",
      previousAssignmentId: null,
      state: "waiting_for_answer",
      stateReason: null,
      revision: 2,
      settledAt: null,
      createdAt: "2026-08-04T17:00:00.000Z",
      updatedAt: "2026-08-04T17:01:00.000Z",
      review: null,
      latestRunProviderThreadId: null,
      modelSelection: { providerManagedModel: true, modelId: null, modelOptions: [] },
      instructions: "",
      modelLabel: null,
      delegation: {
        parentAssignmentId: "assignment:parent",
        parentReplyThreadId: "reply-thread:parent",
        depth: 1,
        objective: "Check the build",
        expectedResult: "A short report",
      },
    } satisfies ChatReplyThreadPageRead["assignment"];
    const question = {
      ...message("question:test", agentParticipant, "Which branch?", 1),
      richContent: {
        schemaVersion: 1,
        value: {
          type: "agent_update",
          updateKind: "question",
          assignmentId: assignment.id,
          agentRunId: "run:test",
          payload: { question: "Which branch?", choices: ["main", "dev"], source: "host_tool" },
        },
      },
    };
    const delegatedPage: ChatReplyThreadPageRead = {
      ...page,
      replies: [question],
      assignment,
      agentRuns: [],
      delegatedChildren: [{
        assignmentId: "assignment:child",
        parentAssignmentId: assignment.id,
        replyThreadId: "reply-thread:child",
        agent: agentParticipant,
        objective: "Run the tests",
        state: "working",
        stateReason: null,
        createdAt: "2026-08-04T17:02:00.000Z",
      }],
    };
    chat.openReplyThreadId = delegatedPage.thread.id;
    chat.replyThread = delegatedPage;
    chat.replyThreadPages = [delegatedPage];
    chat.selectedExecutionRunId = null;
    chat.selectedThreadId = null;
    vi.spyOn(chat, "listScheduledOrganizationalMessages").mockResolvedValue([]);
    const openReplyThread = vi.spyOn(chat, "openReplyThread").mockResolvedValue();
    const setDraft = vi.spyOn(chat, "setOrganizationalDraft");
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReplyThreadPanel, {
      target,
      props: { onClose: vi.fn() },
    });
    await tick();

    const origin = target.querySelector<HTMLElement>("[data-delegation-origin]");
    expect(origin?.textContent).toContain("Check the build");
    expect(origin?.textContent).toContain("A short report");
    origin?.querySelector("button")?.click();
    expect(openReplyThread).toHaveBeenCalledWith("reply-thread:parent");

    const child = target.querySelector<HTMLElement>('[data-delegated-child="assignment:child"]');
    expect(child?.textContent).toContain("Run the tests");
    expect(child?.textContent).toContain("Working");
    child?.querySelector("button")?.click();
    expect(openReplyThread).toHaveBeenCalledWith("reply-thread:child");

    target.querySelectorAll<HTMLButtonElement>("[data-answer-choices] button")[1]?.click();
    expect(setDraft).toHaveBeenCalledWith(`reply-thread:${delegatedPage.thread.id}`, expect.objectContaining({
      normalizedMarkdown: "dev",
      selectionStart: 3,
      selectionEnd: 3,
    }));
  });
});
