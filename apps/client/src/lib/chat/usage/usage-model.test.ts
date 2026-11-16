import { describe, expect, it } from "vitest";
import type { ChatMessageRead, UtcTimestamp } from "$lib/chat/contracts";
import {
  formatTokenCount,
  formatWorkingDuration,
  latestProviderLimitStop,
  providerLimitNotice,
  teammateUsageRange,
} from "./usage-model";

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

describe("latestProviderLimitStop", () => {
  it("reads a provider stop with and without a reset time", () => {
    expect(latestProviderLimitStop([
      failure("1", "assignment:a", { code: "provider_usage_limit", resetsAt: "2026-11-12T14:30:00.000Z" }),
    ], "assignment:a")).toEqual({ resetsAt: "2026-11-12T14:30:00.000Z" });
    expect(latestProviderLimitStop([
      failure("1", "assignment:a", { code: "provider_usage_limit", resetsAt: null }),
    ], "assignment:a")).toEqual({ resetsAt: null });
    expect(latestProviderLimitStop([
      failure("1", "assignment:a", { code: "provider_usage_limit", resetsAt: "soon" }),
    ], "assignment:a")).toEqual({ resetsAt: null });
  });

  it("uses the newest failure of the assignment only", () => {
    const replies = [
      failure("1", "assignment:a", { code: "provider_usage_limit", resetsAt: null }),
      failure("2", "assignment:a", { launchFailure: true }),
      failure("3", "assignment:b", { code: "provider_usage_limit", resetsAt: null }),
      reply("4", { type: "agent_update", updateKind: "result", assignmentId: "assignment:a", payload: {} }),
    ];
    expect(latestProviderLimitStop(replies, "assignment:a")).toBeNull();
    expect(latestProviderLimitStop(replies, "assignment:b")).toEqual({ resetsAt: null });
  });

  it("ignores failures for other reasons", () => {
    expect(latestProviderLimitStop([
      failure("1", "assignment:a", { code: "timeout" }),
    ], "assignment:a")).toBeNull();
    expect(latestProviderLimitStop([failure("1", "assignment:a", "text")], "assignment:a")).toBeNull();
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

describe("providerLimitNotice", () => {
  const resetsAt = "2026-11-12T14:30:00.000Z" as UtcTimestamp;
  const failed = { id: "assignment:a", state: "failed", stateReason: "provider_usage_limit" };

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
