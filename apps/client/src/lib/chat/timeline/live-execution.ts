import type {
  ChatAgentRunId,
  ChatLiveExecutionRead,
  ChatThreadId,
  ChatTimelineItemRead,
  ChatTimelinePageRead,
  ChatTimelineTurnRead,
  ChatTurnId,
  ModelId,
  ProviderFamilyId,
} from "$lib/chat/contracts";

/** The live execution a phone holds of work a linked desktop runs, built from the pages it read. */
export interface LiveExecutionState {
  /** The newest run of the work, once a run started. */
  runId: ChatAgentRunId | null;
  threadId: ChatThreadId | null;
  turnId: ChatTurnId | null;
  providerFamilyId: ProviderFamilyId | null;
  modelId: ModelId | null;
  /** Rows of the run's turn, in timeline order. */
  items: ChatTimelineItemRead[];
  turns: ChatTimelineTurnRead[];
  /** The desktop's change clock the rows were read at, sent back as the next read's cursor. */
  changeCursor: number;
  settled: boolean;
}

/**
 * What the reader does after a page: read the next page now, read again from the start, wait for the next change, or
 * stop because the run settled.
 */
export type LiveExecutionNext = "continue" | "restart" | "wait" | "stop";

/** The state after one page and what the reader does next. */
export interface LiveExecutionStep {
  state: LiveExecutionState | null;
  next: LiveExecutionNext;
}

/** The first delay before a read that failed is tried again. */
export const LIVE_EXECUTION_RETRY_BASE_MS = 2_000;
/** The longest delay between tries while the executing desktop stays unreachable. */
export const LIVE_EXECUTION_RETRY_MAX_MS = 30_000;
/**
 * The shortest time between the starts of two waiting reads, so a desktop that answers a wait at once cannot make the
 * phone read in a tight loop.
 */
export const LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS = 1_000;
/**
 * The first delay before asking again when no device serves the work yet, as while a linked desktop has not yet claimed
 * work posted here or its claim has not yet synced.
 */
export const LIVE_EXECUTION_NOT_SERVED_BASE_MS = 1_500;
/** Asks at the first delay before the delay grows, covering the time a linked desktop usually takes to claim work. */
export const LIVE_EXECUTION_NOT_SERVED_STEADY_ANSWERS = 12;
/** The longest delay between asks while no device serves the work. */
export const LIVE_EXECUTION_NOT_SERVED_MAX_MS = 10_000;
/** Reads of finished work tried at most before following stops, since finished work no longer changes. */
export const LIVE_EXECUTION_SETTLED_ATTEMPTS = 3;

/**
 * Applies one page read after `state` holds. A page that asks for a reset, or that names another run than the state
 * holds, starts over, since its cursor no longer describes the rows held; a page before any run started holds no rows,
 * so the run that starts next continues from its cursor. Otherwise removed and rewritten rows leave,
 * written rows join in timeline order, and the run's turn is replaced.
 */
export function applyLiveExecutionPage(
  state: LiveExecutionState | null,
  read: ChatLiveExecutionRead,
): LiveExecutionStep {
  if (read.resetRequired) return { state: null, next: "restart" };
  if (state?.threadId && (state.threadId !== read.threadId || state.turnId !== read.turnId)) {
    return { state: null, next: "restart" };
  }
  const leaving = new Set<string>(read.removedItemIds);
  for (const item of read.items) leaving.add(item.activityId);
  const kept = (state?.items ?? []).filter((item) => !leaving.has(item.activityId));
  const written = read.items.filter((item) => item.turnId === read.turnId);
  const next: LiveExecutionState = {
    runId: read.runId,
    threadId: read.threadId,
    turnId: read.turnId,
    providerFamilyId: read.providerFamilyId,
    modelId: read.modelId,
    items: [...kept, ...written].sort(compareLiveRows),
    turns: read.turns.filter((turn) => turn.turnId === read.turnId),
    changeCursor: read.changeCursor,
    settled: read.settled,
  };
  return { state: next, next: read.hasMore ? "continue" : read.settled ? "stop" : "wait" };
}

/**
 * The held execution as one timeline page the read-only timeline renders, or `null` while no run started or no row
 * arrived. The page has no paging cursors, since only the run's own turn is read.
 */
export function liveExecutionTimelinePage(state: LiveExecutionState | null): ChatTimelinePageRead | null {
  if (!state?.threadId || state.items.length === 0) return null;
  return {
    threadId: state.threadId,
    items: state.items,
    turns: state.turns,
    previousCursor: null,
    nextCursor: null,
    threadRevision: 0,
    changeCursor: state.changeCursor,
  };
}

/** How long to wait before trying again after `failures` reads in a row failed, doubling up to the bound. */
export function liveExecutionRetryDelay(failures: number): number {
  const exponent = Math.max(0, Math.min(failures - 1, 8));
  return Math.min(LIVE_EXECUTION_RETRY_MAX_MS, LIVE_EXECUTION_RETRY_BASE_MS * 2 ** exponent);
}

/**
 * How long to wait before asking again after `answers` reads in a row found no device serving the work: the first
 * delay while a claim is usually on its way, then doubling up to the bound.
 */
export function liveExecutionNotServedDelay(answers: number): number {
  const exponent = Math.max(0, Math.min(answers - LIVE_EXECUTION_NOT_SERVED_STEADY_ANSWERS, 8));
  return Math.min(LIVE_EXECUTION_NOT_SERVED_MAX_MS, LIVE_EXECUTION_NOT_SERVED_BASE_MS * 2 ** exponent);
}

/** How long to hold the next waiting read when the previous one started `elapsedMs` ago. */
export function liveExecutionWaitDelay(elapsedMs: number): number {
  return Math.max(0, LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS - Math.max(0, elapsedMs));
}

function compareLiveRows(left: ChatTimelineItemRead, right: ChatTimelineItemRead): number {
  if (left.sequenceAnchor !== right.sequenceAnchor) return left.sequenceAnchor - right.sequenceAnchor;
  if (left.activityId === right.activityId) return 0;
  return left.activityId < right.activityId ? -1 : 1;
}

/**
 * Whether the held execution shows every reply message whole, so it can stand for the replies projected from its run.
 * A message longer than a live read carries arrives as its tail only, and its projected reply keeps the whole text.
 */
export function liveExecutionShowsWholeMessages(state: LiveExecutionState): boolean {
  return state.items.every((item) => {
    if (item.kind !== "message") return true;
    const value = item.data.value;
    return typeof value !== "object" || value === null || Array.isArray(value) || value.markdownTruncated !== true;
  });
}
