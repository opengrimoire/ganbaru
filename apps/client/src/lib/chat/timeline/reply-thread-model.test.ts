import { describe, expect, it } from "vitest";
import type { ChatAgentRunRead, ChatMessageRead, VersionedJson } from "$lib/chat/contracts";
import {
  exactRunPresentationsReady,
  latestRenderableAgentRun,
  replyThreadRenderEntries,
  repliesWithoutRenderedRunProjection,
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
  ordinal = 1,
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
    ordinal,
    editedAt: null,
    createdAt,
  };
}

describe("reply thread execution placement", () => {
  it("replaces run projections with exact turns at their durable ordinals", () => {
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

  it("retains projections owned by a run whose exact turn is not loaded", () => {
    const projection = message("other-run", "teammate-1", "2026-08-02T08:00:00Z", {
      type: "agent_update",
      agentRunId: "run-1",
    });

    expect(replyThreadRenderEntries(
      [projection],
      [agentRun("run-1", "thread-1")],
      new Set(),
    ).map((entry) => entry.key)).toEqual(["other-run"]);
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
});
