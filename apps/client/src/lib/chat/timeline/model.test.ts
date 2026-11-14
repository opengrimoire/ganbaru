import { describe, expect, it, vi } from "vitest";
import type { ChangedFileSummary, ChatTimelineItemRead, ChatTimelineTurnRead, ChatTurnState, JsonValue, ThreadUsageUpdatedEvent } from "$lib/chat/contracts";
import { buildTimelineDisplayRows, includeOptimisticTimelineMessage, parseTimelineUserContext, projectTimelineReadModel, stabilizeTimelineRows, timelineActivityShowsLiveStatus, timelineActivitySupportsDisclosure, timelineFinishedRenderKey, timelineModelGroupStartIds, timelineRowsForTurn, type TimelineDisplayRow, type TimelineRow } from "./model";
import { indexTimelineRowsByTurn } from "./row-index";

vi.mock("./row-index", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./row-index")>();
  return { ...actual, indexTimelineRowsByTurn: vi.fn(actual.indexTimelineRowsByTurn) };
});

const start = "2026-07-21T14:00:00.000Z";

interface ItemOptions {
  turnId?: string | null;
  createdAt?: string;
  metadata?: JsonValue;
}

function itemRead(activityId: string, sequence: number, kind: string, value: Record<string, JsonValue>, options: ItemOptions): ChatTimelineItemRead {
  const createdAt = options.createdAt ?? start;
  return {
    activityId,
    turnId: options.turnId === undefined ? "turn-1" : options.turnId,
    sequenceAnchor: sequence,
    changeSequence: sequence,
    kind,
    data: { schemaVersion: 1, value: { ...value, ...(options.metadata === undefined ? {} : { metadata: options.metadata }), createdAt, updatedAt: createdAt } },
  };
}

function messageRead(
  activityId: string,
  sequence: number,
  markdown: string,
  options: ItemOptions & { role?: "user" | "assistant"; streamingState?: string } = {},
): ChatTimelineItemRead {
  return itemRead(activityId, sequence, "message", { role: options.role ?? "assistant", markdown, streamingState: options.streamingState ?? "complete" }, options);
}

function activityRead(
  activityId: string,
  sequence: number,
  activityKind: string,
  status: string,
  title: string,
  options: ItemOptions & { detail?: string | null } = {},
): ChatTimelineItemRead {
  return itemRead(activityId, sequence, "activity", { activityKind, status, title, detailPreview: options.detail ?? null }, options);
}

function planRead(activityId: string, sequence: number, markdown: string, state: string, steps: JsonValue[] = []): ChatTimelineItemRead {
  return itemRead(activityId, sequence, "plan", { markdown, state, steps }, {});
}

function turnRead(
  state: ChatTurnState,
  options: { turnId?: string; completedAt?: string | null; modelId?: string | null; usage?: ThreadUsageUpdatedEvent | null; changedFiles?: ChangedFileSummary[] } = {},
): ChatTimelineTurnRead {
  const terminal = state === "completed" || state === "interrupted" || state === "failed";
  return {
    turnId: options.turnId ?? "turn-1",
    state,
    startedAt: start,
    completedAt: options.completedAt === undefined ? (terminal ? start : null) : options.completedAt,
    stopReason: terminal ? "end_turn" : null,
    errorCode: null,
    modelId: options.modelId === undefined ? "gpt-5" : options.modelId,
    modelOptions: [],
    modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
    usage: options.usage ?? null,
    changedFiles: options.changedFiles ?? [],
  };
}

function displayRows(items: readonly ChatTimelineItemRead[], turns: readonly ChatTimelineTurnRead[]) {
  const projection = projectTimelineReadModel(items, turns);
  return buildTimelineDisplayRows(projection.rows, projection.turns);
}

describe("timeline read model projection", () => {
  it("keeps real actions inspectable even before the provider reports detail", () => {
    const [command, thinking] = projectTimelineReadModel([
      activityRead("activity:command", 1, "command_execution", "active", "pnpm test", { turnId: null }),
      activityRead("activity:thinking", 2, "reasoning", "active", "Reasoning", { turnId: null }),
    ], []).rows;

    expect(command?.kind === "activity" && timelineActivitySupportsDisclosure(command)).toBe(true);
    expect(thinking?.kind === "activity" && timelineActivitySupportsDisclosure(thinking)).toBe(false);
  });

  it("keeps the current completed thinking placeholder visually live until its turn settles", () => {
    const items = [activityRead("activity:thinking", 2, "reasoning", "completed", "Reasoning")];
    const active = projectTimelineReadModel(items, [turnRead("active")]);
    const thinking = active.rows.find((row) => row.kind === "activity" && row.activityKind === "reasoning");

    expect(thinking?.kind === "activity" && timelineActivityShowsLiveStatus(thinking, active.turns[0]?.state)).toBe(true);

    const settled = projectTimelineReadModel(items, [turnRead("completed")]);
    const settledThinking = settled.rows.find((row) => row.kind === "activity" && row.activityKind === "reasoning");

    expect(settledThinking?.kind === "activity" && timelineActivityShowsLiveStatus(settledThinking, settled.turns[0]?.state)).toBe(false);
  });

  it("keeps durable checkpoint restores as ungrouped thread-level notices", () => {
    const restore = activityRead("activity:restore", 1, "notice", "completed", "thread_reverted", {
      turnId: null,
      metadata: { checkpointId: "checkpoint:1", revertedTurnCount: 1, providerHistoryAction: "fork_required" },
    });
    const projection = projectTimelineReadModel([restore], []);

    expect(projection.rows).toMatchObject([{
      kind: "activity",
      title: "thread_reverted",
      status: "completed",
      metadata: { value: { revertedTurnCount: 1 } },
    }]);
    expect(buildTimelineDisplayRows(projection.rows, projection.turns)).toMatchObject([{ id: "activity:restore", kind: "activity" }]);
    const [row] = projection.rows;
    expect(row?.kind === "activity" && timelineActivitySupportsDisclosure(row)).toBe(false);
  });

  it("attaches duration, model, usage, and deduplicated changed files to the final answer", () => {
    const changed = { relativePath: "src/app.ts", previousRelativePath: null, additions: 4, deletions: 1, binary: false, status: "modified" };
    const usage = { inputTokens: 10, outputTokens: 4, cachedInputTokens: 2, cacheWriteInputTokens: null, contextTokens: 16, contextLimit: 100, cost: null, countScope: null };
    const projection = projectTimelineReadModel(
      [messageRead("message:answer", 2, "Done", { streamingState: "streaming" })],
      [turnRead("completed", { completedAt: "2026-07-21T14:00:12.500Z", modelId: "gpt-5.1", usage, changedFiles: [changed, changed] })],
    );

    expect(projection.rows.find((row) => row.id === "message:answer")).toMatchObject({
      state: "complete",
      metadata: {
        durationMs: 12_500,
        modelId: "gpt-5.1",
        changedFiles: [changed],
        usage: { inputTokens: 10, outputTokens: 4 },
      },
    });
  });

  it("marks the final answer of an interrupted or failed turn with the turn outcome", () => {
    const items = [messageRead("message:answer", 2, "Partial", { streamingState: "streaming" })];

    expect(projectTimelineReadModel(items, [turnRead("interrupted")]).rows).toMatchObject([{ state: "interrupted" }]);
    expect(projectTimelineReadModel(items, [turnRead("failed")]).rows).toMatchObject([{ state: "failed" }]);
    expect(projectTimelineReadModel(items, [turnRead("active")]).rows).toMatchObject([{ state: "streaming", metadata: null }]);
  });

  it("projects stored activity and plan rows", () => {
    const projection = projectTimelineReadModel([
      activityRead("activity:command", 1, "command_execution", "completed", "Run tests", { detail: "Passed" }),
      planRead("plan:proposed", 2, "Plan", "complete", [{ id: "one", text: "Test", status: "completed" }, { id: "broken" }]),
      planRead("plan:draft", 3, "Draft", "proposed"),
    ], [turnRead("completed")]);

    expect(projection.rows).toMatchObject([
      { id: "activity:command", status: "completed", detailPreview: "Passed" },
      { id: "plan:proposed", kind: "plan", markdown: "Plan", state: "complete", steps: [{ id: "one", text: "Test", status: "completed" }] },
      { id: "plan:draft", kind: "plan", state: "streaming", steps: [] },
    ]);
  });

  it("shows pending work until meaningful provider output arrives", () => {
    const echo = activityRead("provider-user-echo", 1, "user_message", "completed", "User message", { detail: "Please continue" });
    const pending = projectTimelineReadModel([echo], [turnRead("active")]);

    expect(pending.rows).toMatchObject([{ id: "turn-pending:turn-1", status: "active" }]);
    expect(projectTimelineReadModel(
      [echo, activityRead("message:answer", 2, "assistant_message", "active", "Assistant response")],
      [turnRead("active")],
    ).rows.some((row) => row.id === "turn-pending:turn-1")).toBe(true);
    expect(projectTimelineReadModel(
      [echo, messageRead("message:answer", 2, "Starting", { streamingState: "streaming" })],
      [turnRead("active")],
    ).rows.some((row) => row.id === "turn-pending:turn-1")).toBe(false);
    expect(projectTimelineReadModel(
      [echo, activityRead("activity:command", 2, "command_execution", "active", "pnpm test")],
      [turnRead("active")],
    ).rows.some((row) => row.id === "turn-pending:turn-1")).toBe(false);
    expect(projectTimelineReadModel([], [turnRead("completed")]).rows).toEqual([]);
  });

  it("folds settled work while leaving the final assistant answer visible", () => {
    const items = [
      activityRead("activity:thinking-one", 2, "reasoning", "completed", "Reasoning"),
      activityRead("activity:command", 3, "command_execution", "completed", "Run tests", { detail: "Passed" }),
      activityRead("activity:thinking-two", 4, "reasoning", "completed", "Reasoning"),
      activityRead("message:commentary", 5, "assistant_message", "completed", "Assistant response", { detail: "I checked the tests." }),
      activityRead("activity:file", 6, "file_change", "completed", "Edit file", { detail: "Changed" }),
      messageRead("message:answer", 7, "Finished", { streamingState: "streaming" }),
    ];
    const projection = projectTimelineReadModel(items, [turnRead("interrupted", { completedAt: "2026-07-21T14:00:05.000Z" })]);

    const folded = buildTimelineDisplayRows(projection.rows, projection.turns);
    expect(folded).toMatchObject([
      { id: "turn-fold:turn-1", state: "interrupted", durationMs: 5_000, expanded: false, hiddenRows: [{ id: "activity-group:activity:command", latest: { id: "activity:command" } }, { id: "message:commentary", kind: "message" }, { id: "activity-group:activity:file", latest: { id: "activity:file" } }] },
      { id: "message:answer", state: "interrupted", markdown: "Finished" },
    ]);

    const expanded = buildTimelineDisplayRows(projection.rows, projection.turns, new Set(["turn-1"]));
    expect(expanded.map((row) => row.id)).toEqual(["turn-fold:turn-1", "message:answer"]);
    expect(expanded[0]).toMatchObject({ expanded: true, hiddenRows: [{ id: "activity-group:activity:command", latest: { id: "activity:command" } }, { id: "message:commentary", markdown: "I checked the tests." }, { id: "activity-group:activity:file", latest: { id: "activity:file" } }] });
  });

  it("uses the provider final-answer phase instead of chronological position", () => {
    const items = [
      activityRead("message:commentary-one", 2, "assistant_message", "completed", "Assistant response", { detail: "Checking the result.", metadata: { phase: "commentary" } }),
      activityRead("message:answer", 3, "assistant_message", "completed", "Assistant response", { detail: "The final answer.", metadata: { phase: "final_answer" } }),
      activityRead("message:commentary-two", 4, "assistant_message", "completed", "Assistant response", { detail: "Late provider commentary.", metadata: { phase: "commentary" } }),
      activityRead("activity:command", 5, "command_execution", "completed", "pnpm test", { detail: "Passed" }),
    ];

    expect(displayRows(items, [turnRead("completed", { completedAt: "2026-07-21T14:00:05.000Z" })])).toMatchObject([
      {
        id: "turn-fold:turn-1",
        hiddenRows: [
          { id: "message:commentary-one" },
          { id: "message:commentary-two" },
          { id: "activity-group:activity:command", latest: { id: "activity:command" } },
        ],
      },
      { id: "message:answer", markdown: "The final answer.", metadata: { durationMs: 5_000 } },
    ]);
  });

  it("accumulates real live work and creates the work disclosure after settlement", () => {
    const thinkingOne = activityRead("activity:thinking-one", 2, "reasoning", "active", "Reasoning");
    const commentary = activityRead("message:commentary", 3, "assistant_message", "completed", "Assistant response", { detail: "I am checking this." });
    const thinkingTwo = activityRead("activity:thinking-two", 4, "reasoning", "active", "Reasoning");
    const activeCommand = activityRead("activity:command", 5, "command_execution", "active", "pnpm test");
    const live = [turnRead("active")];

    expect(displayRows([thinkingOne], live)).toMatchObject([{ id: "activity:thinking-one", kind: "activity" }]);
    expect(displayRows([thinkingOne, commentary], live)).toMatchObject([
      { id: "message:commentary", kind: "message", markdown: "I am checking this." },
    ]);
    expect(displayRows([thinkingOne, commentary, thinkingTwo], live)).toMatchObject([
      { id: "message:commentary", kind: "message" },
      { id: "activity:thinking-two", kind: "activity" },
    ]);
    const working = displayRows([thinkingOne, commentary, thinkingTwo, activeCommand], live);
    expect(working).toMatchObject([
      { id: "message:commentary", kind: "message" },
      { id: "activity-group:activity:command", kind: "activity_group", latest: { id: "activity:command", status: "active" } },
    ]);
    expect(working.some((row) => row.kind === "turn_fold")).toBe(false);

    expect(displayRows([
      activityRead("activity:thinking-one", 2, "reasoning", "completed", "Reasoning"),
      commentary,
      activityRead("activity:thinking-two", 4, "reasoning", "completed", "Reasoning"),
      activityRead("activity:command", 5, "command_execution", "completed", "pnpm test", { detail: "Passed" }),
      activityRead("activity:thinking-three", 6, "reasoning", "completed", "Reasoning"),
      activityRead("message:answer", 7, "assistant_message", "completed", "Assistant response", { detail: "All checks passed." }),
    ], [turnRead("completed")])).toMatchObject([
      {
        id: "turn-fold:turn-1",
        state: "completed",
        hiddenRows: [
          { id: "message:commentary" },
          { id: "activity-group:activity:command", latest: { id: "activity:command" } },
        ],
      },
      { id: "message:answer", markdown: "All checks passed." },
    ]);
  });

  it("omits a completed process disclosure when the provider only reports private reasoning", () => {
    const rows = displayRows([
      activityRead("activity:thinking", 2, "reasoning", "completed", "Reasoning"),
      activityRead("message:answer", 4, "assistant_message", "completed", "Assistant response", { detail: "Hi.", metadata: { phase: "final_answer" } }),
    ], [turnRead("completed")]);

    expect(rows).toMatchObject([{ id: "message:answer", kind: "message", markdown: "Hi." }]);
    expect([...timelineModelGroupStartIds(rows)]).toEqual(["message:answer"]);
  });

  it("does not promote earlier commentary to a final answer when work ends with an action", () => {
    expect(displayRows([
      activityRead("message:commentary", 2, "assistant_message", "completed", "Assistant response", { detail: "I will check this." }),
      activityRead("activity:command", 3, "command_execution", "completed", "pnpm test", { detail: "Passed" }),
    ], [turnRead("completed")])).toMatchObject([
      {
        id: "turn-fold:turn-1",
        hiddenRows: [
          { id: "message:commentary" },
          { id: "activity-group:activity:command", latest: { id: "activity:command" } },
        ],
      },
    ]);
  });

  it("shows an optimistic user message only until its durable row arrives", () => {
    const optimistic = {
      id: "message-pending",
      kind: "message" as const,
      role: "user" as const,
      turnId: "turn-pending",
      sequence: 2,
      createdAt: start,
      markdown: "Visible immediately",
      state: "pending" as const,
      phase: null,
      userContext: null,
      metadata: null,
    };
    const existing = [{ ...optimistic, id: "message-existing", markdown: "Earlier", state: "complete" as const }];

    expect(includeOptimisticTimelineMessage(existing, optimistic).map((row) => row.id)).toEqual([
      "message-existing",
      "message-pending",
    ]);
    expect(includeOptimisticTimelineMessage([...existing, optimistic], optimistic).filter((row) => row.id === optimistic.id)).toHaveLength(1);
  });

  it("limits embedded execution history to one exact turn", () => {
    const projection = projectTimelineReadModel([
      activityRead("activity:old", 1, "command_execution", "completed", "Old command", { turnId: "turn-old" }),
      activityRead("activity:current", 2, "command_execution", "completed", "Current command", { turnId: "turn-current" }),
    ], []);

    expect(timelineRowsForTurn(projection.rows, "turn-current").map((row) => row.id)).toEqual(["activity:current"]);
    expect(timelineRowsForTurn(projection.rows, null)).toEqual(projection.rows);
  });

  it("groups consecutive settled activities with stable row IDs", () => {
    const projection = projectTimelineReadModel([
      activityRead("activity:one", 1, "command_execution", "completed", "One", { turnId: null }),
      activityRead("activity:two", 2, "file_change", "completed", "Two", { turnId: null }),
    ], []);
    expect(buildTimelineDisplayRows(projection.rows, projection.turns)).toMatchObject([
      { id: "activity-group:activity:one", latest: { id: "activity:two" }, earlierRows: [{ id: "activity:one" }] },
    ]);
    const groupId = "activity-group:activity:one";
    expect(buildTimelineDisplayRows(projection.rows, projection.turns, new Set(), new Set([groupId]))).toMatchObject([
      { id: groupId, expanded: true, earlierRows: [{ id: "activity:one" }], latest: { id: "activity:two" } },
    ]);
  });

  it("keeps completed and active consecutive tools in one expandable live group", () => {
    const commandOne = activityRead("activity:command-one", 2, "command_execution", "completed", "pnpm check", { detail: "Passed" });
    const readMetadata = { toolInput: { file_path: "src/chat.ts" } };
    const groupId = "activity-group:activity:command-one";
    const live = projectTimelineReadModel([
      commandOne,
      activityRead("activity:read", 3, "dynamic_tool_call", "active", "Read", { metadata: readMetadata }),
    ], [turnRead("active")]);

    expect(buildTimelineDisplayRows(live.rows, live.turns, new Set(), new Set([groupId]))).toMatchObject([{
      id: groupId,
      kind: "activity_group",
      earlierRows: [{ id: "activity:command-one", status: "completed" }],
      latest: { id: "activity:read", status: "active" },
      expanded: true,
    }]);

    const continued = projectTimelineReadModel([
      commandOne,
      activityRead("activity:read", 3, "dynamic_tool_call", "completed", "Read", { metadata: readMetadata }),
      activityRead("activity:command-two", 5, "command_execution", "active", "pnpm test"),
    ], [turnRead("active")]);

    expect(buildTimelineDisplayRows(continued.rows, continued.turns, new Set(), new Set([groupId]))).toMatchObject([{
      id: groupId,
      earlierRows: [
        { id: "activity:command-one", status: "completed" },
        { id: "activity:read", status: "completed" },
      ],
      latest: { id: "activity:command-two", status: "active" },
      expanded: true,
    }]);
  });

  it("starts a new tool group after assistant commentary", () => {
    expect(displayRows([
      activityRead("activity:command", 2, "command_execution", "completed", "pnpm check", { detail: "Passed" }),
      activityRead("message:commentary", 3, "assistant_message", "completed", "Assistant response", { detail: "Now I will edit the file.", metadata: { phase: "commentary" } }),
      activityRead("activity:edit", 4, "file_change", "active", "Edit file"),
    ], [turnRead("active")])).toMatchObject([
      { id: "activity-group:activity:command", latest: { id: "activity:command" } },
      { id: "message:commentary", kind: "message" },
      { id: "activity-group:activity:edit", latest: { id: "activity:edit", status: "active" } },
    ]);
  });

  it("wraps a single settled action so its provider detail stays inside a semantic disclosure", () => {
    expect(displayRows([
      activityRead("activity:command", 1, "command_execution", "completed", "pnpm test", { turnId: null, detail: "Passed" }),
    ], [])).toMatchObject([{
      id: "activity-group:activity:command",
      kind: "activity_group",
      earlierRows: [],
      latest: { id: "activity:command", title: "pnpm test" },
      expanded: false,
    }]);
  });

  it("keeps a matching late assistant message visible after turn settlement", () => {
    const projection = projectTimelineReadModel(
      [messageRead("message:late-answer", 3, "Recovered final answer", { streamingState: "streaming", createdAt: "2026-07-21T14:00:03.000Z" })],
      [turnRead("completed", { completedAt: "2026-07-21T14:00:02.000Z" })],
    );

    expect(projection.rows).toMatchObject([
      { id: "message:late-answer", markdown: "Recovered final answer", state: "complete", metadata: { durationMs: 2_000 } },
    ]);
    expect(buildTimelineDisplayRows(projection.rows, projection.turns)).toMatchObject([{ id: "message:late-answer" }]);
  });

  it("keeps a stored preview shortened and counts UTF-8 bytes when the size is absent", () => {
    const tail = "ok\n".repeat(4);
    const items: ChatTimelineItemRead[] = [{
      activityId: "activity:command",
      turnId: "turn-1",
      sequenceAnchor: 1,
      changeSequence: 1,
      kind: "activity",
      data: { schemaVersion: 1, value: { activityKind: "command_execution", status: "active", title: "Run tests", detailPreview: tail, detailBytes: 9_000, detailTruncated: true, providerItemId: "command", metadata: {}, createdAt: start, updatedAt: "2026-07-21T14:00:05.000Z" } },
    }];
    const projection = projectTimelineReadModel(items, [turnRead("active")]);
    expect(projection.rows.find((row) => row.id === "activity:command")).toMatchObject({ detailPreview: tail, detailBytes: 9_000, detailTruncated: true, updatedAt: "2026-07-21T14:00:05.000Z" });

    const whole = projectTimelineReadModel([
      activityRead("activity:whole", 1, "command_execution", "completed", "Run tests", { turnId: null, detail: "é✓𝄞" }),
    ], []).rows[0];
    expect(whole).toMatchObject({ detailPreview: "é✓𝄞", detailBytes: 9, detailTruncated: false });
  });


  it("offers a disclosure only when an activity has detail bytes or metadata", () => {
    const base = { id: "activity:tool", kind: "activity", turnId: "turn-1", sequence: 1, createdAt: start, activityKind: "dynamic_tool_call", status: "completed", title: "Lookup", metadata: null } as const;
    expect(timelineActivitySupportsDisclosure({ ...base, detailPreview: null, detailBytes: 0, detailTruncated: false })).toBe(false);
    expect(timelineActivitySupportsDisclosure({ ...base, detailPreview: "", detailBytes: 5_000, detailTruncated: true })).toBe(true);
  });

  it("hydrates the same display model from validated paged SQLite projections", () => {
    const items: ChatTimelineItemRead[] = [
      {
        activityId: "provider-user-echo",
        turnId: "turn-1",
        sequenceAnchor: 1,
        changeSequence: 1,
        kind: "activity",
        data: { schemaVersion: 1, value: { activityKind: "user_message", status: "completed", title: "User message", detailPreview: null, providerItemId: "provider-user", metadata: {}, createdAt: start, updatedAt: start } },
      },
      {
        activityId: "message-1",
        turnId: "turn-1",
        sequenceAnchor: 2,
        changeSequence: 2,
        kind: "message",
        data: { schemaVersion: 1, value: { role: "assistant", markdown: "Persisted", streamingState: "complete", providerItemId: "provider-item", metadata: {}, createdAt: start, updatedAt: start } },
      },
      {
        activityId: "message-1",
        turnId: "turn-1",
        sequenceAnchor: 3,
        changeSequence: 3,
        kind: "activity",
        data: { schemaVersion: 1, value: { activityKind: "assistant_message", status: "completed", title: "Assistant response", detailPreview: "Lifecycle copy", providerItemId: "provider-item", metadata: {}, createdAt: start, updatedAt: start } },
      },
    ];
    const turns: ChatTimelineTurnRead[] = [{
      turnId: "turn-1",
      state: "completed",
      startedAt: start,
      completedAt: "2026-07-21T14:00:02.000Z",
      stopReason: "end_turn",
      errorCode: null,
      modelId: "gpt-5",
      modelOptions: [],
      modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
      usage: null,
      changedFiles: [],
    }];

    expect(projectTimelineReadModel(items, turns).rows).toMatchObject([
      { id: "message-1", markdown: "Persisted", state: "complete", metadata: { durationMs: 2_000, modelId: "gpt-5" } },
    ]);
  });

  it("renders persisted assistant lifecycle commentary as Markdown inside expanded work", () => {
    const items: ChatTimelineItemRead[] = [
      {
        activityId: "commentary",
        turnId: "turn-1",
        sequenceAnchor: 2,
        changeSequence: 2,
        kind: "activity",
        data: { schemaVersion: 1, value: { activityKind: "assistant_message", status: "completed", title: "Assistant response", detailPreview: "A normal **paragraph**.", providerItemId: "commentary", metadata: {}, createdAt: start, updatedAt: start } },
      },
      {
        activityId: "command",
        turnId: "turn-1",
        sequenceAnchor: 3,
        changeSequence: 3,
        kind: "activity",
        data: { schemaVersion: 1, value: { activityKind: "command_execution", status: "completed", title: "Run tests", detailPreview: "Passed", providerItemId: "command", metadata: {}, createdAt: start, updatedAt: start } },
      },
      {
        activityId: "answer",
        turnId: "turn-1",
        sequenceAnchor: 4,
        changeSequence: 4,
        kind: "message",
        data: { schemaVersion: 1, value: { role: "assistant", markdown: "Final answer", streamingState: "complete", providerItemId: "answer", metadata: {}, createdAt: start, updatedAt: start } },
      },
    ];
    const turns: ChatTimelineTurnRead[] = [{
      turnId: "turn-1",
      state: "completed",
      startedAt: start,
      completedAt: "2026-07-21T14:00:02.000Z",
      stopReason: "end_turn",
      errorCode: null,
      modelId: "gpt-5",
      modelOptions: [],
      modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
      usage: null,
      changedFiles: [],
    }];

    const projection = projectTimelineReadModel(items, turns);
    expect(buildTimelineDisplayRows(projection.rows, projection.turns, new Set(["turn-1"]))).toMatchObject([
      {
        id: "turn-fold:turn-1",
        hiddenRows: [
          { id: "commentary", kind: "message", markdown: "A normal **paragraph**." },
          { id: "activity-group:command", kind: "activity_group", latest: { id: "command" } },
        ],
      },
      { id: "answer", kind: "message", markdown: "Final answer" },
    ]);
  });

  it("starts one model participant group after the user message in each turn", () => {
    const rows = displayRows([
      messageRead("message:prompt", 2, "Question", { role: "user" }),
      activityRead("activity:command", 3, "command_execution", "completed", "Checked files"),
      activityRead("message:answer", 4, "assistant_message", "completed", "Assistant response", { detail: "Answer" }),
    ], [turnRead("completed")]);

    expect([...timelineModelGroupStartIds(rows)]).toEqual(["turn-fold:turn-1"]);
  });
  it("bounds and validates durable user context metadata", () => {
    const context = parseTimelineUserContext({
      attachments: [
        { attachmentId: "attachment-1", displayName: "diagram.png", kind: "image", byteSize: 1024, status: "persisted" },
        { id: "predecessor-image", displayName: "screenshot.png", mimeType: "image/png", byteSize: 2048, status: "managed" },
        { attachmentId: "incomplete", displayName: "incomplete.png", kind: "image" },
        { filename: 42 },
      ],
      mentions: [{ relativePath: "src/main.ts", kind: "file" }, { relativePath: null }],
      terminalContext: [
        { attachmentId: "terminal-1", displayName: "Focused terminal selection", byteSize: 512 },
        { attachmentId: "terminal-incomplete", displayName: "Missing size" },
        "pnpm test",
        { label: "Old terminal selection" },
      ],
      preCheckpointId: "checkpoint-1",
      checkpointId: "old-checkpoint",
    });

    expect(context).toEqual({
      attachments: [
        { attachmentId: "attachment-1", displayName: "diagram.png", kind: "image", byteSize: 1024, status: "persisted" },
      ],
      mentions: [{ relativePath: "src/main.ts", kind: "file" }],
      terminalContext: ["Focused terminal selection"],
      preCheckpointId: "checkpoint-1",
    });

    expect(parseTimelineUserContext({
      attachments: [{ id: "old", filename: "old.png", mimeType: "image/png" }],
      terminalContext: ["old terminal", { label: "Old terminal selection" }],
      checkpointId: "old-checkpoint",
    })).toBeNull();
  });
});

describe("timeline row stabilization", () => {
  function stableDisplay(items: readonly ChatTimelineItemRead[], turns: readonly ChatTimelineTurnRead[], previousRows: readonly TimelineRow[], previousDisplay: readonly TimelineDisplayRow[], expandedTurns: ReadonlySet<string> = new Set()) {
    const projection = projectTimelineReadModel(items, turns);
    const rows = stabilizeTimelineRows(previousRows, projection.rows);
    const display = stabilizeTimelineRows(previousDisplay, buildTimelineDisplayRows(rows, projection.turns, expandedTurns));
    return { rows, display };
  }

  const settled = [
    messageRead("message:user", 1, "Run the tests", { role: "user" }),
    activityRead("activity:command", 2, "command_execution", "completed", "pnpm test", { detail: "Passed" }),
    activityRead("activity:lint", 3, "command_execution", "completed", "pnpm lint", { detail: "Clean" }),
    messageRead("message:answer", 4, "All checks passed."),
  ];
  const completed = [turnRead("completed", { completedAt: "2026-07-21T14:00:05.000Z" })];

  it("returns the previous arrays when a refresh changes nothing", () => {
    const first = stableDisplay(settled, completed, [], []);
    const second = stableDisplay(settled, completed, first.rows, first.display);

    expect(second.rows).toBe(first.rows);
    expect(second.display).toBe(first.display);
  });

  it("replaces only the row whose change sequence advanced", () => {
    const first = stableDisplay(settled, completed, [], []);
    const edited = settled.map((item) => item.activityId === "message:answer"
      ? { ...messageRead("message:answer", 4, "All checks passed again."), changeSequence: 9 }
      : item);
    const second = stableDisplay(edited, completed, first.rows, first.display);

    expect(second.display).not.toBe(first.display);
    expect(second.display[0]).toBe(first.display[0]);
    expect(second.display[1]).toBe(first.display[1]);
    expect(second.display[2]).not.toBe(first.display[2]);
    expect(second.display[2]).toMatchObject({ id: "message:answer", markdown: "All checks passed again." });
  });

  it("returns reused rows in the new order when rows move", () => {
    const first = stabilizeTimelineRows([], projectTimelineReadModel(settled, completed).rows);
    const reordered = stabilizeTimelineRows(first, [...projectTimelineReadModel(settled, completed).rows].reverse());

    expect(reordered).not.toBe(first);
    expect(reordered.map((row) => row.id)).toEqual(first.map((row) => row.id).reverse());
    reordered.forEach((row, index) => expect(row).toBe(first[first.length - 1 - index]));
  });

  it("replaces a group whose children changed and keeps its unchanged siblings", () => {
    const active = [turnRead("active")];
    const commentary = activityRead("message:commentary", 2, "assistant_message", "completed", "Assistant response", { detail: "Checking." });
    const running = [commentary, activityRead("activity:command", 3, "command_execution", "active", "pnpm test")];
    const first = stableDisplay(running, active, [], []);
    const finished = stableDisplay(
      [commentary, { ...activityRead("activity:command", 3, "command_execution", "completed", "pnpm test", { detail: "Passed" }), changeSequence: 8 }],
      active,
      first.rows,
      first.display,
    );
    expect(finished.display[0]).toBe(first.display[0]);
    expect(finished.display[1]).not.toBe(first.display[1]);
    expect(finished.display[1]).toMatchObject({ kind: "activity_group", latest: { status: "completed" } });

    const grown = stableDisplay(
      [...running, activityRead("activity:lint", 4, "command_execution", "active", "pnpm lint")],
      active,
      first.rows,
      first.display,
    );
    expect(grown.display[0]).toBe(first.display[0]);
    expect(grown.display[1]).not.toBe(first.display[1]);
    expect(grown.display[1]).toMatchObject({ kind: "activity_group", earlierRows: [{ id: "activity:command" }], latest: { id: "activity:lint" } });
  });

  it("gives the assistant answer a new object when its turn settles without an item change", () => {
    const items = [messageRead("message:answer", 2, "Done.")];
    const first = stableDisplay(items, [turnRead("active")], [], []);
    const second = stableDisplay(items, completed, first.rows, first.display);

    const firstAnswer = first.rows.find((row) => row.id === "message:answer");
    const secondAnswer = second.rows.find((row) => row.id === "message:answer");
    expect(secondAnswer).not.toBe(firstAnswer);
    expect(secondAnswer).toMatchObject({ metadata: { durationMs: 5_000 } });
  });

  it("reuses an unchanged pending work row", () => {
    const items = [messageRead("message:user", 1, "Start", { role: "user" })];
    const first = stableDisplay(items, [turnRead("active")], [], []);
    const second = stableDisplay(items, [turnRead("active")], first.rows, first.display);

    expect(first.display.some((row) => row.id === "turn-pending:turn-1")).toBe(true);
    expect(second.display).toBe(first.display);
  });

  it("replaces a turn fold when it expands and keeps its hidden rows", () => {
    const first = stableDisplay(settled, completed, [], []);
    const expanded = stableDisplay(settled, completed, first.rows, first.display, new Set(["turn-1"]));
    const firstFold = first.display.find((row) => row.kind === "turn_fold");
    const expandedFold = expanded.display.find((row) => row.kind === "turn_fold");

    expect(expandedFold).not.toBe(firstFold);
    expect(expandedFold).toMatchObject({ expanded: true });
    expect(expanded.display.find((row) => row.id === "message:answer")).toBe(first.display.find((row) => row.id === "message:answer"));
  });

  it("indexes rows by turn once per projection", () => {
    const turns = Array.from({ length: 200 }, (_, turnIndex) => turnRead("completed", { turnId: `turn-${turnIndex}` }));
    const items = turns.flatMap((turn, turnIndex) => Array.from({ length: 25 }, (_, rowIndex) => {
      const sequence = turnIndex * 25 + rowIndex + 1;
      return rowIndex === 24
        ? messageRead(`message:${sequence}`, sequence, "Answer", { turnId: turn.turnId })
        : activityRead(`activity:${sequence}`, sequence, "command_execution", "completed", "pnpm test", { turnId: turn.turnId, detail: "Passed" });
    }));
    vi.mocked(indexTimelineRowsByTurn).mockClear();

    const projection = projectTimelineReadModel(items, turns);

    expect(projection.rows).toHaveLength(5_000);
    expect(indexTimelineRowsByTurn).toHaveBeenCalledTimes(1);
  });
});

describe("finished Markdown render keys", () => {
  it("names only stored text that can no longer grow", () => {
    const [message] = projectTimelineReadModel([messageRead("message:answer", 4, "Done")], [turnRead("completed")]).rows;
    const [streaming] = projectTimelineReadModel([messageRead("message:answer", 4, "Do", { streamingState: "streaming" })], [turnRead("active")]).rows;
    const [plan] = projectTimelineReadModel([planRead("plan:one", 5, "1. Step", "complete")], [turnRead("completed")]).rows;

    expect(message?.kind === "message" && timelineFinishedRenderKey(message)).toBe("message:answer:4");
    expect(streaming?.kind === "message" && timelineFinishedRenderKey(streaming)).toBeUndefined();
    expect(plan?.kind === "plan" && timelineFinishedRenderKey(plan)).toBe("plan:one:5");
    if (message?.kind === "message") expect(timelineFinishedRenderKey({ ...message, changeSequence: undefined })).toBeUndefined();
  });
});
