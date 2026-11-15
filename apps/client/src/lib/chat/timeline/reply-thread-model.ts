import type {
  ChatAgentRunRead,
  ChatErrorCode,
  ChatMessageRead,
  ChatTeammateChannelAccess,
  ChatTimelinePageRead,
  ChatTimelineTurnRead,
  ChatWorkAssignmentRead,
  ProjectWorkingFolder,
  VersionedJson,
} from "$lib/chat/contracts";
import { compareOrderKeyThenId } from "$lib/utils/order-key";
import { chatAgentUpdatePresentation, isHostToolUpdate } from "./agent-update";
import { projectTimelineReadModel } from "./projection";

interface RichReply {
  richContent: VersionedJson;
}

export type ReplyThreadRenderEntry =
  | { kind: "message"; key: string; message: ChatMessageRead }
  | {
    kind: "execution";
    key: string;
    run: ChatAgentRunRead;
    /** Whether the run's exact provider turn is loaded; otherwise only the run's status line renders. */
    loaded: boolean;
  };

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

/** Run states in which a run still works or waits to work. */
const ACTIVE_RUN_STATES: ReadonlySet<ChatAgentRunRead["state"]> = new Set(["queued", "starting", "working", "waiting"]);

/** Returns whether a run is still working or waiting to work, so it can be stopped. */
export function agentRunIsActive(run: Pick<ChatAgentRunRead, "state">): boolean {
  return ACTIVE_RUN_STATES.has(run.state);
}

/**
 * Returns whether a run needs its own place in the thread although its exact turn is not loaded: a run that failed or
 * was stopped before it had a provider turn, or a run that still works and can be stopped.
 */
function runShowsWithoutExecution(run: ChatAgentRunRead, loadedRunIds: ReadonlySet<string>): boolean {
  if (run.providerExecutionThreadId !== null && loadedRunIds.has(run.id)) return false;
  if (agentRunIsActive(run)) return true;
  return run.providerExecutionThreadId === null && (run.state === "failed" || run.state === "cancelled");
}

/**
 * Replaces projected run copies with exact execution turns at the same thread position. Runs that need a status line
 * without a loaded turn get an entry with `loaded: false` and keep their projected replies. Replies awaiting delivery
 * stay out: the queue above the composer lists them until the work receives them and they take their place.
 */
export function replyThreadRenderEntries(
  threadReplies: readonly ChatMessageRead[],
  agentRuns: readonly ChatAgentRunRead[],
  loadedRunIds: ReadonlySet<string>,
): ReplyThreadRenderEntry[] {
  const replies = threadReplies.filter((reply) => !reply.awaitingDelivery);
  const renderedRuns = agentRuns.filter((run) => (
    run.providerExecutionThreadId !== null && loadedRunIds.has(run.id)
  ));
  const statusOnlyRuns = agentRuns.filter((run) => runShowsWithoutExecution(run, loadedRunIds));
  if (renderedRuns.length === 0 && statusOnlyRuns.length === 0) {
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
  for (const run of [...renderedRuns, ...statusOnlyRuns]) {
    const projectedPositions = ordered.flatMap((reply, position) => (
      projectedAgentRunId(reply) === run.id ? [position] : []
    ));
    const loaded = renderedRunIds.has(run.id);
    // A status line follows the projected replies it leaves visible.
    positioned.push({
      position: projectedPositions.length > 0
        ? Math.max(...projectedPositions) + (loaded ? 0 : 0.25)
        : fallbackRunPosition(ordered, run.createdAt),
      entry: { kind: "execution", key: `execution:${run.id}`, run, loaded },
    });
  }
  return positioned
    .sort((left, right) => left.position - right.position || left.entry.key.localeCompare(right.entry.key))
    .map(({ entry }) => entry);
}

/**
 * Returns whether a visible failure reply already explains how `run` failed: its own failure message, or a launch
 * failure of the same work posted after the run started. The run's status line then leaves the failure to that reply.
 *
 * @param entries Render entries of the thread, see {@link replyThreadRenderEntries}.
 * @param run The run whose status line would show the failure.
 */
export function failureShownByReply(
  entries: readonly ReplyThreadRenderEntry[],
  run: Pick<ChatAgentRunRead, "id" | "assignmentId" | "createdAt">,
): boolean {
  return entries.some((entry) => {
    if (entry.kind !== "message") return false;
    const presentation = chatAgentUpdatePresentation(entry.message.richContent);
    if (presentation?.kind !== "failure") return false;
    if (presentation.agentRunId !== null) return presentation.agentRunId === run.id;
    return presentation.assignmentId === run.assignmentId && entry.message.createdAt >= run.createdAt;
  });
}

/**
 * Returns the key of the render entry that offers Retry: the last entry that shows a failure of the thread's work
 * while that work is still failed. Earlier failures and failures of earlier work show no Retry, so the thread offers
 * one at most.
 *
 * @param entries Render entries of the thread, see {@link replyThreadRenderEntries}.
 * @param assignment The thread's current work, or `null` without work.
 */
export function retryableFailureEntryKey(
  entries: readonly ReplyThreadRenderEntry[],
  assignment: Pick<ChatWorkAssignmentRead, "id" | "state"> | null,
): string | null {
  if (assignment?.state !== "failed") return null;
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const entry = entries[index];
    if (!entry) continue;
    if (entry.kind === "message") {
      const presentation = chatAgentUpdatePresentation(entry.message.richContent);
      if (presentation?.kind === "failure" && presentation.assignmentId === assignment.id) return entry.key;
    } else if (
      entry.run.assignmentId === assignment.id
      && entry.run.state === "failed"
      && !failureShownByReply(entries, entry.run)
    ) {
      return entry.key;
    }
  }
  return null;
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

/** How a settled run ended when its end needs its own line in the reply thread. */
export type AgentRunOutcome =
  | {
    kind: "failed";
    /** Whether the run failed before it showed any output. */
    beforeOutput: boolean;
    /** Code of the stored error, which chooses the localized explanation. */
    errorCode: ChatErrorCode | null;
    /** Diagnostic text for the technical detail slot only. */
    detail: string | null;
  }
  | { kind: "stopped_before_output" };

/**
 * Returns whether a run showed anything in the thread: a projected reply or a row of its exact turn other than the
 * hidden user message.
 */
export function agentRunShowedOutput(
  run: Pick<ChatAgentRunRead, "id" | "providerExecutionTurnId">,
  timelinePage: Pick<ChatTimelinePageRead, "items" | "turns"> | null,
  replies: readonly RichReply[],
): boolean {
  if (replies.some((reply) => projectedAgentRunId(reply) === run.id)) return true;
  if (!timelinePage) return false;
  return projectTimelineReadModel(timelinePage.items, timelinePage.turns).rows.some((row) => (
    row.turnId === run.providerExecutionTurnId && !(row.kind === "message" && row.role === "user")
  ));
}

/**
 * Returns how a settled run ended when the thread would otherwise not show it: every failure, and a stop that came
 * before any output. A completed run, a run still working, and a stop after output return `null`.
 *
 * @param run Run to describe.
 * @param turn The run's exact provider turn, when it had one and it is loaded.
 * @param showedOutput Whether the run showed output, see {@link agentRunShowedOutput}.
 * @param fallbackDetail Diagnostic text to use when the turn has none, such as the assignment's state reason.
 */
export function agentRunOutcome(
  run: Pick<ChatAgentRunRead, "state">,
  turn: Pick<ChatTimelineTurnRead, "stopReason" | "errorCode"> | null,
  showedOutput: boolean,
  fallbackDetail: string | null = null,
): AgentRunOutcome | null {
  if (run.state === "failed") {
    return {
      kind: "failed",
      beforeOutput: !showedOutput,
      errorCode: turn?.errorCode ?? null,
      detail: turn?.stopReason ?? fallbackDetail,
    };
  }
  if (run.state === "cancelled" && !showedOutput) return { kind: "stopped_before_output" };
  return null;
}

/** Where the newest run of a reply thread works. */
export type AgentRunLocation =
  | { kind: "folder"; name: string }
  | { kind: "conversation" };

/**
 * Returns where the thread's newest run works: the named working folder, or conversation only when it has neither a
 * folder nor a private scratch. A folder is named only when its project has other active folders, since the only
 * folder of a project is where every run there works and naming it adds nothing. Returns `null` without runs, for
 * private scratch, for a project's only folder, or when the folder is not known here.
 */
export function agentRunLocation(
  runs: readonly ChatAgentRunRead[],
  folders: readonly { workingFolder: Pick<ProjectWorkingFolder, "id" | "projectId" | "displayName" | "archivedAt"> }[],
): AgentRunLocation | null {
  const run = runs.at(-1);
  if (!run) return null;
  if (run.workingFolderId === null) {
    return run.scratchGenerationId === null ? { kind: "conversation" } : null;
  }
  const folder = folders.find((entry) => entry.workingFolder.id === run.workingFolderId)?.workingFolder;
  if (folder === undefined) return null;
  const projectFolderCount = folders.filter((entry) => (
    entry.workingFolder.projectId === folder.projectId && entry.workingFolder.archivedAt === null
  )).length;
  return projectFolderCount > 1 ? { kind: "folder", name: folder.displayName } : null;
}

/**
 * Returns whether the teammate holds a folder grant in the channel, so conversation-only work there means no grant
 * was implied rather than that the teammate has no folder access at all.
 */
export function channelHasFolderGrants(
  access: { channels: readonly Pick<ChatTeammateChannelAccess, "channelId" | "removedAt" | "folderGrants">[] } | null,
  channelId: string | null,
): boolean {
  const channel = access?.channels.find((entry) => entry.channelId === channelId && entry.removedAt === null);
  return channel?.folderGrants.some((grant) => grant.revokedAt === null && grant.capability !== "none") ?? false;
}
