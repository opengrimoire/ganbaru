import { describe, expect, it } from "vitest";
import { parseChatWorkAssignment } from "./organizational";

const TIMESTAMP = "2026-08-05T16:00:00.000Z";
const REVIEW_TIME = "2026-08-05T17:30:00.000Z";

function assignmentWire(overrides: Record<string, unknown>): Record<string, unknown> {
  return {
    id: "assignment:review",
    replyThreadId: "reply-thread:review",
    teammate: {
      id: "participant:atlas",
      kind: "ai_teammate",
      displayName: "Atlas",
      avatar: { schemaVersion: 1, value: {} },
      revision: 1,
      archivedAt: null,
    },
    triggeringMessageItemId: "item:root",
    previousAssignmentId: null,
    state: "completed",
    stateReason: null,
    revision: 3,
    settledAt: REVIEW_TIME,
    createdAt: TIMESTAMP,
    updatedAt: REVIEW_TIME,
    review: null,
    localAcceptance: null,
    ...overrides,
  };
}

const acceptedReview = {
  outcome: "accepted",
  reviewedAt: REVIEW_TIME,
  reviewedByParticipantId: "participant:local",
};

describe("parseChatWorkAssignment", () => {
  it("reads the review recorded on completed work", () => {
    for (const outcome of ["accepted", "changes_requested"]) {
      const parsed = parseChatWorkAssignment(assignmentWire({ review: { ...acceptedReview, outcome } }));

      expect(parsed.review).toEqual({ ...acceptedReview, outcome });
    }
  });

  it("reads work without a review", () => {
    expect(parseChatWorkAssignment(assignmentWire({ state: "ready_for_review", settledAt: null })).review).toBeNull();
  });

  it("refuses a review on work that is not completed", () => {
    for (const state of ["ready_for_review", "working", "failed", "cancelled"]) {
      expect(() => parseChatWorkAssignment(assignmentWire({ state, review: acceptedReview })))
        .toThrow("Chat work assignment.review is only valid on completed work");
    }
  });

  it("refuses an unknown outcome or a partial review", () => {
    expect(() => parseChatWorkAssignment(assignmentWire({ review: { ...acceptedReview, outcome: "ignored" } })))
      .toThrow("Chat work assignment.review.outcome");
    expect(() => parseChatWorkAssignment(assignmentWire({ review: { ...acceptedReview, reviewedAt: null } })))
      .toThrow("Chat work assignment.review.reviewedAt");
    expect(() => parseChatWorkAssignment(assignmentWire({ review: { outcome: "accepted" } })))
      .toThrow("Chat work assignment.review.reviewedAt");
  });

  it("refuses work that omits the review field", () => {
    const wire = assignmentWire({});
    delete wire.review;

    expect(() => parseChatWorkAssignment(wire)).toThrow("Chat work assignment.review");
  });
});
