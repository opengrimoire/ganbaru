import { describe, expect, it } from "vitest";
import { parseChatReplyThreadPage, parseChatSecretRequests, parseChatWorkAssignment } from "./organizational";

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
    delegation: null,
    latestRunProviderThreadId: null,
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

const delegationWire = {
  parentAssignmentId: "assignment:parent",
  parentReplyThreadId: "reply-thread:parent",
  depth: 1,
  objective: "Check the build",
  expectedResult: "A short report",
};

describe("delegated work reads", () => {
  it("reads where delegated work came from, with or without the parent thread", () => {
    expect(parseChatWorkAssignment(assignmentWire({ delegation: delegationWire })).delegation).toEqual(delegationWire);
    expect(parseChatWorkAssignment(assignmentWire({
      delegation: { ...delegationWire, parentReplyThreadId: null, depth: 2 },
    })).delegation?.parentReplyThreadId).toBeNull();
  });

  it("refuses a delegation outside the depth limit or without its expected result", () => {
    for (const depth of [0, 3]) {
      expect(() => parseChatWorkAssignment(assignmentWire({ delegation: { ...delegationWire, depth } })))
        .toThrow("Chat work assignment.delegation.depth must be between 1 and 2");
    }
    expect(() => parseChatWorkAssignment(assignmentWire({ delegation: { ...delegationWire, expectedResult: null } })))
      .toThrow("Chat work assignment.delegation.expectedResult");
  });

  it("reads the delegated children of a reply thread page and refuses unknown states", () => {
    const participant = assignmentWire({}).teammate;
    const child = {
      assignmentId: "assignment:child",
      parentAssignmentId: "assignment:review",
      replyThreadId: "reply-thread:child",
      teammate: participant,
      objective: "Check the build",
      state: "waiting_for_answer",
      stateReason: null,
      createdAt: TIMESTAMP,
    };
    const message = {
      itemId: "item:root",
      conversationId: "conversation:general",
      replyThreadId: null,
      revisionId: "revision:root",
      revision: 1,
      author: participant,
      authorLabelSnapshot: "Atlas",
      normalizedMarkdown: "Root",
      richContent: { schemaVersion: 1, value: {} },
      attachmentIds: [],
      references: [],
      replyThread: null,
      orderKey: "c001",
      editedAt: null,
      createdAt: TIMESTAMP,
    };
    const pageWire = (delegatedChildren: unknown[]) => ({
      thread: {
        id: "reply-thread:review",
        replyCount: 0,
        lastActivityAt: TIMESTAMP,
        participants: [participant],
        unread: false,
        workState: "working",
      },
      rootMessage: message,
      replies: [],
      assignment: null,
      agentRuns: [],
      delegatedChildren,
      previousCursor: null,
      revision: 1,
    });

    expect(parseChatReplyThreadPage(pageWire([child])).delegatedChildren).toEqual([child]);
    expect(() => parseChatReplyThreadPage(pageWire([{ ...child, state: "paused" }])))
      .toThrow("Chat reply thread page.delegatedChildren[0].state");
    expect(() => parseChatReplyThreadPage({ ...pageWire([]), delegatedChildren: undefined }))
      .toThrow("Chat reply thread page.delegatedChildren");
  });
});

describe("parseChatSecretRequests", () => {
  const request = {
    id: "0a1b2c",
    assignmentId: "assignment:review",
    label: "API token",
    purpose: "Deploy the preview",
    createdAt: TIMESTAMP,
    expiresAt: REVIEW_TIME,
  };

  it("reads open secret requests", () => {
    expect(parseChatSecretRequests([request])).toEqual([request]);
  });

  it("refuses a request that expires before it was made or misses its label", () => {
    expect(() => parseChatSecretRequests([{ ...request, expiresAt: TIMESTAMP }]))
      .toThrow("Chat secret requests[0].expiresAt must be after createdAt");
    expect(() => parseChatSecretRequests([{ ...request, label: 7 }])).toThrow("Chat secret requests[0].label");
  });
});
