import { describe, expect, it } from "vitest";
import type { ChatAssignmentBudgetRead, ChatMessageRead, UtcTimestamp } from "$lib/chat/contracts";
import {
  budgetAttention,
  budgetMeters,
  budgetStoppedAt,
  budgetThreshold,
  formatTokenCount,
  formatWorkingDuration,
  latestAssignmentLimitStop,
  parseTokenLimitInput,
  parseWorkingMinutesInput,
  providerLimitNotice,
  teammateUsageRange,
  warningPercent,
  workingMinutesFieldValue,
} from "./budget-model";

function budget(overrides: Partial<ChatAssignmentBudgetRead> = {}): ChatAssignmentBudgetRead {
  return {
    assignmentId: "assignment:a",
    tokenLimit: 20_000,
    workingSecondsLimit: null,
    setByParticipantId: "participant:victor",
    setAt: "2026-11-12T10:00:00.000Z",
    usedCountedTokens: 0,
    usedWorkingSeconds: 0,
    ...overrides,
  };
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
    awaitingDelivery: false,
    editedAt: null,
    createdAt: "2026-11-12T10:00:00.000Z",
  };
}

function failure(itemId: string, assignmentId: string, payload: unknown): ChatMessageRead {
  return reply(itemId, {
    type: "agent_update",
    updateKind: "failure",
    assignmentId,
    agentRunId: "run:1",
    payload,
  });
}

describe("budgetThreshold", () => {
  it("crosses each threshold exactly at its percent", () => {
    expect(budgetThreshold(749, 1_000)).toBe("none");
    expect(budgetThreshold(750, 1_000)).toBe("warning75");
    expect(budgetThreshold(899, 1_000)).toBe("warning75");
    expect(budgetThreshold(900, 1_000)).toBe("warning90");
    expect(budgetThreshold(999, 1_000)).toBe("warning90");
    expect(budgetThreshold(1_000, 1_000)).toBe("reached");
    expect(budgetThreshold(1_500, 1_000)).toBe("reached");
  });

  it("stays exact at the largest limit", () => {
    expect(budgetThreshold(749_999_999_999, 1_000_000_000_000)).toBe("none");
    expect(budgetThreshold(750_000_000_000, 1_000_000_000_000)).toBe("warning75");
  });

  it("treats a missing limit as nothing to warn about", () => {
    expect(budgetThreshold(10, 0)).toBe("none");
  });
});

describe("budgetMeters and budgetAttention", () => {
  it("returns only the limits the budget sets, tokens first", () => {
    const meters = budgetMeters(budget({
      workingSecondsLimit: 600,
      usedCountedTokens: 5_000,
      usedWorkingSeconds: 570,
    }));
    expect(meters.map((entry) => [entry.kind, entry.threshold, entry.fraction])).toEqual([
      ["tokens", "none", 0.25],
      ["workingTime", "warning90", 0.95],
    ]);
    expect(budgetAttention(meters)?.kind).toBe("workingTime");
  });

  it("clamps the bar past the limit and prefers tokens on a tie", () => {
    const meters = budgetMeters(budget({
      workingSecondsLimit: 600,
      usedCountedTokens: 30_000,
      usedWorkingSeconds: 900,
    }));
    expect(meters.map((entry) => entry.fraction)).toEqual([1, 1]);
    expect(budgetAttention(meters)?.kind).toBe("tokens");
  });

  it("returns no attention below the first warning", () => {
    expect(budgetAttention(budgetMeters(budget({ usedCountedTokens: 100 })))).toBeNull();
    expect(budgetMeters(budget({ tokenLimit: null }))).toEqual([]);
  });

  it("names the percent of warning thresholds only", () => {
    expect(warningPercent("warning75")).toBe(75);
    expect(warningPercent("warning90")).toBe(90);
    expect(warningPercent("reached")).toBeNull();
    expect(warningPercent("none")).toBeNull();
  });
});

describe("latestAssignmentLimitStop", () => {
  it("reads a budget stop with its limit and amounts", () => {
    const replies = [failure("1", "assignment:a", {
      code: "budget_limit_reached", limit: "working_time", used: 610, limitValue: 600,
    })];
    expect(latestAssignmentLimitStop(replies, "assignment:a")).toEqual({
      kind: "budget", limit: "workingTime", used: 610, limitValue: 600,
    });
  });

  it("reads a provider stop with and without a reset time", () => {
    expect(latestAssignmentLimitStop([
      failure("1", "assignment:a", { code: "provider_usage_limit", resetsAt: "2026-11-12T14:30:00.000Z" }),
    ], "assignment:a")).toEqual({ kind: "provider", resetsAt: "2026-11-12T14:30:00.000Z" });
    expect(latestAssignmentLimitStop([
      failure("1", "assignment:a", { code: "provider_usage_limit", resetsAt: null }),
    ], "assignment:a")).toEqual({ kind: "provider", resetsAt: null });
    expect(latestAssignmentLimitStop([
      failure("1", "assignment:a", { code: "provider_usage_limit", resetsAt: "soon" }),
    ], "assignment:a")).toEqual({ kind: "provider", resetsAt: null });
  });

  it("uses the newest failure of the assignment only", () => {
    const replies = [
      failure("1", "assignment:a", { code: "provider_usage_limit", resetsAt: null }),
      failure("2", "assignment:a", { launchFailure: true }),
      failure("3", "assignment:b", { code: "provider_usage_limit", resetsAt: null }),
      reply("4", { type: "agent_update", updateKind: "result", assignmentId: "assignment:a", payload: {} }),
    ];
    expect(latestAssignmentLimitStop(replies, "assignment:a")).toBeNull();
    expect(latestAssignmentLimitStop(replies, "assignment:b")?.kind).toBe("provider");
  });

  it("ignores malformed budget payloads", () => {
    expect(latestAssignmentLimitStop([
      failure("1", "assignment:a", { code: "budget_limit_reached", limit: "cost", used: 1, limitValue: 1 }),
    ], "assignment:a")).toBeNull();
    expect(latestAssignmentLimitStop([
      failure("1", "assignment:a", { code: "budget_limit_reached", limit: "tokens", used: -1, limitValue: 1 }),
    ], "assignment:a")).toBeNull();
  });
});

describe("formatting", () => {
  it("formats token counts compactly", () => {
    expect(formatTokenCount("en", 950)).toBe("950");
    expect(formatTokenCount("en", 20_000)).toBe("20K");
    expect(formatTokenCount("en", 1_250_000)).toBe("1.3M");
  });

  it("formats working time in hours and minutes", () => {
    expect(formatWorkingDuration("en", 45)).toBe("45 sec");
    expect(formatWorkingDuration("en", 60)).toBe("1 min");
    expect(formatWorkingDuration("en", 3_600)).toBe("1 hr");
    expect(formatWorkingDuration("en", 5_430)).toBe("1 hr 30 min");
  });
});

describe("limit inputs", () => {
  it("parses token limits within the backend bounds", () => {
    expect(parseTokenLimitInput("")).toEqual({ kind: "empty" });
    expect(parseTokenLimitInput(" 20,000 ")).toEqual({ kind: "valid", value: 20_000 });
    expect(parseTokenLimitInput("20.000")).toEqual({ kind: "valid", value: 20_000 });
    expect(parseTokenLimitInput("999")).toEqual({ kind: "invalid" });
    expect(parseTokenLimitInput("1000000000001")).toEqual({ kind: "invalid" });
    expect(parseTokenLimitInput("12k")).toEqual({ kind: "invalid" });
    expect(parseTokenLimitInput("-5000")).toEqual({ kind: "invalid" });
  });

  it("parses working minutes into seconds within the backend bounds", () => {
    expect(parseWorkingMinutesInput("30")).toEqual({ kind: "valid", value: 1_800 });
    expect(parseWorkingMinutesInput("0")).toEqual({ kind: "invalid" });
    expect(parseWorkingMinutesInput("144000")).toEqual({ kind: "valid", value: 8_640_000 });
    expect(parseWorkingMinutesInput("144001")).toEqual({ kind: "invalid" });
  });

  it("shows a seconds limit as whole minutes", () => {
    expect(workingMinutesFieldValue(null)).toBe("");
    expect(workingMinutesFieldValue(1_800)).toBe("30");
    expect(workingMinutesFieldValue(90)).toBe("2");
  });
});

describe("teammateUsageRange", () => {
  it("covers seven days ending today", () => {
    expect(teammateUsageRange(Temporal.PlainDate.from("2026-03-02"))).toEqual({
      fromDate: "2026-02-24",
      toDate: "2026-03-02",
    });
  });

  it("covers at least today", () => {
    expect(teammateUsageRange(Temporal.PlainDate.from("2026-11-12"), 0)).toEqual({
      fromDate: "2026-11-12",
      toDate: "2026-11-12",
    });
  });
});

describe("budgetStoppedAt", () => {
  const stopped = {
    id: "assignment:a",
    state: "failed",
    stateReason: "budget_limit_reached",
    budget: budget({ workingSecondsLimit: 600, usedCountedTokens: 1_000, usedWorkingSeconds: 600 }),
  };

  it("names the limit from the failure update", () => {
    expect(budgetStoppedAt(stopped, [failure("1", "assignment:a", {
      code: "budget_limit_reached", limit: "tokens", used: 20_100, limitValue: 20_000,
    })])).toBe("tokens");
  });

  it("falls back to the reached meter before the failure update arrives", () => {
    expect(budgetStoppedAt(stopped, [])).toBe("workingTime");
  });

  it("returns nothing for work that did not stop at its budget", () => {
    expect(budgetStoppedAt({ ...stopped, stateReason: "provider_usage_limit" }, [])).toBeNull();
    expect(budgetStoppedAt({ ...stopped, state: "completed" }, [])).toBeNull();
    expect(budgetStoppedAt({ ...stopped, budget: null }, [])).toBeNull();
  });
});

describe("providerLimitNotice", () => {
  const resetsAt = "2026-11-12T14:30:00.000Z" as UtcTimestamp;
  const failed = { id: "assignment:a", state: "failed", stateReason: "provider_usage_limit", budget: null };

  it("shows the reset time from the failure every device reads", () => {
    expect(providerLimitNotice(failed, [
      failure("1", "assignment:a", { code: "provider_usage_limit", resetsAt }),
    ], null)).toEqual({ resetsAt });
  });

  it("shows the notice without a time when the provider gave none", () => {
    expect(providerLimitNotice(failed, [
      failure("1", "assignment:a", { code: "provider_usage_limit", resetsAt: null }),
    ], null)).toEqual({ resetsAt: null });
    expect(providerLimitNotice(failed, [], null)).toEqual({ resetsAt: null });
  });

  it("reads the live report on the running device", () => {
    const working = { ...failed, state: "working", stateReason: null };
    expect(providerLimitNotice(working, [], { limited: true, resetsAt })).toEqual({ resetsAt });
    expect(providerLimitNotice(null, [], { limited: true, resetsAt: "later" as UtcTimestamp })).toEqual({ resetsAt: null });
    expect(providerLimitNotice(working, [], { limited: false, resetsAt })).toBeNull();
    expect(providerLimitNotice(null, [], null)).toBeNull();
  });
});
