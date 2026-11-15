import { describe, expect, it } from "vitest";
import type {
  ChatAgentRunRead,
  ChatErrorCode,
  ChatFolderGrant,
  ChatMessageRead,
  ChatTimelineItemRead,
  ChatTimelineTurnRead,
  JsonValue,
  VersionedJson,
} from "$lib/chat/contracts";
import {
  agentRunIsActive,
  agentRunOutcome,
  agentRunRestart,
  agentRunShowedOutput,
  agentRunLocation,
  channelHasFolderGrants,
  exactExecutionReadKey,
  exactExecutionRuns,
  exactRunPresentationsReady,
  ExactExecutionReadCache,
  failureShownByReply,
  latestRenderableAgentRun,
  planExactExecutionReads,
  replyThreadRenderEntries,
  repliesWithoutRenderedRunProjection,
  retryableFailureEntryKey,
  shouldGroupReplyMessages,
} from "./reply-thread-model";

function richContent(value: VersionedJson["value"]): VersionedJson {
  return { schemaVersion: 1, value };
}

function agentRun(id: string, providerExecutionThreadId: string | null): ChatAgentRunRead {
  return {
    id,
    assignmentId: "assignment-1",
    projectId: "project-1",
    workingFolderId: "folder-1",
    executionEnvironmentId: "environment-1",
    scratchGenerationId: null,
    teammatePolicyRevisionId: "policy-1",
    effort: "medium",
    providerExecutionTurnId: `turn-${id}`,
    providerExecutionThreadId,
    state: "working",
    runOrdinal: 1,
    attemptReason: "fresh",
    handoffReason: null,
    createdAt: "2026-08-02T08:00:00Z",
    updatedAt: "2026-08-02T08:00:01Z",
  };
}

describe("reply thread provider projection", () => {
  it("selects the newest run that has renderable provider history", () => {
    expect(latestRenderableAgentRun([
      agentRun("run-1", "thread-1"),
      agentRun("run-2", null),
      agentRun("run-3", "thread-3"),
    ])?.id).toBe("run-3");
  });

  it("keeps projected agent content hidden until every exact run turn is loaded", () => {
    const first = agentRun("run-1", "thread-1");
    const second = agentRun("run-2", "thread-2");

    expect(exactRunPresentationsReady([first, second], new Set())).toBe(false);
    expect(exactRunPresentationsReady([first, second], new Set([first.id]))).toBe(false);
    expect(exactRunPresentationsReady([first, second], new Set([first.id, second.id]))).toBe(true);
    expect(exactRunPresentationsReady([], new Set())).toBe(true);
    expect(exactRunPresentationsReady([agentRun("pending", null)], new Set())).toBe(true);
  });

  it("hides only the materialized copy of the rendered run", () => {
    const replies = [
      { id: "human", richContent: richContent({ type: "doc" }) },
      { id: "older-run", richContent: richContent({ type: "agent_update", agentRunId: "run-1" }) },
      { id: "current-run", richContent: richContent({ type: "agent_update", agentRunId: "run-2" }) },
    ];

    expect(repliesWithoutRenderedRunProjection(replies, "run-2").map((reply) => reply.id))
      .toEqual(["human", "older-run"]);
    expect(repliesWithoutRenderedRunProjection(replies, null)).toEqual(replies);
  });
});

function message(
  itemId: string,
  authorId: string,
  createdAt: string,
  value: VersionedJson["value"] = { type: "doc" },
  position = 1,
): ChatMessageRead {
  return {
    itemId,
    conversationId: "conversation-1",
    replyThreadId: "reply-thread-1",
    revisionId: `revision-${itemId}`,
    revision: 1,
    author: {
      id: authorId,
      kind: "ai_teammate",
      displayName: "Atlas",
      avatar: { schemaVersion: 1, value: {} },
      revision: 1,
      archivedAt: null,
    },
    authorLabelSnapshot: "Atlas",
    normalizedMarkdown: itemId,
    richContent: richContent(value),
    attachmentIds: [],
    references: [],
    replyThread: null,
    orderKey: `c${String(position).padStart(3, "0")}`,
    awaitingDelivery: false,
    editedAt: null,
    createdAt,
  };
}

describe("reply thread execution placement", () => {
  it("replaces run projections with exact turns at their durable positions", () => {
    const replies = [
      message("approval", "teammate-1", "2026-08-02T08:00:00Z", {
        type: "agent_update",
        agentRunId: "run-1",
      }, 1),
      message("failure", "teammate-1", "2026-08-02T08:01:00Z", {
        type: "agent_update",
        agentRunId: "run-1",
      }, 2),
      message("retry", "local-user", "2026-08-02T09:00:00Z", { type: "doc" }, 3),
      message("result", "teammate-1", "2026-08-02T09:01:00Z", {
        type: "agent_update",
        agentRunId: "run-2",
      }, 4),
    ];
    const olderRun = agentRun("run-1", "thread-1");
    const currentRun = { ...agentRun("run-2", "thread-1"), createdAt: "2026-08-02T09:00:01Z" };

    expect(replyThreadRenderEntries(replies, [olderRun, currentRun], new Set([olderRun.id, currentRun.id]))
      .map((entry) => entry.key))
      .toEqual(["execution:run-1", "retry", "execution:run-2"]);
  });

  it("keeps host tool updates of the rendered run visible beside its exact turn", () => {
    const hostUpdate = (kind: string): JsonValue => ({
      type: "agent_update",
      updateKind: kind,
      assignmentId: "assignment-1",
      agentRunId: "run-1",
      payload: kind === "question"
        ? { question: "Which?", choices: ["A"], source: "host_tool" }
        : { kind, source: "host_tool" },
    });
    const replies = [
      message("plan", "teammate-1", "2026-08-02T08:00:00Z", hostUpdate("plan"), 1),
      message("projection", "teammate-1", "2026-08-02T08:01:00Z", { type: "agent_update", agentRunId: "run-1" }, 2),
      message("question", "teammate-1", "2026-08-02T08:02:00Z", hostUpdate("question"), 3),
    ];
    const run = agentRun("run-1", "thread-1");

    expect(replyThreadRenderEntries(replies, [run], new Set([run.id])).map((entry) => entry.key))
      .toEqual(["plan", "execution:run-1", "question"]);
    expect(repliesWithoutRenderedRunProjection(replies, "run-1").map((reply) => reply.itemId))
      .toEqual(["plan", "question"]);
    expect(shouldGroupReplyMessages(replies[0] ?? null, message("next", "teammate-1", "2026-08-02T08:00:30Z")))
      .toBe(false);
  });

  it("orders replies that share an order key by item id before placing runs", () => {
    const replies = [
      message("c-note", "local-user", "2026-08-02T08:02:00Z", { type: "doc" }, 3),
      message("b-retry", "local-user", "2026-08-02T08:00:00Z", { type: "doc" }, 2),
      message("a-result", "teammate-1", "2026-08-02T08:01:00Z", {
        type: "agent_update",
        agentRunId: "run-1",
      }, 2),
    ];
    const run = agentRun("run-1", "thread-1");

    expect(replyThreadRenderEntries(replies, [run], new Set([run.id])).map((entry) => entry.key))
      .toEqual(["execution:run-1", "b-retry", "c-note"]);
  });

  it("retains projections owned by a settled run whose exact turn is not loaded", () => {
    const projection = message("other-run", "teammate-1", "2026-08-02T08:00:00Z", {
      type: "agent_update",
      agentRunId: "run-1",
    });

    expect(replyThreadRenderEntries(
      [projection],
      [{ ...agentRun("run-1", "thread-1"), state: "completed" }],
      new Set(),
    ).map((entry) => entry.key)).toEqual(["other-run"]);
  });
});

describe("reply thread run status lines", () => {
  function settled(run: ChatAgentRunRead, state: ChatAgentRunRead["state"]): ChatAgentRunRead {
    return { ...run, state };
  }

  it("places a run that failed before it had a provider turn after the replies created before it", () => {
    const replies = [
      message("request", "local-user", "2026-08-02T07:59:00Z", { type: "doc" }, 1),
      message("later", "local-user", "2026-08-02T08:05:00Z", { type: "doc" }, 2),
    ];
    const failed = settled(agentRun("run-1", null), "failed");

    expect(replyThreadRenderEntries(replies, [failed], new Set()).map((entry) => (
      entry.kind === "execution" ? `${entry.key}:${entry.loaded}` : entry.key
    ))).toEqual(["request", "execution:run-1:false", "later"]);
  });

  it("keeps the projected replies of an unloaded active run and follows them with its status line", () => {
    const projection = message("progress", "teammate-1", "2026-08-02T08:00:30Z", {
      type: "agent_update",
      agentRunId: "run-1",
    }, 1);
    const note = message("note", "local-user", "2026-08-02T08:02:00Z", { type: "doc" }, 2);

    expect(replyThreadRenderEntries([projection, note], [agentRun("run-1", "thread-1")], new Set())
      .map((entry) => entry.key)).toEqual(["progress", "execution:run-1", "note"]);
  });

  it("leaves replies awaiting delivery to the queue, with or without runs in the thread", () => {
    const request = message("request", "local-user", "2026-08-02T07:59:00Z", { type: "doc" }, 1);
    const queued = {
      ...message("queued", "local-user", "2026-08-02T08:00:00Z", { type: "doc" }, 2),
      awaitingDelivery: true,
    };
    const note = message("note", "local-user", "2026-08-02T08:02:00Z", { type: "doc" }, 3);
    const run = agentRun("run-1", "thread-1");

    expect(replyThreadRenderEntries([request, queued, note], [], new Set()).map((entry) => entry.key))
      .toEqual(["request", "note"]);
    expect(replyThreadRenderEntries([request, queued, note], [run], new Set([run.id])).map((entry) => entry.key))
      .not.toContain("queued");
  });

  it("gives no line to settled runs that are not loaded or ended without trouble", () => {
    const replies = [message("request", "local-user", "2026-08-02T07:59:00Z")];
    const runs = [
      settled(agentRun("unloaded", "thread-1"), "failed"),
      settled(agentRun("completed", null), "completed"),
    ];

    expect(replyThreadRenderEntries(replies, runs, new Set()).map((entry) => entry.key)).toEqual(["request"]);
  });

  it("marks loaded runs so their exact turn renders", () => {
    const run = settled(agentRun("run-1", "thread-1"), "failed");
    const [entry] = replyThreadRenderEntries([], [run], new Set([run.id]));

    expect(entry).toEqual({ kind: "execution", key: "execution:run-1", run, loaded: true });
  });
});

describe("reply thread failures", () => {
  function failureReply(
    itemId: string,
    agentRunId: string | null,
    createdAt: string,
    position: number,
    assignmentId = "assignment-1",
  ): ChatMessageRead {
    return message(itemId, "teammate-1", createdAt, {
      type: "agent_update",
      updateKind: "failure",
      assignmentId,
      agentRunId,
      payload: agentRunId === null ? { launchFailure: true } : { code: "timeout", safeDetails: null },
    }, position);
  }

  function failedRun(id: string, providerExecutionThreadId: string | null = null): ChatAgentRunRead {
    return { ...agentRun(id, providerExecutionThreadId), state: "failed" };
  }

  const request = message("request", "local-user", "2026-08-02T07:59:00Z", { type: "doc" }, 1);

  it("leaves a failed run's status line to its own visible failure message", () => {
    const run = failedRun("run-1");
    const entries = replyThreadRenderEntries(
      [request, failureReply("failure", "run-1", "2026-08-02T08:01:00Z", 2)],
      [run],
      new Set(),
    );

    expect(failureShownByReply(entries, run)).toBe(true);
    expect(failureShownByReply(entries, failedRun("run-2"))).toBe(false);
  });

  it("covers a run with a launch failure of the same work posted after the run started", () => {
    const run = failedRun("run-1");
    const later = replyThreadRenderEntries(
      [request, failureReply("launch", null, "2026-08-02T08:01:00Z", 2)],
      [run],
      new Set(),
    );
    const earlier = replyThreadRenderEntries(
      [request, failureReply("launch", null, "2026-08-02T07:59:30Z", 2)],
      [run],
      new Set(),
    );
    const otherWork = replyThreadRenderEntries(
      [request, failureReply("launch", null, "2026-08-02T08:01:00Z", 2, "assignment-2")],
      [run],
      new Set(),
    );

    expect(failureShownByReply(later, run)).toBe(true);
    expect(failureShownByReply(earlier, run)).toBe(false);
    expect(failureShownByReply(otherWork, run)).toBe(false);
  });

  it("offers Retry only on the newest failure while the work is still failed", () => {
    const entries = replyThreadRenderEntries(
      [
        request,
        failureReply("first", "run-1", "2026-08-02T08:01:00Z", 2),
        failureReply("second", "run-2", "2026-08-02T08:03:00Z", 3),
      ],
      [],
      new Set(),
    );

    expect(retryableFailureEntryKey(entries, { id: "assignment-1", state: "failed" })).toBe("second");
    expect(retryableFailureEntryKey(entries, { id: "assignment-1", state: "working" })).toBeNull();
    expect(retryableFailureEntryKey(entries, { id: "assignment-2", state: "failed" })).toBeNull();
    expect(retryableFailureEntryKey(entries, null)).toBeNull();
  });

  it("offers Retry on a failed run's status line when no message explains the failure", () => {
    const run = failedRun("run-1", "thread-1");
    const entries = replyThreadRenderEntries([request], [run], new Set([run.id]));

    expect(retryableFailureEntryKey(entries, { id: "assignment-1", state: "failed" })).toBe("execution:run-1");
  });

  it("offers Retry on the failure message rather than the status line it covers", () => {
    const run = failedRun("run-1");
    const entries = replyThreadRenderEntries(
      [request, failureReply("failure", "run-1", "2026-08-02T08:01:00Z", 2)],
      [run],
      new Set(),
    );

    expect(entries.map((entry) => entry.key)).toEqual(["request", "failure", "execution:run-1"]);
    expect(retryableFailureEntryKey(entries, { id: "assignment-1", state: "failed" })).toBe("failure");
  });
});

describe("reply thread run outcomes", () => {
  const run = agentRun("run-1", "thread-1");

  function turn(errorCode: ChatErrorCode | null, stopReason: string | null): Pick<ChatTimelineTurnRead, "errorCode" | "stopReason"> {
    return { errorCode, stopReason };
  }

  function timelineItem(activityId: string, role: "user" | "assistant", turnId: string): ChatTimelineItemRead {
    return {
      activityId,
      turnId,
      sequenceAnchor: 1,
      changeSequence: 1,
      kind: "message",
      data: richContent({ role, markdown: activityId, streamingState: "complete", createdAt: "2026-08-02T08:00:00Z" }),
    };
  }

  it("counts only output beyond the hidden user message of the run's own turn", () => {
    const userOnly = { items: [timelineItem("prompt", "user", run.providerExecutionTurnId)], turns: [] };
    const otherTurn = { items: [timelineItem("other", "assistant", "turn-other")], turns: [] };
    const answered = { items: [...userOnly.items, timelineItem("answer", "assistant", run.providerExecutionTurnId)], turns: [] };

    expect(agentRunShowedOutput(run, null, [])).toBe(false);
    expect(agentRunShowedOutput(run, userOnly, [])).toBe(false);
    expect(agentRunShowedOutput(run, otherTurn, [])).toBe(false);
    expect(agentRunShowedOutput(run, answered, [])).toBe(true);
  });

  it("counts projected replies of the run as output but not host tool updates or other runs", () => {
    const projected = { richContent: richContent({ type: "agent_update", agentRunId: "run-1" }) };
    const otherRun = { richContent: richContent({ type: "agent_update", agentRunId: "run-2" }) };
    const hostUpdate = {
      richContent: richContent({
        type: "agent_update",
        updateKind: "plan",
        agentRunId: "run-1",
        payload: { kind: "plan", source: "host_tool" },
      }),
    };

    expect(agentRunShowedOutput(run, null, [otherRun, hostUpdate])).toBe(false);
    expect(agentRunShowedOutput(run, null, [projected])).toBe(true);
  });

  it("describes a failure with its stored code and keeps the diagnostic text as detail", () => {
    const failed = { state: "failed" } as const;
    const stored = turn("timeout", "The provider did not authenticate its internal host tools in time");

    expect(agentRunOutcome(failed, stored, false)).toEqual({
      kind: "failed",
      beforeOutput: true,
      errorCode: "timeout",
      detail: "The provider did not authenticate its internal host tools in time",
    });
    expect(agentRunOutcome(failed, stored, true)).toMatchObject({ kind: "failed", beforeOutput: false });
    expect(agentRunOutcome(failed, null, false, "provider_usage_limit")).toEqual({
      kind: "failed",
      beforeOutput: true,
      errorCode: null,
      detail: "provider_usage_limit",
    });
    expect(agentRunOutcome(failed, turn(null, null), false, "fallback")).toMatchObject({ errorCode: null, detail: "fallback" });
  });

  it("reports a stop only before output and nothing for completed or active runs", () => {
    expect(agentRunOutcome({ state: "cancelled" }, null, false)).toEqual({ kind: "stopped_before_output" });
    expect(agentRunOutcome({ state: "cancelled" }, null, true)).toBeNull();
    expect(agentRunOutcome({ state: "completed" }, turn(null, "end_turn"), false)).toBeNull();
    expect(agentRunOutcome({ state: "working" }, null, false)).toBeNull();
  });

  it("treats queued, starting, working, and waiting runs as stoppable", () => {
    const states: ChatAgentRunRead["state"][] = ["queued", "starting", "working", "waiting", "completed", "failed", "cancelled"];

    expect(states.filter((state) => agentRunIsActive({ state }))).toEqual(["queued", "starting", "working", "waiting"]);
  });
});

describe("reply thread message grouping", () => {
  it("groups nearby participant messages from the same author", () => {
    const previous = message("first", "teammate-1", "2026-08-02T08:00:00Z");
    const current = message("second", "teammate-1", "2026-08-02T08:04:00Z");

    expect(shouldGroupReplyMessages(previous, current)).toBe(true);
  });

  it("keeps projected approval and failure updates independently attributed", () => {
    const approval = message("approval", "teammate-1", "2026-08-02T08:00:00Z", {
      type: "agent_update",
      agentRunId: "run-1",
      updateKind: "approval",
    });
    const failure = message("failure", "teammate-1", "2026-08-02T08:01:00Z", {
      type: "agent_update",
      agentRunId: "run-1",
      updateKind: "failure",
    });

    expect(shouldGroupReplyMessages(approval, failure)).toBe(false);
  });

  it("does not group messages across authors or long pauses", () => {
    const previous = message("first", "teammate-1", "2026-08-02T08:00:00Z");

    expect(shouldGroupReplyMessages(
      previous,
      message("other-author", "teammate-2", "2026-08-02T08:01:00Z"),
    )).toBe(false);
    expect(shouldGroupReplyMessages(
      previous,
      message("later", "teammate-1", "2026-08-02T08:06:00Z"),
    )).toBe(false);
  });

  it("does not group messages that carry only an author label", () => {
    const unattributed = { ...message("first", "teammate-1", "2026-08-02T08:00:00Z"), author: null };
    const next = { ...message("second", "teammate-1", "2026-08-02T08:01:00Z"), author: null };

    expect(shouldGroupReplyMessages(unattributed, next)).toBe(false);
    expect(shouldGroupReplyMessages(
      unattributed,
      message("attributed", "teammate-1", "2026-08-02T08:01:00Z"),
    )).toBe(false);
  });
});

describe("reply thread run restarts", () => {
  it("labels only runs that handed off or retried in a new session", () => {
    const run = agentRun("run-1", "thread-1");
    expect(agentRunRestart({ ...run, attemptReason: "fresh" })).toBeNull();
    expect(agentRunRestart({ ...run, attemptReason: "continuation" })).toBeNull();
    expect(agentRunRestart({ ...run, attemptReason: "retry" })).toBe("retry");
    expect(agentRunRestart({
      ...run, attemptReason: "handoff", handoffReason: "native_session_lost",
    })).toBe("handoff");
  });
});

describe("reply thread exact execution reads", () => {
  function settledRun(id: string, updatedAt: string): ChatAgentRunRead {
    return { ...agentRun(id, "thread-1"), state: "completed", updatedAt };
  }

  it("reads only settled or scratch runs that have a provider thread", () => {
    const runs = [
      agentRun("working", "thread-1"),
      { ...agentRun("scratch", "thread-1"), scratchGenerationId: "scratch-1" },
      { ...agentRun("unstarted", null), state: "completed" as const },
      settledRun("done", "2026-08-02T08:00:05Z"),
      { ...agentRun("failed", "thread-1"), state: "failed" as const },
    ];

    expect(exactExecutionRuns(runs).map((run) => run.id)).toEqual(["scratch", "done", "failed"]);
  });

  it("rereads only runs that are new or whose revision changed", () => {
    const first = settledRun("first", "2026-08-02T08:00:05Z");
    const second = settledRun("second", "2026-08-02T08:00:06Z");
    const cache = new Map([
      ["first", { updatedAt: first.updatedAt, read: "first read" }],
      ["second", { updatedAt: "2026-08-02T08:00:02Z", read: "old second read" }],
      ["gone", { updatedAt: first.updatedAt, read: "gone read" }],
    ]);
    const third = settledRun("third", "2026-08-02T08:00:07Z");

    const plan = planExactExecutionReads([first, second, third], cache, new Set());

    expect(plan.toRead.map((run) => run.id)).toEqual(["second", "third"]);
    expect(plan.visible).toEqual({ first: "first read", second: "old second read" });
    expect([...plan.cache.keys()]).toEqual(["first", "second"]);
  });

  it("returns nothing to read when every run is cached at its revision", () => {
    const run = settledRun("first", "2026-08-02T08:00:05Z");
    const cache = new Map([["first", { updatedAt: run.updatedAt, read: 1 }]]);

    const plan = planExactExecutionReads([run], cache, new Set());

    expect(plan.toRead).toEqual([]);
    expect(plan.visible).toEqual({ first: 1 });
  });

  it("does not start a second read of a run revision already in flight", () => {
    const run = settledRun("first", "2026-08-02T08:00:05Z");

    const plan = planExactExecutionReads([run], new Map(), new Set([exactExecutionReadKey(run)]));

    expect(plan.toRead).toEqual([]);
    expect(plan.visible).toEqual({});
  });
});

describe("ExactExecutionReadCache", () => {
  it("returns a reply thread's reads again without sharing its stored map", () => {
    const cache = new ExactExecutionReadCache<string>(2);
    const reads = new Map([["run", { updatedAt: "2026-08-02T08:00:05Z", read: "turns" }]]);
    cache.set("thread", reads);
    reads.clear();

    const restored = cache.get("thread");
    restored.delete("run");

    expect([...cache.get("thread").entries()]).toEqual([["run", { updatedAt: "2026-08-02T08:00:05Z", read: "turns" }]]);
    expect(cache.get("unknown").size).toBe(0);
  });

  it("evicts the least recently shown reply thread past its bound", () => {
    const cache = new ExactExecutionReadCache<number>(2);
    cache.set("first", new Map([["run", { updatedAt: "a", read: 1 }]]));
    cache.set("second", new Map([["run", { updatedAt: "a", read: 2 }]]));
    cache.get("first");
    cache.set("third", new Map([["run", { updatedAt: "a", read: 3 }]]));

    expect(cache.get("second").size).toBe(0);
    expect(cache.get("first").get("run")?.read).toBe(1);
    expect(cache.get("third").get("run")?.read).toBe(3);
    cache.clear();
    expect(cache.get("first").size).toBe(0);
  });
});


describe("agentRunLocation", () => {
  const folders = [
    { workingFolder: { id: "folder-1", projectId: "project-1", displayName: "Website", archivedAt: null } },
    { workingFolder: { id: "folder-2", projectId: "project-1", displayName: "Mobile app", archivedAt: null } },
  ];

  it("names nothing when the folder is its project's only active one", () => {
    const solo = [
      { workingFolder: { id: "folder-1", projectId: "project-1", displayName: "Website", archivedAt: null } },
      { workingFolder: { id: "folder-2", projectId: "project-1", displayName: "Old", archivedAt: "2026-11-01T00:00:00Z" } },
      { workingFolder: { id: "folder-3", projectId: "project-2", displayName: "Other", archivedAt: null } },
    ];

    expect(agentRunLocation([agentRun("run-1", "thread-1")], solo)).toBeNull();
  });

  it("names the folder of the newest run", () => {
    const runs = [agentRun("run-1", "thread-1"), { ...agentRun("run-2", "thread-1"), workingFolderId: "folder-2" }];

    expect(agentRunLocation(runs, folders)).toEqual({ kind: "folder", name: "Mobile app" });
  });

  it("reads a newest run without folder or scratch as conversation only, even after an earlier folder run", () => {
    const conversation = { ...agentRun("run-2", "thread-1"), workingFolderId: null };

    expect(agentRunLocation([agentRun("run-1", "thread-1"), conversation], folders)).toEqual({ kind: "conversation" });
  });

  it("names nothing for private scratch, without runs, or when the folder is not known here", () => {
    const scratch = { ...agentRun("run-1", "thread-1"), workingFolderId: null, scratchGenerationId: "scratch-1" };

    expect(agentRunLocation([scratch], folders)).toBeNull();
    expect(agentRunLocation([], folders)).toBeNull();
    expect(agentRunLocation([{ ...agentRun("run-1", null), workingFolderId: "folder-9" }], folders)).toBeNull();
  });
});

describe("channelHasFolderGrants", () => {
  function grant(overrides: Partial<ChatFolderGrant> = {}): ChatFolderGrant {
    return {
      workingFolderId: "folder-1",
      displayName: "Website",
      capability: "execute",
      isDefault: false,
      runtimeApprovalOverride: null,
      revision: 1,
      revokedAt: null,
      ...overrides,
    };
  }

  function access(channelId: string, grants: ChatFolderGrant[], removedAt: string | null = null) {
    return { channels: [{ channelId, removedAt, folderGrants: grants }] };
  }

  it("finds an active grant in the channel", () => {
    expect(channelHasFolderGrants(access("channel-1", [grant()]), "channel-1")).toBe(true);
  });

  it("ignores revoked grants, grants without capability, other channels, and removed memberships", () => {
    expect(channelHasFolderGrants(access("channel-1", [grant({ revokedAt: "2026-11-13T00:00:00Z" })]), "channel-1")).toBe(false);
    expect(channelHasFolderGrants(access("channel-1", [grant({ capability: "none" })]), "channel-1")).toBe(false);
    expect(channelHasFolderGrants(access("channel-2", [grant()]), "channel-1")).toBe(false);
    expect(channelHasFolderGrants(access("channel-1", [grant()], "2026-11-13T00:00:00Z"), "channel-1")).toBe(false);
    expect(channelHasFolderGrants(null, "channel-1")).toBe(false);
  });
});
