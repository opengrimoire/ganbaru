import {
  CHAT_REVIEW_QUEUE_KINDS,
  type ChatReviewQueueItemRead,
  type ChatReviewQueueKind,
  type ChatReviewQueueRead,
  type ChatWorkAssignmentId,
  type ChatWorkAssignmentState,
} from "$lib/chat/contracts";

/** The queue items of one kind, in queue order. */
export interface ChatReviewQueueSection {
  kind: ChatReviewQueueKind;
  items: ChatReviewQueueItemRead[];
}

/** How one accept in a batch ended. */
export type ChatBatchAcceptOutcome = "accepted" | "stale" | "failed";

/** The outcome counts of a batch accept. */
export interface ChatBatchAcceptSummary {
  accepted: number;
  stale: number;
  failed: number;
}

/**
 * How many items wait in the queue, including those the cap leaves out of `items`.
 *
 * @param queue The last queue read, or `null` before the first read.
 */
export function reviewQueueCount(queue: ChatReviewQueueRead | null): number {
  return queue?.total ?? 0;
}

/**
 * Groups queue items by kind in priority order (answer, approval, review, failure), keeping the
 * order of items within each kind and leaving out empty kinds.
 */
export function reviewQueueSections(items: readonly ChatReviewQueueItemRead[]): ChatReviewQueueSection[] {
  return CHAT_REVIEW_QUEUE_KINDS
    .map((kind) => ({ kind, items: items.filter((item) => item.kind === kind) }))
    .filter((section) => section.items.length > 0);
}

/**
 * The selected items a batch accept may record: only reviews can be accepted, so answers,
 * approvals, and failures are skipped even when selected.
 */
export function batchAcceptCandidates(
  items: readonly ChatReviewQueueItemRead[],
  selectedIds: ReadonlySet<ChatWorkAssignmentId>,
): ChatReviewQueueItemRead[] {
  return items.filter((item) => item.kind === "review" && selectedIds.has(item.assignmentId));
}

/** Counts how each accept of a batch ended. */
export function summarizeBatchAccept(results: readonly ChatBatchAcceptOutcome[]): ChatBatchAcceptSummary {
  const summary: ChatBatchAcceptSummary = { accepted: 0, stale: 0, failed: 0 };
  for (const result of results) summary[result] += 1;
  return summary;
}

/** The channel attention states that mark a channel row, because they wait for the person. */
export type ChatAttentionMarkerState = Extract<
  ChatWorkAssignmentState,
  "waiting_for_answer" | "waiting_for_approval" | "ready_for_review" | "failed"
>;

/**
 * The marker a channel row shows for its attention state. Queued and working work stays quiet, so
 * progress never asks for attention.
 */
export function channelAttentionMarker(state: ChatWorkAssignmentState | null): ChatAttentionMarkerState | null {
  switch (state) {
    case "waiting_for_answer":
    case "waiting_for_approval":
    case "ready_for_review":
    case "failed":
      return state;
    default:
      return null;
  }
}
