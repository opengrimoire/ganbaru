import { describe, expect, it } from "vitest";
import type { ChatAssignmentInputRead } from "$lib/chat/contracts";
import { defaultFollowupRouting, pendingInputRows, pendingInputStatus } from "./pending-inputs";

function input(overrides: Partial<ChatAssignmentInputRead> = {}): ChatAssignmentInputRead {
  return {
    inputId: "input:1",
    messageItemId: "item:1",
    ordinal: 1,
    routingKind: "steer",
    deliveryState: "pending",
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

describe("defaultFollowupRouting", () => {
  it("steers a teammate in the middle of a turn and queues behind anything else", () => {
    expect(defaultFollowupRouting(null, "working")).toBe("steer");
    expect(defaultFollowupRouting(null, "waiting_for_approval")).toBe("queue");
    expect(defaultFollowupRouting(null, "queued")).toBe("queue");
  });

  it("keeps the choice the user made for the destination", () => {
    expect(defaultFollowupRouting("queue", "working")).toBe("queue");
    expect(defaultFollowupRouting("steer", "waiting_for_answer")).toBe("steer");
  });
});
