import { describe, expect, it } from "vitest";
import type { ChatReviewQueueItemRead, ChatReviewQueueKind } from "$lib/chat/contracts";
import {
  batchAcceptCandidates,
  channelAttentionMarker,
  channelNeedsVisit,
  channelUnreadBadge,
  reviewQueueCount,
  reviewQueueSections,
  sidebarReviewQueueItems,
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
    expect(reviewQueueCount({ items: [item("a", "review")], total: 240, failureCount: 0, truncated: true })).toBe(240);
  });

  it("leaves failures out of the count", () => {
    const queue = { items: [item("a", "review"), item("b", "failure")], total: 2, failureCount: 1, truncated: false };

    expect(reviewQueueCount(queue)).toBe(1);
    expect(reviewQueueCount({ items: [item("b", "failure")], total: 1, failureCount: 1, truncated: false })).toBe(0);
  });

  it("reports zero before the first read", () => {
    expect(reviewQueueCount(null)).toBe(0);
  });
});

describe("sidebarReviewQueueItems", () => {
  it("keeps answers, approvals, and reviews in queue order and drops failures", () => {
    const items = sidebarReviewQueueItems([
      item("answer-1", "answer"),
      item("failure-1", "failure"),
      item("review-1", "review"),
      item("approval-1", "approval"),
    ]);

    expect(items.map((entry) => entry.assignmentId)).toEqual(["answer-1", "review-1", "approval-1"]);
  });
});

describe("reviewQueueSections", () => {
  it("orders kinds by priority, keeps item order within a kind, and leaves out failures", () => {
    const sections = reviewQueueSections([
      item("failure-1", "failure"),
      item("review-1", "review"),
      item("answer-1", "answer"),
      item("review-2", "review"),
      item("approval-1", "approval"),
    ]);

    expect(sections.map((section) => section.kind)).toEqual(["answer", "approval", "review"]);
    expect(sections[2].items.map((entry) => entry.assignmentId)).toEqual(["review-1", "review-2"]);
  });

  it("leaves out kinds with no items", () => {
    const sections = reviewQueueSections([item("review-1", "review"), item("failure-1", "failure")]);

    expect(sections.map((section) => section.kind)).toEqual(["review"]);
    expect(reviewQueueSections([item("failure-1", "failure")])).toEqual([]);
    expect(reviewQueueSections([])).toEqual([]);
  });
});

describe("channelUnreadBadge", () => {
  it("adds new replies to unread top-level messages", () => {
    expect(channelUnreadBadge({ unreadCount: 2, unreadReplyCount: 3, unseenFailure: false })).toEqual({
      count: 5,
      failure: false,
    });
  });

  it("marks an unseen failure even when nothing else is new", () => {
    expect(channelUnreadBadge({ unreadCount: 0, unreadReplyCount: 0, unseenFailure: true })).toEqual({
      count: 0,
      failure: true,
    });
  });
});

describe("channelNeedsVisit", () => {
  it("records a visit for unread messages, new replies, or an unseen failure", () => {
    expect(channelNeedsVisit({ unreadCount: 1, unreadReplyCount: 0, unseenFailure: false })).toBe(true);
    expect(channelNeedsVisit({ unreadCount: 0, unreadReplyCount: 1, unseenFailure: false })).toBe(true);
    expect(channelNeedsVisit({ unreadCount: 0, unreadReplyCount: 0, unseenFailure: true })).toBe(true);
  });

  it("leaves a quiet channel alone", () => {
    expect(channelNeedsVisit({ unreadCount: 0, unreadReplyCount: 0, unseenFailure: false })).toBe(false);
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
  it("marks the states that wait for the person, leaving failures to the unread badge", () => {
    expect(channelAttentionMarker("waiting_for_answer")).toBe("waiting_for_answer");
    expect(channelAttentionMarker("waiting_for_approval")).toBe("waiting_for_approval");
    expect(channelAttentionMarker("ready_for_review")).toBe("ready_for_review");
    expect(channelAttentionMarker("failed")).toBeNull();
  });

  it("keeps progress and settled work quiet", () => {
    expect(channelAttentionMarker("queued")).toBeNull();
    expect(channelAttentionMarker("working")).toBeNull();
    expect(channelAttentionMarker("completed")).toBeNull();
    expect(channelAttentionMarker("cancelled")).toBeNull();
    expect(channelAttentionMarker(null)).toBeNull();
  });
});
