import { describe, expect, it } from "vitest";
import type { ActivityStatus, ChatMessageRead, PlanStep } from "$lib/chat/contracts";
import { latestStructuredPlanSteps, summarizePlanProgress } from "./plan-progress";

function steps(...statuses: ActivityStatus[]): PlanStep[] {
  return statuses.map((status, index) => ({ id: `step-${index + 1}`, text: `Step ${index + 1}`, status }));
}

function reply(itemId: string, value: unknown): ChatMessageRead {
  return {
    itemId,
    conversationId: "conversation:test",
    replyThreadId: "reply-thread:test",
    revisionId: `revision:${itemId}`,
    revision: 1,
    author: null,
    authorLabelSnapshot: "Atlas",
    normalizedMarkdown: "",
    richContent: { schemaVersion: 1, value: value as ChatMessageRead["richContent"]["value"] },
    attachmentIds: [],
    references: [],
    replyThread: null,
    orderKey: itemId,
    editedAt: null,
    createdAt: "2026-11-12T10:00:00.000Z",
  };
}

function planUpdate(itemId: string, assignmentId: string, planSteps: unknown): ChatMessageRead {
  return reply(itemId, {
    type: "agent_update",
    updateKind: "plan",
    assignmentId,
    agentRunId: "run:1",
    payload: { steps: planSteps },
  });
}

describe("summarizePlanProgress", () => {
  it("returns null for a plan without steps", () => {
    expect(summarizePlanProgress([])).toBeNull();
  });

  it("counts every step as completed when the plan is done and names no active step", () => {
    expect(summarizePlanProgress(steps("completed", "completed"))).toEqual({
      total: 2,
      completed: 2,
      failed: 0,
      activeStep: null,
      segments: ["completed", "completed"],
    });
  });

  it("counts failed and interrupted steps together and leaves them out of completed", () => {
    const summary = summarizePlanProgress(steps("completed", "failed", "interrupted", "active"));
    expect(summary?.completed).toBe(1);
    expect(summary?.failed).toBe(2);
    expect(summary?.activeStep?.id).toBe("step-4");
    expect(summary?.segments).toEqual(["completed", "failed", "interrupted", "active"]);
  });

  it("prefers the step in progress over an earlier pending step", () => {
    expect(summarizePlanProgress(steps("pending", "active"))?.activeStep?.id).toBe("step-2");
  });

  it("falls back to the first pending step when no step is in progress", () => {
    expect(summarizePlanProgress(steps("completed", "waiting", "pending", "pending"))?.activeStep?.id).toBe("step-3");
  });
});

describe("latestStructuredPlanSteps", () => {
  it("reads the latest plan update of the assignment and skips other assignments and proposed plans", () => {
    const replies = [
      planUpdate("c1", "assignment:a", steps("active")),
      planUpdate("c2", "assignment:a", steps("completed", "active")),
      planUpdate("c3", "assignment:b", steps("pending")),
      reply("c4", { type: "agent_update", updateKind: "plan", assignmentId: "assignment:a", payload: { planId: "plan:1" } }),
      reply("c5", { lines: [] }),
    ];
    expect(latestStructuredPlanSteps(replies, "assignment:a")?.map((step) => step.status)).toEqual(["completed", "active"]);
  });

  it("drops malformed steps and treats a plan with none left as absent", () => {
    const replies = [
      planUpdate("c1", "assignment:a", steps("pending")),
      planUpdate("c2", "assignment:a", [{ id: "", text: "Empty id", status: "active" }, { id: "x", text: "Odd", status: "in_progress" }]),
    ];
    expect(latestStructuredPlanSteps(replies, "assignment:a")?.map((step) => step.id)).toEqual(["step-1"]);
    expect(latestStructuredPlanSteps([], "assignment:a")).toBeNull();
  });
});
