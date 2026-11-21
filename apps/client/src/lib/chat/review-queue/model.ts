import type {
  ChatChannelRead,
  ChatReviewQueueItemRead,
  ChatReviewQueueKind,
  ChatReviewQueueRead,
  ChatWorkAssignmentId,
  ChatWorkAssignmentState,
} from "$lib/chat/contracts";

/**
 * The kinds the sidebar lists, in priority order. Failures stay out: the channel row and the reply thread mark them
 * instead, while notifications still announce them from the full queue.
 */
export const SIDEBAR_REVIEW_QUEUE_KINDS = ["answer", "approval", "review"] as const;

/** A kind the sidebar lists. */
export type ChatSidebarReviewQueueKind = (typeof SIDEBAR_REVIEW_QUEUE_KINDS)[number];

/** A queue item the sidebar lists. */
export type ChatSidebarReviewQueueItem = ChatReviewQueueItemRead & { kind: ChatSidebarReviewQueueKind };

/** The sidebar queue items of one kind, in queue order. */
export interface ChatReviewQueueSection {
  kind: ChatSidebarReviewQueueKind;
  items: ChatSidebarReviewQueueItem[];
}

/** How one accept in a batch ended. */
export type ChatBatchAcceptOutcome = "accepted" | "stale" | "failed";

/** The outcome counts of a batch accept. */
export interface ChatBatchAcceptSummary {
  accepted: number;
  stale: number;
  failed: number;
}

/** What a channel row shows at its end: a count of new messages and a marker for an unseen failure. */
export interface ChatChannelUnreadBadge {
  /** Unread top-level messages plus new replies in threads the person took part in. */
  count: number;
  /** Whether a failed run in one of the channel's threads has not been seen yet. */
  failure: boolean;
}

function isSidebarKind(kind: ChatReviewQueueKind): kind is ChatSidebarReviewQueueKind {
  return (SIDEBAR_REVIEW_QUEUE_KINDS as readonly ChatReviewQueueKind[]).includes(kind);
}

function isSidebarItem(item: ChatReviewQueueItemRead): item is ChatSidebarReviewQueueItem {
  return isSidebarKind(item.kind);
}

/**
 * How many items wait in the sidebar queue, including those the cap leaves out of `items`. Failures are not counted.
 *
 * @param queue The last queue read, or `null` before the first read.
 */
export function reviewQueueCount(queue: ChatReviewQueueRead | null): number {
  if (!queue) return 0;
  return Math.max(0, queue.total - queue.failureCount);
}

/** The items of `items` the sidebar lists, in queue order, leaving failures out. */
export function sidebarReviewQueueItems(items: readonly ChatReviewQueueItemRead[]): ChatSidebarReviewQueueItem[] {
  return items.filter(isSidebarItem);
}

/**
 * Groups the sidebar items of `items` by kind in priority order (answer, approval, review), keeping the order of items
 * within each kind and leaving out failures and empty kinds.
 */
export function reviewQueueSections(items: readonly ChatReviewQueueItemRead[]): ChatReviewQueueSection[] {
  const listed = sidebarReviewQueueItems(items);
  return SIDEBAR_REVIEW_QUEUE_KINDS
    .map((kind) => ({ kind, items: listed.filter((item) => item.kind === kind) }))
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

/**
 * The count and failure marker a channel row shows.
 *
 * @param channel The channel as last read.
 */
export function channelUnreadBadge(
  channel: Pick<ChatChannelRead, "unreadCount" | "unreadReplyCount" | "unseenFailure">,
): ChatChannelUnreadBadge {
  return { count: channel.unreadCount + channel.unreadReplyCount, failure: channel.unseenFailure };
}

/**
 * Whether opening the channel should record a visit: it has unread messages, new replies, or an unseen failure.
 *
 * @param channel The channel as last read.
 */
export function channelNeedsVisit(
  channel: Pick<ChatChannelRead, "unreadCount" | "unreadReplyCount" | "unseenFailure">,
): boolean {
  const badge = channelUnreadBadge(channel);
  return badge.count > 0 || badge.failure;
}

/** The channel attention states that mark a channel row, because they wait for the person. */
export type ChatAttentionMarkerState = Extract<
  ChatWorkAssignmentState,
  "waiting_for_answer" | "waiting_for_approval" | "ready_for_review"
>;

/**
 * The marker a channel row shows for its attention state. Queued and working work stays quiet, so progress never asks
 * for attention, and failures show as the failure marker of the unread badge instead.
 */
export function channelAttentionMarker(state: ChatWorkAssignmentState | null): ChatAttentionMarkerState | null {
  switch (state) {
    case "waiting_for_answer":
    case "waiting_for_approval":
    case "ready_for_review":
      return state;
    default:
      return null;
  }
}
