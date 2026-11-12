import type {
  ChatAgentRunRead,
  ChatMessageRead,
  VersionedJson,
} from "$lib/chat/contracts";
import { compareOrderKeyThenId } from "$lib/utils/order-key";
import { isHostToolUpdate } from "./agent-update";

interface RichReply {
  richContent: VersionedJson;
}

export type ReplyThreadRenderEntry =
  | { kind: "message"; key: string; message: ChatMessageRead }
  | { kind: "execution"; key: string; run: ChatAgentRunRead };

/** Returns the newest agent run whose provider execution can be rendered. */
export function latestRenderableAgentRun(
  agentRuns: readonly ChatAgentRunRead[],
): ChatAgentRunRead | null {
  return agentRuns.findLast((agentRun) => agentRun.providerExecutionThreadId !== null) ?? null;
}

/** Returns whether every persisted provider run has its exact turn ready to render. */
export function exactRunPresentationsReady(
  runs: readonly ChatAgentRunRead[],
  loadedRunIds: ReadonlySet<string>,
): boolean {
  return runs.every((run) => (
    run.providerExecutionThreadId === null || loadedRunIds.has(run.id)
  ));
}

/** Run states after which a provider run's exact turn no longer changes. */
const SETTLED_RUN_STATES: ReadonlySet<ChatAgentRunRead["state"]> = new Set(["completed", "failed", "cancelled"]);

/** Returns the runs whose exact provider turn the reply thread reads directly: settled runs and scratch runs. */
export function exactExecutionRuns(runs: readonly ChatAgentRunRead[]): ChatAgentRunRead[] {
  return runs.filter((run) => (
    run.providerExecutionThreadId !== null
    && (run.scratchGenerationId !== null || SETTLED_RUN_STATES.has(run.state))
  ));
}

/** One exact execution read and the run revision it was read at. */
export interface CachedExactExecution<Read> {
  updatedAt: string;
  read: Read;
}

/** What to show and what to read for the exact runs of a reply thread. */
export interface ExactExecutionReadPlan<Read> {
  /** Cache pruned to the current runs. */
  cache: Map<string, CachedExactExecution<Read>>;
  /** Reads to show by run ID, including a previous read while its run is read again. */
  visible: Record<string, Read>;
  /** Runs that are new or whose `updatedAt` changed since their cached read, minus reads already in flight. */
  toRead: ChatAgentRunRead[];
}

/** Returns the key that identifies one read of a run at one revision. */
export function exactExecutionReadKey(run: Pick<ChatAgentRunRead, "id" | "updatedAt">): string {
  return `${run.id}:${run.updatedAt}`;
}

/**
 * Plans exact execution reads so a refresh rereads only runs that changed. A cached read whose run changed stays
 * visible until its new read lands.
 *
 * @param runs Current exact runs of the reply thread.
 * @param cache Reads kept from earlier refreshes, keyed by run ID.
 * @param inFlight Read keys (see {@link exactExecutionReadKey}) that are already being read.
 */
export function planExactExecutionReads<Read>(
  runs: readonly ChatAgentRunRead[],
  cache: ReadonlyMap<string, CachedExactExecution<Read>>,
  inFlight: ReadonlySet<string>,
): ExactExecutionReadPlan<Read> {
  const kept = new Map<string, CachedExactExecution<Read>>();
  const visible: Record<string, Read> = {};
  const toRead: ChatAgentRunRead[] = [];
  for (const run of runs) {
    const cached = cache.get(run.id);
    if (cached) {
      kept.set(run.id, cached);
      visible[run.id] = cached.read;
    }
    if (cached?.updatedAt !== run.updatedAt && !inFlight.has(exactExecutionReadKey(run))) toRead.push(run);
  }
  return { cache: kept, visible, toRead };
}

/** Removes only the projected copies owned by the provider run rendered in the thread. */
export function repliesWithoutRenderedRunProjection<T extends RichReply>(
  replies: readonly T[],
  renderedRunId: string | null,
): T[] {
  if (renderedRunId === null) return [...replies];
  return replies.filter((reply) => projectedAgentRunId(reply) !== renderedRunId);
}

/**
 * Returns the provider run a reply is a projected copy of, or `null` when the reply stands on its own.
 * Updates a host tool call posted carry their run but are not in the provider transcript, so they stay
 * visible next to the run's exact turn.
 */
export function projectedAgentRunId(message: RichReply): string | null {
  const value = message.richContent.value;
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  if (value.type !== "agent_update" || typeof value.agentRunId !== "string") return null;
  return isHostToolUpdate(message.richContent) ? null : value.agentRunId;
}

function isAgentUpdate(message: RichReply): boolean {
  const value = message.richContent.value;
  return typeof value === "object" && value !== null && !Array.isArray(value) && value.type === "agent_update";
}

/** Groups same-author replies within five minutes, keeping agent updates separately attributable. */
export function shouldGroupReplyMessages(
  previous: ChatMessageRead | null,
  current: ChatMessageRead,
): boolean {
  if (!previous?.author || previous.author.id !== current.author?.id) return false;
  if (isAgentUpdate(previous) || isAgentUpdate(current)) return false;
  const elapsedMs = Date.parse(current.createdAt) - Date.parse(previous.createdAt);
  return elapsedMs >= 0 && elapsedMs <= 5 * 60 * 1_000;
}

/** Replaces projected run copies with exact execution turns at the same thread position. */
export function replyThreadRenderEntries(
  replies: readonly ChatMessageRead[],
  agentRuns: readonly ChatAgentRunRead[],
  loadedRunIds: ReadonlySet<string>,
): ReplyThreadRenderEntry[] {
  const renderedRuns = agentRuns.filter((run) => (
    run.providerExecutionThreadId !== null && loadedRunIds.has(run.id)
  ));
  if (renderedRuns.length === 0) {
    return replies.map((message) => ({ kind: "message", key: message.itemId, message }));
  }
  const renderedRunIds = new Set(renderedRuns.map((run) => run.id));
  // Positions are indexes in thread order, `(orderKey, itemId)`.
  const ordered = [...replies].sort((left, right) => (
    compareOrderKeyThenId(left.orderKey, left.itemId, right.orderKey, right.itemId)
  ));
  const positioned: Array<{ position: number; entry: ReplyThreadRenderEntry }> = [];
  ordered.forEach((message, position) => {
    const runId = projectedAgentRunId(message);
    if (runId !== null && renderedRunIds.has(runId)) return;
    positioned.push({ position, entry: { kind: "message", key: message.itemId, message } });
  });
  for (const run of renderedRuns) {
    const projectedPositions = ordered.flatMap((reply, position) => (
      projectedAgentRunId(reply) === run.id ? [position] : []
    ));
    positioned.push({
      position: projectedPositions.length > 0
        ? Math.max(...projectedPositions)
        : fallbackRunPosition(ordered, run.createdAt),
      entry: { kind: "execution", key: `execution:${run.id}`, run },
    });
  }
  return positioned
    .sort((left, right) => left.position - right.position || left.entry.key.localeCompare(right.entry.key))
    .map(({ entry }) => entry);
}

/** Places a run without projected replies after the last reply created before it. */
function fallbackRunPosition(ordered: readonly ChatMessageRead[], createdAt: string): number {
  let preceding: number | null = null;
  let following: number | null = null;
  for (const [position, reply] of ordered.entries()) {
    if (reply.createdAt <= createdAt) {
      preceding = Math.max(preceding ?? position, position);
    } else {
      following = Math.min(following ?? position, position);
    }
  }
  if (preceding !== null) return preceding + 0.5;
  if (following !== null) return following - 0.5;
  return -0.5;
}

/** Returns how a run restarted its provider session, or `null` when it opened or continued one as planned. */
export function agentRunRestart(run: ChatAgentRunRead): "handoff" | "retry" | null {
  return run.attemptReason === "handoff" || run.attemptReason === "retry" ? run.attemptReason : null;
}
