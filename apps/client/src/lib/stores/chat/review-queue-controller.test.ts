import { beforeEach, describe, expect, it, vi } from "vitest";
import { en } from "$lib/i18n/messages/en";
import type {
  ChatAssignmentReviewRequest,
  ChatReviewQueueItemRead,
  ChatReviewQueueKind,
  ChatReviewQueueRead,
  ChatWorkAssignmentRead,
} from "$lib/chat/contracts";
import { ChatReviewQueueController } from "./review-queue-controller.svelte";

const api = vi.hoisted(() => ({
  listReviewQueue: vi.fn<(projectId: string | null) => Promise<ChatReviewQueueRead>>(),
  reviewAssignment: vi.fn<
    (assignmentId: string, expectedRevision: number, review: ChatAssignmentReviewRequest) => Promise<ChatWorkAssignmentRead>
  >(),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  listChatReviewQueue: api.listReviewQueue,
  reviewChatAssignment: api.reviewAssignment,
}));

function item(id: string, kind: ChatReviewQueueKind, channelId = "channel:one"): ChatReviewQueueItemRead {
  return {
    assignmentId: id,
    revision: 4,
    kind,
    projectId: "project:one",
    projectName: "One",
    channelId,
    channelName: "general",
    replyThreadId: `reply-thread:${id}`,
    teammateParticipantId: "participant:teammate",
    teammateDisplayName: "Builder",
    stateReason: null,
    changedFileCount: 1,
    executedHere: true,
    updatedAt: "2026-11-12T10:00:00.000Z",
  };
}

function queueOf(items: ChatReviewQueueItemRead[]): ChatReviewQueueRead {
  return { items, total: items.length, truncated: false };
}

/** A promise with its resolver exposed, so a test controls when a read finishes. */
function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

describe("ChatReviewQueueController", () => {
  beforeEach(() => {
    api.listReviewQueue.mockReset();
    api.reviewAssignment.mockReset();
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  it("accepts a batch sequentially and reports accepted, stale, and failed outcomes", async () => {
    const onReviewed = vi.fn();
    const controller = new ChatReviewQueueController({ onReviewed });
    const items = [
      item("assignment:accepted", "review", "channel:one"),
      item("assignment:stale", "review", "channel:two"),
      item("assignment:failed", "review", "channel:three"),
      item("assignment:answer", "answer"),
    ];
    api.listReviewQueue.mockResolvedValueOnce(queueOf(items));
    await controller.refresh();
    const order: string[] = [];
    api.reviewAssignment.mockImplementation(async (assignmentId) => {
      order.push(assignmentId);
      if (assignmentId === "assignment:stale") throw { code: "stale_revision", message: "Changed elsewhere" };
      if (assignmentId === "assignment:failed") throw new Error("Database is busy");
      return {} as ChatWorkAssignmentRead;
    });
    api.listReviewQueue.mockResolvedValueOnce(queueOf([items[1], items[2]]));

    const summary = await controller.acceptReviewItems(items.map((entry) => entry.assignmentId));

    expect(summary).toEqual({ accepted: 1, stale: 1, failed: 1 });
    expect(order).toEqual(["assignment:accepted", "assignment:stale", "assignment:failed"]);
    expect(api.reviewAssignment).toHaveBeenCalledWith("assignment:accepted", 4, { kind: "accept" });
    expect(onReviewed).toHaveBeenCalledExactlyOnceWith(["channel:one"]);
    expect(api.listReviewQueue).toHaveBeenCalledTimes(2);
    expect(controller.queue?.items.map((entry) => entry.assignmentId)).toEqual(["assignment:stale", "assignment:failed"]);
  });

  it("coalesces a burst of refreshes into one running read and one queued read", async () => {
    const controller = new ChatReviewQueueController({ onReviewed: vi.fn() });
    const first = deferred<ChatReviewQueueRead>();
    const second = deferred<ChatReviewQueueRead>();
    api.listReviewQueue.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    const running = controller.refresh();
    const queuedA = controller.refresh();
    const queuedB = controller.refresh();
    expect(queuedB).toBe(queuedA);
    expect(api.listReviewQueue).toHaveBeenCalledTimes(1);

    first.resolve(queueOf([item("assignment:first", "review")]));
    await running;
    second.resolve(queueOf([item("assignment:second", "failure")]));
    await queuedA;

    expect(api.listReviewQueue).toHaveBeenCalledTimes(2);
    expect(api.listReviewQueue).toHaveBeenCalledWith(null);
    expect(controller.queue?.items.map((entry) => entry.assignmentId)).toEqual(["assignment:second"]);
  });

  it("keeps the previous queue and reports an error when a refresh fails", async () => {
    const controller = new ChatReviewQueueController({ onReviewed: vi.fn() });
    api.listReviewQueue.mockResolvedValueOnce(queueOf([item("assignment:kept", "review")]));
    await controller.refresh();
    api.listReviewQueue.mockRejectedValueOnce(new Error("Database is locked"));

    await controller.refresh();

    expect(controller.queue?.items.map((entry) => entry.assignmentId)).toEqual(["assignment:kept"]);
    expect(controller.error).toBe(en.chatErrors.actions.reviewQueueAction);

    api.listReviewQueue.mockResolvedValueOnce(queueOf([]));
    await controller.refresh();
    expect(controller.error).toBeNull();
    expect(controller.queue?.total).toBe(0);
  });

  it("ignores a read that finishes after a vault reset", async () => {
    const controller = new ChatReviewQueueController({ onReviewed: vi.fn() });
    const stale = deferred<ChatReviewQueueRead>();
    api.listReviewQueue.mockReturnValueOnce(stale.promise);

    const pending = controller.refresh();
    controller.reset();
    stale.resolve(queueOf([item("assignment:old-vault", "review")]));
    await pending;

    expect(controller.queue).toBeNull();
  });
});
