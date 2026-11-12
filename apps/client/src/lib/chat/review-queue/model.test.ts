import { describe, expect, it } from "vitest";
import type { ChatReviewQueueItemRead, ChatReviewQueueKind } from "$lib/chat/contracts";
import {
  batchAcceptCandidates,
  channelAttentionMarker,
  reviewQueueCount,
  reviewQueueSections,
  summarizeBatchAccept,
} from "./model";

function item(id: string, kind: ChatReviewQueueKind): ChatReviewQueueItemRead {
  return {
    assignmentId: id,
    revision: 3,
    kind,
    projectId: "project:one",
    projectName: "One",
    channelId: "channel:one",
    channelName: "general",
    replyThreadId: `reply-thread:${id}`,
    teammateParticipantId: "participant:teammate",
    teammateDisplayName: "Builder",
    stateReason: null,
    changedFileCount: kind === "review" ? 2 : 0,
    executedHere: true,
    updatedAt: "2026-11-12T10:00:00.000Z",
  };
}

describe("reviewQueueCount", () => {
  it("reports the total, including items the cap leaves out", () => {
    expect(reviewQueueCount({ items: [item("a", "review")], total: 240, truncated: true })).toBe(240);
  });

  it("reports zero before the first read", () => {
    expect(reviewQueueCount(null)).toBe(0);
  });
});

describe("reviewQueueSections", () => {
  it("orders kinds by priority and keeps item order within a kind", () => {
    const sections = reviewQueueSections([
      item("failure-1", "failure"),
      item("review-1", "review"),
      item("answer-1", "answer"),
      item("review-2", "review"),
      item("approval-1", "approval"),
    ]);

    expect(sections.map((section) => section.kind)).toEqual(["answer", "approval", "review", "failure"]);
    expect(sections[2].items.map((entry) => entry.assignmentId)).toEqual(["review-1", "review-2"]);
  });

  it("leaves out kinds with no items", () => {
    const sections = reviewQueueSections([item("review-1", "review"), item("failure-1", "failure")]);

    expect(sections.map((section) => section.kind)).toEqual(["review", "failure"]);
    expect(reviewQueueSections([])).toEqual([]);
  });
});

describe("batchAcceptCandidates", () => {
  it("keeps selected reviews and ignores selected items of other kinds", () => {
    const items = [
      item("answer-1", "answer"),
      item("review-1", "review"),
      item("review-2", "review"),
      item("failure-1", "failure"),
    ];

    const candidates = batchAcceptCandidates(items, new Set(["answer-1", "review-2", "failure-1"]));

    expect(candidates.map((entry) => entry.assignmentId)).toEqual(["review-2"]);
  });

  it("ignores selected ids that are no longer in the queue", () => {
    expect(batchAcceptCandidates([item("review-1", "review")], new Set(["review-gone"]))).toEqual([]);
  });
});

describe("summarizeBatchAccept", () => {
  it("counts each outcome of a mixed batch", () => {
    expect(summarizeBatchAccept(["accepted", "stale", "accepted", "failed", "accepted"])).toEqual({
      accepted: 3,
      stale: 1,
      failed: 1,
    });
  });

  it("reports zeros for an empty batch", () => {
    expect(summarizeBatchAccept([])).toEqual({ accepted: 0, stale: 0, failed: 0 });
  });
});

describe("channelAttentionMarker", () => {
  it("marks the states that wait for the person", () => {
    expect(channelAttentionMarker("waiting_for_answer")).toBe("waiting_for_answer");
    expect(channelAttentionMarker("waiting_for_approval")).toBe("waiting_for_approval");
    expect(channelAttentionMarker("ready_for_review")).toBe("ready_for_review");
    expect(channelAttentionMarker("failed")).toBe("failed");
  });

  it("keeps progress and settled work quiet", () => {
    expect(channelAttentionMarker("queued")).toBeNull();
    expect(channelAttentionMarker("working")).toBeNull();
    expect(channelAttentionMarker("completed")).toBeNull();
    expect(channelAttentionMarker("cancelled")).toBeNull();
    expect(channelAttentionMarker(null)).toBeNull();
  });
});
