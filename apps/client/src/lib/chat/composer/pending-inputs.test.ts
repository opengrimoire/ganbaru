import { describe, expect, it } from "vitest";
import type { ChatAssignmentInputRead } from "$lib/chat/contracts";
import {
  pendingInputCanSteerNow,
  pendingInputReorderable,
  pendingInputRows,
  pendingInputStartsWork,
  pendingInputStatus,
} from "./pending-inputs";

function input(overrides: Partial<ChatAssignmentInputRead> = {}): ChatAssignmentInputRead {
  return {
    inputId: "input:1",
    messageItemId: "item:1",
    ordinal: 1,
    routingKind: "steer",
    deliveryState: "pending",
    held: false,
    deliveredAt: null,
    deliveredRevisionId: null,
    currentRevisionId: "revision:1",
    preview: "Use the staging bucket",
    previewTruncated: false,
    authorParticipantId: null,
    ...overrides,
  };
}

describe("pendingInputStatus", () => {
  it("shows waiting and undelivered replies", () => {
    expect(pendingInputStatus(input())).toBe("pending");
    expect(pendingInputStatus(input({ deliveryState: "failed" }))).toBe("failed");
  });

  it("hides delivered replies the run received as they read now", () => {
    expect(pendingInputStatus(input({
      deliveryState: "delivered",
      deliveredRevisionId: "revision:1",
      currentRevisionId: "revision:1",
    }))).toBeNull();
  });

  it("flags a delivered reply edited after the run received it", () => {
    expect(pendingInputStatus(input({
      deliveryState: "delivered",
      deliveredRevisionId: "revision:1",
      currentRevisionId: "revision:2",
    }))).toBe("editedAfterDelivery");
  });

  it("hides ignored replies and delivered replies without a recorded revision", () => {
    expect(pendingInputStatus(input({ deliveryState: "ignored" }))).toBeNull();
    expect(pendingInputStatus(input({ deliveryState: "delivered", deliveredRevisionId: null }))).toBeNull();
  });
});

describe("pendingInputRows", () => {
  it("lists the replies that need attention in delivery order", () => {
    const rows = pendingInputRows([
      input({ inputId: "input:3", ordinal: 3, deliveryState: "failed" }),
      input({ inputId: "input:2", ordinal: 2, deliveryState: "delivered", deliveredRevisionId: "revision:1" }),
      input({ inputId: "input:1", ordinal: 1 }),
    ]);

    expect(rows.map((row) => [row.input.inputId, row.status])).toEqual([
      ["input:1", "pending"],
      ["input:3", "failed"],
    ]);
  });
});

describe("pendingInputStartsWork", () => {
  it("marks only the reply that started its own work", () => {
    expect(pendingInputStartsWork(input({ routingKind: "follow_up" }))).toBe(true);
    expect(pendingInputStartsWork(input({ routingKind: "steer" }))).toBe(false);
    expect(pendingInputStartsWork(input({ routingKind: "queued_continuation" }))).toBe(false);
  });
});

describe("pendingInputReorderable", () => {
  it("lets only waiting replies that did not start work move in the queue", () => {
    expect(pendingInputReorderable({ input: input({ routingKind: "queued_continuation" }), status: "pending" })).toBe(true);
    expect(pendingInputReorderable({ input: input({ held: true }), status: "pending" })).toBe(true);
    expect(pendingInputReorderable({ input: input({ routingKind: "follow_up" }), status: "pending" })).toBe(false);
    expect(pendingInputReorderable({ input: input({ deliveryState: "failed" }), status: "failed" })).toBe(false);
    expect(pendingInputReorderable({ input: input({ deliveryState: "delivered" }), status: "editedAfterDelivery" })).toBe(false);
  });
});

describe("pendingInputCanSteerNow", () => {
  it("offers steering for queued and held replies only", () => {
    expect(pendingInputCanSteerNow({ input: input({ routingKind: "queued_continuation" }), status: "pending" })).toBe(true);
    expect(pendingInputCanSteerNow({ input: input({ routingKind: "steer", held: true }), status: "pending" })).toBe(true);
    expect(pendingInputCanSteerNow({ input: input({ routingKind: "steer" }), status: "pending" })).toBe(false);
    expect(pendingInputCanSteerNow({ input: input({ routingKind: "follow_up" }), status: "pending" })).toBe(false);
  });
});
