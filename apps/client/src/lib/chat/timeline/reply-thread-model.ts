import type {
  ChatAgentRunRead,
  ChatMessageRead,
  VersionedJson,
} from "$lib/chat/contracts";
import { compareOrderKeyThenId } from "$lib/utils/order-key";

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

/** Removes only the projected copies owned by the provider run rendered in the thread. */
export function repliesWithoutRenderedRunProjection<T extends RichReply>(
  replies: readonly T[],
  renderedRunId: string | null,
): T[] {
  if (renderedRunId === null) return [...replies];
  return replies.filter((reply) => projectedAgentRunId(reply) !== renderedRunId);
}

export function projectedAgentRunId(message: RichReply): string | null {
  const value = message.richContent.value;
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  return value.type === "agent_update" && typeof value.agentRunId === "string"
    ? value.agentRunId
    : null;
}

/** Groups same-author replies within five minutes, keeping agent updates separately attributable. */
export function shouldGroupReplyMessages(
  previous: ChatMessageRead | null,
  current: ChatMessageRead,
): boolean {
  if (!previous?.author || previous.author.id !== current.author?.id) return false;
  if (projectedAgentRunId(previous) !== null || projectedAgentRunId(current) !== null) return false;
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
