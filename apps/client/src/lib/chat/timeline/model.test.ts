import { describe, expect, it } from "vitest";
import type { CanonicalEvent, CanonicalStoredEvent, ChatTimelineItemRead, ChatTimelineTurnRead } from "$lib/chat/contracts";
import { buildTimelineDisplayRows, includeOptimisticTimelineMessage, parseTimelineUserContext, projectCanonicalTimeline, projectTimelineReadModel, timelineActivityShowsLiveStatus, timelineActivitySupportsDisclosure, timelineModelGroupStartIds, timelineRowsForTurn } from "./model";

const start = "2026-07-21T14:00:00.000Z";

function stored(sequence: number, event: CanonicalEvent, options: { eventId?: string; turnId?: string | null; createdAt?: string } = {}): CanonicalStoredEvent {
  return {
    schemaVersion: 1,
    eventId: options.eventId ?? `event-${sequence}`,
    providerFamilyId: "codex",
    providerInstanceId: "codex-work",
    threadId: "thread-1",
    createdAt: options.createdAt ?? start,
    turnId: options.turnId ?? "turn-1",
    providerTurnId: "provider-turn-1",
    providerItemId: null,
    providerRequestId: null,
    providerTaskId: null,
    providerReference: null,
    event,
    redactedDiagnostic: null,
    sequence,
    ingestedAt: options.createdAt ?? start,
  };
}

describe("canonical timeline projection", () => {
  it("keeps real actions inspectable even before the provider reports detail", () => {
    const command = projectCanonicalTimeline([
      stored(1, { type: "item_started", payload: { itemId: "command", kind: "command_execution", status: "active", title: "pnpm test", detail: null, safeMetadata: null } }),
    ]).rows[0];
    const thinking = projectCanonicalTimeline([
      stored(1, { type: "item_started", payload: { itemId: "thinking", kind: "reasoning", status: "active", title: "Reasoning", detail: null, safeMetadata: null } }),
    ]).rows[0];

    expect(command?.kind === "activity" && timelineActivitySupportsDisclosure(command)).toBe(true);
    expect(thinking?.kind === "activity" && timelineActivitySupportsDisclosure(thinking)).toBe(false);
  });

  it("keeps the current completed thinking placeholder visually live until its turn settles", () => {
    const events = [
      stored(1, { type: "turn_started", payload: { providerTurnId: "provider-turn-1", state: "active", modes: { safetyMode: "ask_for_approval", interactionMode: "build" }, modelId: "gpt-5", modelOptions: [] } }),
      stored(2, { type: "item_completed", payload: { itemId: "thinking", kind: "reasoning", status: "completed", title: "Reasoning", detail: null, safeMetadata: null } }),
    ];
    const active = projectCanonicalTimeline(events);
    const thinking = active.rows.find((row) => row.kind === "activity" && row.activityKind === "reasoning");

    expect(thinking?.kind === "activity" && timelineActivityShowsLiveStatus(thinking, active.turns[0]?.state)).toBe(true);

    const settled = projectCanonicalTimeline([
      ...events,
      stored(3, { type: "turn_completed", payload: { state: "completed", stopReason: "end_turn", usage: null, changedFiles: [] } }),
    ]);
    const settledThinking = settled.rows.find((row) => row.kind === "activity" && row.activityKind === "reasoning");

    expect(settledThinking?.kind === "activity" && timelineActivityShowsLiveStatus(settledThinking, settled.turns[0]?.state)).toBe(false);
  });

  it("projects public reasoning summaries without retaining raw reasoning text in the activity detail", () => {
    const projection = projectCanonicalTimeline([
      stored(1, { type: "item_started", payload: { itemId: "thinking", kind: "reasoning", status: "active", title: "Reasoning", detail: null, safeMetadata: null } }),
      stored(2, { type: "content_delta", payload: { itemId: "thinking", streamKind: "reasoning_text", contentIndex: 0, delta: "Private provider reasoning" } }),
      stored(3, { type: "content_delta", payload: { itemId: "thinking", streamKind: "reasoning_summary", contentIndex: 0, delta: "Checking attachment rendering" } }),
      stored(4, { type: "item_completed", payload: { itemId: "thinking", kind: "reasoning", status: "completed", title: "Reasoning", detail: null, safeMetadata: null } }),
    ]);
    const thinking = projection.rows.find((row) => row.id === "activity:thinking");

    expect(thinking).toMatchObject({
      kind: "activity",
      activityKind: "reasoning_summary",
      detail: "Checking attachment rendering",
    });
  });

  it("projects durable checkpoint restores as thread-level notices", () => {
    const projection = projectCanonicalTimeline([
      stored(1, {
        type: "thread_reverted",
        payload: {
          checkpointId: "checkpoint:1",
          revertedTurnIds: ["turn:2"],
          providerHistoryAction: "fork_required",
        },
      }, { turnId: null }),
    ]);
    expect(projection.rows).toMatchObject([{
      kind: "activity",
      title: "thread_reverted",
      status: "completed",
      metadata: { value: { revertedTurnIds: ["turn:2"] } },
    }]);
  });

  it("keeps multiple assistant items and orders content parts by provider content index", () => {
    const events = [
      stored(4, { type: "content_delta", payload: { itemId: "assistant-a", streamKind: "assistant_text", contentIndex: 1, delta: "world" } }),
      stored(2, { type: "content_delta", payload: { itemId: "assistant-a", streamKind: "assistant_text", contentIndex: 0, delta: "Hello " } }),
      stored(3, { type: "content_delta", payload: { itemId: "assistant-b", streamKind: "assistant_text", contentIndex: 0, delta: "Second answer" } }),
      stored(5, { type: "content_delta", payload: { itemId: "assistant-a", streamKind: "assistant_text", contentIndex: 0, delta: "there " } }),
      stored(6, { type: "content_delta", payload: { itemId: "assistant-a", streamKind: "assistant_text", contentIndex: 0, delta: "ignored" } }, { eventId: "event-5" }),
    ];

    const projection = projectCanonicalTimeline(events);
    expect(projection.rows.filter((row) => row.kind === "message")).toMatchObject([
      { id: "message:assistant-a", markdown: "Hello there world" },
      { id: "message:assistant-b", markdown: "Second answer" },
    ]);
    expect(projection.ignoredDuplicateEventIds).toEqual(["event-5"]);
  });

  it("attaches duration, effective model, usage, and deduplicated changed files to the final answer", () => {
    const changed = { relativePath: "src/app.ts", previousRelativePath: null, additions: 4, deletions: 1, binary: false, status: "modified" };
    const events = [
      stored(1, { type: "turn_started", payload: { providerTurnId: "provider-turn-1", state: "active", modes: { safetyMode: "ask_for_approval", interactionMode: "build" }, modelId: "gpt-5", modelOptions: [] } }),
      stored(2, { type: "content_delta", payload: { itemId: "answer", streamKind: "assistant_text", contentIndex: 0, delta: "Done" } }),
      stored(3, { type: "diff_updated", payload: { source: "provider", files: [changed], providerDiff: null } }),
      stored(4, { type: "model_rerouted", payload: { requestedModelId: "gpt-5", effectiveModelId: "gpt-5.1", reason: "Capacity" } }),
      stored(5, { type: "turn_completed", payload: { state: "completed", stopReason: "end_turn", usage: { inputTokens: 10, outputTokens: 4, cachedInputTokens: 2, contextTokens: 16, contextLimit: 100, cost: null }, changedFiles: [changed] } }, { createdAt: "2026-07-21T14:00:12.500Z" }),
    ];

    const projection = projectCanonicalTimeline(events);
    const answer = projection.rows.find((row) => row.id === "message:answer");
    expect(answer).toMatchObject({
      state: "complete",
      metadata: {
        durationMs: 12_500,
        modelId: "gpt-5.1",
        changedFiles: [changed],
        diffSources: ["provider"],
        usage: { inputTokens: 10, outputTokens: 4 },
      },
    });
  });

  it("projects lifecycle activity, plans, and pending request resolution", () => {
    const events = [
      stored(1, { type: "item_started", payload: { itemId: "command", kind: "command_execution", status: "active", title: "Run tests", detail: null, safeMetadata: null } }),
      stored(2, { type: "content_delta", payload: { itemId: "command", streamKind: "command_output", contentIndex: 0, delta: "Passed" } }),
      stored(3, { type: "item_completed", payload: { itemId: "command", kind: "command_execution", status: "completed", title: "Run tests", detail: null, safeMetadata: null } }),
      stored(4, { type: "plan_updated", payload: { markdown: "Plan", steps: [{ id: "one", text: "Test", status: "completed" }] } }),
      stored(5, { type: "request_opened", payload: { requestId: "request-1", kind: "command_execution", title: "Run command?", detail: null, allowedDecisions: [], safePayload: { schemaVersion: 1, value: {} } } }),
      stored(6, { type: "request_resolved", payload: { requestId: "request-1", state: "resolved", decision: null } }),
    ];

    const projection = projectCanonicalTimeline(events);
    expect(projection.rows).toEqual(expect.arrayContaining([
      expect.objectContaining({ id: "activity:command", status: "completed", detail: "Passed" }),
      expect.objectContaining({ id: "plan:structured:turn-1", markdown: "Plan" }),
    ]));
    expect(projection.pendingRequests).toMatchObject([{ id: "request-1", state: "resolved" }]);
  });

  it("ignores provider user-message echoes and shows pending work until meaningful output arrives", () => {
    const started = stored(1, { type: "turn_started", payload: { providerTurnId: "provider-turn-1", state: "active", modes: { safetyMode: "ask_for_approval", interactionMode: "build" }, modelId: "gpt-5", modelOptions: [] } });
    const user = stored(2, { type: "item_completed", payload: { itemId: "user", kind: "user_message", status: "completed", title: null, detail: "Please continue", safeMetadata: null } });
    const pending = projectCanonicalTimeline([started, user]);
    expect(pending.rows).toMatchObject([
      { id: "turn-pending:turn-1", status: "active" },
    ]);
    expect(pending.rows.some((row) => row.id === "message:user")).toBe(false);
    expect(projectCanonicalTimeline([
      started,
      user,
      stored(3, { type: "item_started", payload: { itemId: "answer", kind: "assistant_message", status: "active", title: null, detail: null, safeMetadata: null } }),
    ]).rows.some((row) => row.id === "turn-pending:turn-1")).toBe(true);
    expect(projectCanonicalTimeline([
      started,
      user,
      stored(3, { type: "content_delta", payload: { itemId: "answer", streamKind: "assistant_text", contentIndex: 0, delta: "Starting" } }),
    ]).rows.some((row) => row.id === "turn-pending:turn-1")).toBe(false);
  });

  it("folds settled work while leaving the final assistant answer visible", () => {
    const events = [
      stored(1, { type: "turn_started", payload: { providerTurnId: "provider-turn-1", state: "active", modes: { safetyMode: "ask_for_approval", interactionMode: "build" }, modelId: "gpt-5", modelOptions: [] } }),
      stored(2, { type: "item_completed", payload: { itemId: "thinking-one", kind: "reasoning", status: "completed", title: "Reasoning", detail: null, safeMetadata: null } }),
      stored(3, { type: "item_completed", payload: { itemId: "command", kind: "command_execution", status: "completed", title: "Run tests", detail: "Passed", safeMetadata: null } }),
      stored(4, { type: "item_completed", payload: { itemId: "thinking-two", kind: "reasoning", status: "completed", title: "Reasoning", detail: null, safeMetadata: null } }),
      stored(5, { type: "item_completed", payload: { itemId: "commentary", kind: "assistant_message", status: "completed", title: null, detail: "I checked the tests.", safeMetadata: null } }),
      stored(6, { type: "item_completed", payload: { itemId: "file", kind: "file_change", status: "completed", title: "Edit file", detail: "Changed", safeMetadata: null } }),
      stored(7, { type: "content_delta", payload: { itemId: "answer", streamKind: "assistant_text", contentIndex: 0, delta: "Finished" } }),
      stored(8, { type: "turn_aborted", payload: { state: "interrupted", reason: "User stopped", recoverable: true } }, { createdAt: "2026-07-21T14:00:05.000Z" }),
    ];
    const projection = projectCanonicalTimeline(events);

    const folded = buildTimelineDisplayRows(projection.rows, projection.turns);
    expect(folded).toMatchObject([
      { id: "turn-fold:turn-1", state: "interrupted", durationMs: 5_000, hiddenRows: [{ id: "activity-group:activity:command", latest: { id: "activity:command" } }, { id: "message:commentary", kind: "message" }, { id: "activity-group:activity:file", latest: { id: "activity:file" } }] },
      { id: "message:answer", state: "interrupted", markdown: "Finished" },
    ]);

    const expanded = buildTimelineDisplayRows(projection.rows, projection.turns, new Set(["turn-1"]));
    expect(expanded.map((row) => row.id)).toEqual([
      "turn-fold:turn-1",
      "message:answer",
    ]);
    expect(expanded[0]).toMatchObject({ expanded: true, hiddenRows: [{ id: "activity-group:activity:command", latest: { id: "activity:command" } }, { id: "message:commentary", markdown: "I checked the tests." }, { id: "activity-group:activity:file", latest: { id: "activity:file" } }] });
  });

  it("uses the provider final-answer phase instead of chronological position", () => {
    const projection = projectCanonicalTimeline([
      stored(1, { type: "turn_started", payload: { providerTurnId: "provider-turn-1", state: "active", modes: { safetyMode: "ask_for_approval", interactionMode: "build" }, modelId: "gpt-5", modelOptions: [] } }),
      stored(2, { type: "item_completed", payload: { itemId: "commentary-one", kind: "assistant_message", status: "completed", title: null, detail: "Checking the result.", safeMetadata: { schemaVersion: 1, value: { phase: "commentary" } } } }),
      stored(3, { type: "item_completed", payload: { itemId: "answer", kind: "assistant_message", status: "completed", title: null, detail: "The final answer.", safeMetadata: { schemaVersion: 1, value: { phase: "final_answer" } } } }),
      stored(4, { type: "item_completed", payload: { itemId: "commentary-two", kind: "assistant_message", status: "completed", title: null, detail: "Late provider commentary.", safeMetadata: { schemaVersion: 1, value: { phase: "commentary" } } } }),
      stored(5, { type: "item_completed", payload: { itemId: "command", kind: "command_execution", status: "completed", title: "pnpm test", detail: "Passed", safeMetadata: null } }),
      stored(6, { type: "turn_completed", payload: { state: "completed", stopReason: "end_turn", usage: null, changedFiles: [] } }, { createdAt: "2026-07-21T14:00:05.000Z" }),
    ]);

    expect(buildTimelineDisplayRows(projection.rows, projection.turns)).toMatchObject([
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
    const events = [
      stored(1, { type: "turn_started", payload: { providerTurnId: "provider-turn-1", state: "active", modes: { safetyMode: "ask_for_approval", interactionMode: "build" }, modelId: "gpt-5", modelOptions: [] } }),
      stored(2, { type: "item_started", payload: { itemId: "thinking-one", kind: "reasoning", status: "active", title: "Reasoning", detail: null, safeMetadata: null } }),
      stored(3, { type: "item_completed", payload: { itemId: "commentary", kind: "assistant_message", status: "completed", title: null, detail: "I am checking this.", safeMetadata: null } }),
      stored(4, { type: "item_started", payload: { itemId: "thinking-two", kind: "reasoning", status: "active", title: "Reasoning", detail: null, safeMetadata: null } }),
      stored(5, { type: "item_started", payload: { itemId: "command", kind: "command_execution", status: "active", title: "pnpm test", detail: null, safeMetadata: null } }),
    ];

    expect(buildTimelineDisplayRows(projectCanonicalTimeline(events.slice(0, 2)).rows, projectCanonicalTimeline(events.slice(0, 2)).turns)).toMatchObject([
      { id: "activity:thinking-one", kind: "activity" },
    ]);
    expect(buildTimelineDisplayRows(projectCanonicalTimeline(events.slice(0, 3)).rows, projectCanonicalTimeline(events.slice(0, 3)).turns)).toMatchObject([
      { id: "message:commentary", kind: "message", markdown: "I am checking this." },
    ]);
    expect(buildTimelineDisplayRows(projectCanonicalTimeline(events.slice(0, 4)).rows, projectCanonicalTimeline(events.slice(0, 4)).turns)).toMatchObject([
      { id: "message:commentary", kind: "message" },
      { id: "activity:thinking-two", kind: "activity" },
    ]);
    expect(buildTimelineDisplayRows(projectCanonicalTimeline(events).rows, projectCanonicalTimeline(events).turns)).toMatchObject([
      { id: "message:commentary", kind: "message" },
      { id: "activity-group:activity:command", kind: "activity_group", latest: { id: "activity:command", status: "active" } },
    ]);
    expect(buildTimelineDisplayRows(projectCanonicalTimeline(events).rows, projectCanonicalTimeline(events).turns).some((row) => row.kind === "turn_fold")).toBe(false);

    const settled = projectCanonicalTimeline([
      ...events,
      stored(6, { type: "item_completed", payload: { itemId: "command", kind: "command_execution", status: "completed", title: "pnpm test", detail: "Passed", safeMetadata: null } }),
      stored(7, { type: "item_completed", payload: { itemId: "thinking-three", kind: "reasoning", status: "completed", title: "Reasoning", detail: null, safeMetadata: null } }),
      stored(8, { type: "item_completed", payload: { itemId: "answer", kind: "assistant_message", status: "completed", title: null, detail: "All checks passed.", safeMetadata: null } }),
      stored(9, { type: "turn_completed", payload: { state: "completed", stopReason: "end_turn", usage: null, changedFiles: [] } }),
    ]);
    expect(buildTimelineDisplayRows(settled.rows, settled.turns)).toMatchObject([
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
    const projection = projectCanonicalTimeline([
      stored(1, { type: "turn_started", payload: { providerTurnId: "provider-turn-1", state: "active", modes: { safetyMode: "ask_for_approval", interactionMode: "build" }, modelId: "gpt-5", modelOptions: [] } }),
      stored(2, { type: "item_started", payload: { itemId: "thinking", kind: "reasoning", status: "active", title: "Reasoning", detail: null, safeMetadata: null } }),
      stored(3, { type: "item_completed", payload: { itemId: "thinking", kind: "reasoning", status: "completed", title: "Reasoning", detail: null, safeMetadata: null } }),
      stored(4, { type: "item_completed", payload: { itemId: "answer", kind: "assistant_message", status: "completed", title: null, detail: "Hi.", safeMetadata: { schemaVersion: 1, value: { phase: "final_answer" } } } }),
      stored(5, { type: "turn_completed", payload: { state: "completed", stopReason: "end_turn", usage: null, changedFiles: [] } }),
    ]);
    const displayRows = buildTimelineDisplayRows(projection.rows, projection.turns);

    expect(displayRows).toMatchObject([{ id: "message:answer", kind: "message", markdown: "Hi." }]);
    expect([...timelineModelGroupStartIds(displayRows)]).toEqual(["message:answer"]);
  });

  it("does not promote earlier commentary to a final answer when work ends with an action", () => {
    const projection = projectCanonicalTimeline([
      stored(1, { type: "turn_started", payload: { providerTurnId: "provider-turn-1", state: "active", modes: { safetyMode: "ask_for_approval", interactionMode: "build" }, modelId: "gpt-5", modelOptions: [] } }),
      stored(2, { type: "item_completed", payload: { itemId: "commentary", kind: "assistant_message", status: "completed", title: null, detail: "I will check this.", safeMetadata: null } }),
      stored(3, { type: "item_completed", payload: { itemId: "command", kind: "command_execution", status: "completed", title: "pnpm test", detail: "Passed", safeMetadata: null } }),
      stored(4, { type: "turn_completed", payload: { state: "completed", stopReason: "end_turn", usage: null, changedFiles: [] } }),
    ]);

    expect(buildTimelineDisplayRows(projection.rows, projection.turns)).toMatchObject([
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
    const projection = projectCanonicalTimeline([
      stored(1, { type: "item_completed", payload: { itemId: "old", kind: "command_execution", status: "completed", title: "Old command", detail: null, safeMetadata: null } }, { turnId: "turn-old" }),
      stored(2, { type: "item_completed", payload: { itemId: "current", kind: "command_execution", status: "completed", title: "Current command", detail: null, safeMetadata: null } }, { turnId: "turn-current" }),
    ]);

    expect(timelineRowsForTurn(projection.rows, "turn-current").map((row) => row.id))
      .toEqual(["activity:current"]);
    expect(timelineRowsForTurn(projection.rows, null)).toEqual(projection.rows);
  });

  it("groups consecutive settled activities with stable row IDs", () => {
    const events = [
      stored(1, { type: "item_completed", payload: { itemId: "one", kind: "command_execution", status: "completed", title: "One", detail: null, safeMetadata: null } }, { turnId: null }),
      stored(2, { type: "item_completed", payload: { itemId: "two", kind: "file_change", status: "completed", title: "Two", detail: null, safeMetadata: null } }, { turnId: null }),
    ];
    const projection = projectCanonicalTimeline(events);
    expect(buildTimelineDisplayRows(projection.rows, projection.turns)).toMatchObject([
      {
        id: "activity-group:activity:one",
        latest: { id: "activity:two" },
        earlierRows: [{ id: "activity:one" }],
      },
    ]);
    const groupId = "activity-group:activity:one";
    expect(buildTimelineDisplayRows(projection.rows, projection.turns, new Set(), new Set([groupId]))).toMatchObject([
      { id: groupId, expanded: true, earlierRows: [{ id: "activity:one" }], latest: { id: "activity:two" } },
    ]);
  });

  it("keeps completed and active consecutive tools in one expandable live group", () => {
    const events = [
      stored(1, { type: "turn_started", payload: { providerTurnId: "provider-turn-1", state: "active", modes: { safetyMode: "ask_for_approval", interactionMode: "build" }, modelId: "gpt-5", modelOptions: [] } }),
      stored(2, { type: "item_completed", payload: { itemId: "command-one", kind: "command_execution", status: "completed", title: "pnpm check", detail: "Passed", safeMetadata: null } }),
      stored(3, { type: "item_started", payload: { itemId: "read", kind: "dynamic_tool_call", status: "active", title: "Read", detail: null, safeMetadata: { schemaVersion: 1, value: { toolInput: { file_path: "src/chat.ts" } } } } }),
    ];
    const live = projectCanonicalTimeline(events);
    const groupId = "activity-group:activity:command-one";

    expect(buildTimelineDisplayRows(live.rows, live.turns, new Set(), new Set([groupId]))).toMatchObject([{
      id: groupId,
      kind: "activity_group",
      earlierRows: [{ id: "activity:command-one", status: "completed" }],
      latest: { id: "activity:read", status: "active" },
      expanded: true,
    }]);

    const continued = projectCanonicalTimeline([
      ...events,
      stored(4, { type: "item_completed", payload: { itemId: "read", kind: "dynamic_tool_call", status: "completed", title: "Read", detail: null, safeMetadata: { schemaVersion: 1, value: { toolInput: { file_path: "src/chat.ts" } } } } }),
      stored(5, { type: "item_started", payload: { itemId: "command-two", kind: "command_execution", status: "active", title: "pnpm test", detail: null, safeMetadata: null } }),
    ]);

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
    const projection = projectCanonicalTimeline([
      stored(1, { type: "turn_started", payload: { providerTurnId: "provider-turn-1", state: "active", modes: { safetyMode: "ask_for_approval", interactionMode: "build" }, modelId: "gpt-5", modelOptions: [] } }),
      stored(2, { type: "item_completed", payload: { itemId: "command", kind: "command_execution", status: "completed", title: "pnpm check", detail: "Passed", safeMetadata: null } }),
      stored(3, { type: "item_completed", payload: { itemId: "commentary", kind: "assistant_message", status: "completed", title: null, detail: "Now I will edit the file.", safeMetadata: { schemaVersion: 1, value: { phase: "commentary" } } } }),
      stored(4, { type: "item_started", payload: { itemId: "edit", kind: "file_change", status: "active", title: "Edit file", detail: null, safeMetadata: null } }),
    ]);

    expect(buildTimelineDisplayRows(projection.rows, projection.turns)).toMatchObject([
      { id: "activity-group:activity:command", latest: { id: "activity:command" } },
      { id: "message:commentary", kind: "message" },
      { id: "activity-group:activity:edit", latest: { id: "activity:edit", status: "active" } },
    ]);
  });

  it("wraps a single settled action so its provider detail stays inside a semantic disclosure", () => {
    const projection = projectCanonicalTimeline([
      stored(1, { type: "item_completed", payload: { itemId: "command", kind: "command_execution", status: "completed", title: "pnpm test", detail: "Passed", safeMetadata: null } }, { turnId: null }),
    ]);

    expect(buildTimelineDisplayRows(projection.rows, projection.turns)).toMatchObject([{
      id: "activity-group:activity:command",
      kind: "activity_group",
      earlierRows: [],
      latest: { id: "activity:command", title: "pnpm test" },
      expanded: false,
    }]);
  });

  it("keeps a matching late assistant event visible after turn settlement", () => {
    const projection = projectCanonicalTimeline([
      stored(1, { type: "turn_started", payload: { providerTurnId: "provider-turn-1", state: "active", modes: { safetyMode: "ask_for_approval", interactionMode: "build" }, modelId: "gpt-5", modelOptions: [] } }),
      stored(2, { type: "turn_completed", payload: { state: "completed", stopReason: "end_turn", usage: null, changedFiles: [] } }, { createdAt: "2026-07-21T14:00:02.000Z" }),
      stored(3, { type: "content_delta", payload: { itemId: "late-answer", streamKind: "assistant_text", contentIndex: 0, delta: "Recovered final answer" } }, { createdAt: "2026-07-21T14:00:03.000Z" }),
    ]);

    expect(projection.rows).toMatchObject([
      { id: "message:late-answer", markdown: "Recovered final answer", state: "complete", metadata: { durationMs: 2_000 } },
    ]);
  });

  it("hydrates the same display model from validated paged SQLite projections", () => {
    const items: ChatTimelineItemRead[] = [
      {
        activityId: "provider-user-echo",
        turnId: "turn-1",
        sequenceAnchor: 1,
        kind: "activity",
        data: { schemaVersion: 1, value: { activityKind: "user_message", status: "completed", title: "User message", detail: null, providerItemId: "provider-user", metadata: {}, createdAt: start, updatedAt: start } },
      },
      {
        activityId: "message-1",
        turnId: "turn-1",
        sequenceAnchor: 2,
        kind: "message",
        data: { schemaVersion: 1, value: { role: "assistant", markdown: "Persisted", streamingState: "complete", providerItemId: "provider-item", metadata: {}, createdAt: start, updatedAt: start } },
      },
      {
        activityId: "message-1",
        turnId: "turn-1",
        sequenceAnchor: 3,
        kind: "activity",
        data: { schemaVersion: 1, value: { activityKind: "assistant_message", status: "completed", title: "Assistant response", detail: "Lifecycle copy", providerItemId: "provider-item", metadata: {}, createdAt: start, updatedAt: start } },
      },
    ];
    const turns: ChatTimelineTurnRead[] = [{
      turnId: "turn-1",
      state: "completed",
      startedAt: start,
      completedAt: "2026-07-21T14:00:02.000Z",
      stopReason: "end_turn",
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
        kind: "activity",
        data: { schemaVersion: 1, value: { activityKind: "assistant_message", status: "completed", title: "Assistant response", detail: "A normal **paragraph**.", providerItemId: "commentary", metadata: {}, createdAt: start, updatedAt: start } },
      },
      {
        activityId: "command",
        turnId: "turn-1",
        sequenceAnchor: 3,
        kind: "activity",
        data: { schemaVersion: 1, value: { activityKind: "command_execution", status: "completed", title: "Run tests", detail: "Passed", providerItemId: "command", metadata: {}, createdAt: start, updatedAt: start } },
      },
      {
        activityId: "answer",
        turnId: "turn-1",
        sequenceAnchor: 4,
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
    const projection = projectCanonicalTimeline([
      stored(1, { type: "turn_started", payload: { providerTurnId: "provider-turn-1", state: "active", modes: { safetyMode: "ask_for_approval", interactionMode: "build" }, modelId: "gpt-5", modelOptions: [] } }),
      stored(2, { type: "item_completed", payload: { itemId: "prompt", kind: "user_message", status: "completed", title: null, detail: "Question", safeMetadata: null } }),
      stored(3, { type: "item_completed", payload: { itemId: "command", kind: "command_execution", status: "completed", title: "Checked files", detail: null, safeMetadata: null } }),
      stored(4, { type: "item_completed", payload: { itemId: "answer", kind: "assistant_message", status: "completed", title: null, detail: "Answer", safeMetadata: null } }),
      stored(5, { type: "turn_completed", payload: { state: "completed", stopReason: "end_turn", usage: null, changedFiles: [] } }),
    ]);
    const displayRows = buildTimelineDisplayRows(projection.rows, projection.turns);

    expect([...timelineModelGroupStartIds(displayRows)]).toEqual(["turn-fold:turn-1"]);
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
