// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as chatApi from "$lib/api/chat";
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
  kind: "ai_teammate",
  displayName: "Atlas",
  avatar: { schemaVersion: 1, value: {} },
  revision: 1,
  archivedAt: null,
};

function message(
  itemId: string,
  author: ChatParticipantRead,
  markdown: string,
  ordinal: number,
  agentRunId: string | null = author.kind === "ai_teammate" ? "run:test" : null,
): ChatMessageRead {
  return {
    itemId,
    conversationId: "conversation:test",
    replyThreadId: ordinal === 0 ? null : "reply-thread:test",
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
    ordinal,
    editedAt: null,
    createdAt: `2026-08-04T17:0${ordinal}:00.000Z`,
  };
}

const run: ChatAgentRunRead = {
  id: "run:test",
  assignmentId: "assignment:test",
  projectId: "project:test",
  workingFolderId: "folder:test",
  executionEnvironmentId: "environment:test",
  scratchGenerationId: null,
  teammatePolicyRevisionId: "policy:test",
  effort: "medium",
  providerExecutionTurnId: "turn:test",
  providerExecutionThreadId: "provider-thread:test",
  state: "completed",
  runOrdinal: 1,
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
      modelId: null,
      modelOptions: [],
      modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
      usage: null,
      changedFiles: [],
    }],
    previousCursor: null,
    nextCursor: null,
    threadRevision: 1,
  };
}

const page: ChatReplyThreadPageRead = {
  thread: {
    id: "reply-thread:test",
    replyCount: 1,
    lastActivityAt: "2026-08-04T17:01:00.000Z",
    participants: [agentParticipant],
    unread: false,
    workState: "ready_for_review",
  },
  rootMessage: message("root:test", localParticipant, "Original request", 0),
  replies: [message("reply:test", agentParticipant, "Projected answer", 1)],
  assignment: null,
  agentRuns: [run],
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
});
