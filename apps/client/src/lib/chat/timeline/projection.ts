import {
  ACTIVITY_STATUSES,
  CANONICAL_ITEM_KINDS,
  CONTENT_STREAM_KINDS,
  MESSAGE_STREAMING_STATES,
} from "$lib/chat/contracts";
import type {
  ActivityStatus,
  CanonicalItemKind,
  CanonicalStoredEvent,
  ChangedFileSummary,
  ChatTimelineItemRead,
  ChatTimelineTurnRead,
  ChatTurnId,
  ChatTurnState,
  ContentDeltaEvent,
  ContentStreamKind,
  ItemLifecycleEvent,
  PlanStep,
  ProposedPlanDeltaEvent,
  ThreadUsageUpdatedEvent,
  TurnModeSnapshot,
  UtcTimestamp,
  VersionedJson,
} from "$lib/chat/contracts";
import { normalizeChangedFileSummaries } from "$lib/chat/review/changed-files";

import type {
  TimelineActivityGroupRow,
  TimelineActivityRow,
  TimelineAttachmentSummary,
  TimelineDisplayRow,
  TimelineMentionSummary,
  TimelineMessagePhase,
  TimelineMessageRow,
  TimelineMessageState,
  TimelinePendingRequest,
  TimelinePlanRow,
  TimelineProjection,
  TimelineRow,
  TimelineTurn,
  TimelineTurnFoldRow,
  TimelineUserContext,
} from "./types";

/** Returns whether an activity owns an inspectable detail disclosure in the conversation UI. */
export function timelineActivitySupportsDisclosure(activity: TimelineActivityRow): boolean {
  if (activity.id.startsWith("turn-pending:")
    || activity.activityKind === "reasoning"
    || activity.activityKind === "reasoning_text"
    || activity.activityKind === "reasoning_summary"
    || activity.title === "thread_reverted") return false;
  if (activity.activityKind === "command_execution"
    || activity.activityKind === "command_output"
    || activity.activityKind === "file_change"
    || activity.activityKind === "file_change_output") return true;
  if (activity.detail?.trim()) return true;
  const metadata = activity.metadata?.value;
  return typeof metadata === "object"
    && metadata !== null
    && !Array.isArray(metadata)
    && Object.keys(metadata).length > 0;
}

/**
 * Returns whether a visible activity represents ongoing work in the timeline.
 *
 * Providers may complete a reasoning item before emitting the next real item. The
 * latest reasoning row remains the transient Thinking placeholder while its turn
 * is live, so it should retain the same active presentation as pending work.
 *
 * @param activity Activity row being presented.
 * @param turnState Current state of the activity's parent turn when available.
 * @returns Whether the activity should use the live status presentation.
 */
export function timelineActivityShowsLiveStatus(
  activity: TimelineActivityRow,
  turnState: ChatTurnState | null | undefined,
): boolean {
  if (activity.status === "pending" || activity.status === "active" || activity.status === "waiting") return true;
  return isTransientThinkingActivity(activity)
    && (turnState === "pending" || turnState === "dispatching" || turnState === "active");
}

/** Adds a local user message until the matching durable projection arrives. */
export function includeOptimisticTimelineMessage(
  rows: readonly TimelineRow[],
  optimisticMessage: TimelineMessageRow | null,
): TimelineRow[] {
  if (!optimisticMessage || rows.some((row) => row.id === optimisticMessage.id)) return [...rows];
  return [...rows, optimisticMessage].sort(compareRows);
}

/** Limits an embedded execution timeline to the exact provider-neutral run turn. */
export function timelineRowsForTurn(
  rows: readonly TimelineRow[],
  turnId: ChatTurnId | null,
): TimelineRow[] {
  return turnId === null ? [...rows] : rows.filter((row) => row.turnId === turnId);
}

/** Finds the first model-owned display row in each turn for participant headers. */
export function timelineModelGroupStartIds(
  rows: readonly TimelineDisplayRow[],
): Set<string> {
  const seenTurns = new Set<ChatTurnId>();
  const startIds = new Set<string>();
  for (const row of rows) {
    if (!row.turnId || (row.kind === "message" && row.role === "user")) continue;
    if (seenTurns.has(row.turnId)) continue;
    seenTurns.add(row.turnId);
    startIds.add(row.id);
  }
  return startIds;
}

interface MutableStreamRow {
  row: TimelineMessageRow | TimelineActivityRow | TimelinePlanRow;
  parts: Map<number, string>;
}

const DEFAULT_MODES: TurnModeSnapshot = {
  safetyMode: "ask_for_approval",
  interactionMode: "build",
};

/**
 * Projects validated canonical events into stable, provider-neutral timeline state.
 *
 * @param events Canonical events from one thread, in any sequence order.
 * @returns Stable timeline rows, turn summaries, and unresolved requests.
 */
export function projectCanonicalTimeline(events: readonly CanonicalStoredEvent[]): TimelineProjection {
  const rows = new Map<string, TimelineRow>();
  const streams = new Map<string, MutableStreamRow>();
  const turns = new Map<ChatTurnId, TimelineTurn>();
  const requests = new Map<string, TimelinePendingRequest>();
  const seenEventIds = new Set<string>();
  const ignoredDuplicateEventIds: string[] = [];
  let threadUsage: ThreadUsageUpdatedEvent | null = null;

  const orderedEvents = [...events].sort((left, right) => left.sequence - right.sequence);
  for (const event of orderedEvents) {
    if (seenEventIds.has(event.eventId)) {
      ignoredDuplicateEventIds.push(event.eventId);
      continue;
    }
    seenEventIds.add(event.eventId);

    switch (event.event.type) {
      case "turn_started": {
        if (!event.turnId) break;
        turns.set(event.turnId, {
          id: event.turnId,
          state: event.event.payload.state,
          startedAt: event.createdAt,
          completedAt: null,
          durationMs: null,
          modes: event.event.payload.modes,
          modelId: event.event.payload.modelId,
          effectiveModelId: event.event.payload.modelId,
          usage: null,
          changedFiles: [],
          diffSources: [],
          stopReason: null,
          recoverable: false,
        });
        break;
      }
      case "turn_completed": {
        const turn = ensureTurn(turns, event);
        if (!turn) break;
        turn.state = event.event.payload.state;
        turn.completedAt = event.createdAt;
        turn.durationMs = elapsedMilliseconds(turn.startedAt, event.createdAt);
        turn.usage = event.event.payload.usage;
        turn.changedFiles = mergeChangedFiles(turn.changedFiles, event.event.payload.changedFiles);
        turn.stopReason = event.event.payload.stopReason;
        break;
      }
      case "turn_aborted": {
        const turn = ensureTurn(turns, event);
        if (!turn) break;
        turn.state = event.event.payload.state;
        turn.completedAt = event.createdAt;
        turn.durationMs = elapsedMilliseconds(turn.startedAt, event.createdAt);
        turn.stopReason = event.event.payload.reason;
        turn.recoverable = event.event.payload.recoverable;
        break;
      }
      case "thread_usage_updated":
        threadUsage = event.event.payload;
        break;
      case "thread_reverted":
        upsertActivity(
          rows,
          event,
          `activity:restore:${event.eventId}`,
          "notice",
          "completed",
          "thread_reverted",
          null,
          {
            schemaVersion: 1,
            value: {
              checkpointId: event.event.payload.checkpointId,
              revertedTurnIds: [...event.event.payload.revertedTurnIds],
              providerHistoryAction: event.event.payload.providerHistoryAction,
            },
          },
        );
        break;
      case "model_rerouted": {
        const turn = ensureTurn(turns, event);
        if (turn) turn.effectiveModelId = event.event.payload.effectiveModelId;
        upsertActivity(rows, event, `model:${event.sequence}`, "notice", "completed", "Model rerouted", event.event.payload.reason, null);
        break;
      }
      case "diff_updated": {
        const turn = ensureTurn(turns, event);
        if (!turn) break;
        turn.changedFiles = mergeChangedFiles(turn.changedFiles, event.event.payload.files);
        if (!turn.diffSources.includes(event.event.payload.source)) turn.diffSources.push(event.event.payload.source);
        break;
      }
      case "content_delta":
        appendContentDelta(rows, streams, event, event.event.payload);
        break;
      case "item_started":
      case "item_updated":
      case "item_completed":
        upsertLifecycleItem(rows, event, event.event.payload);
        break;
      case "plan_updated":
        rows.set(`plan:structured:${event.turnId ?? "thread"}`, {
          id: `plan:structured:${event.turnId ?? "thread"}`,
          kind: "plan",
          turnId: event.turnId,
          sequence: event.sequence,
          createdAt: event.createdAt,
          planKind: "structured",
          markdown: event.event.payload.markdown,
          steps: event.event.payload.steps,
          state: "complete",
        });
        break;
      case "proposed_plan_delta":
        appendPlanDelta(rows, streams, event, event.event.payload);
        break;
      case "proposed_plan_completed": {
        const id = `plan:proposed:${event.event.payload.planId}`;
        rows.set(id, {
          id,
          kind: "plan",
          turnId: event.turnId,
          sequence: rows.get(id)?.sequence ?? event.sequence,
          createdAt: rows.get(id)?.createdAt ?? event.createdAt,
          planKind: "proposed",
          markdown: event.event.payload.markdown,
          steps: [],
          state: "complete",
        });
        break;
      }
      case "request_opened":
        requests.set(event.event.payload.requestId, {
          id: event.event.payload.requestId,
          turnId: event.turnId,
          sequence: event.sequence,
          createdAt: event.createdAt,
          kind: "approval",
          title: event.event.payload.title,
          detail: event.event.payload.detail,
          questions: [],
          state: "open",
        });
        break;
      case "user_input_requested":
        requests.set(event.event.payload.requestId, {
          id: event.event.payload.requestId,
          turnId: event.turnId,
          sequence: event.sequence,
          createdAt: event.createdAt,
          kind: "user_input",
          title: event.event.payload.questions[0]?.question ?? "Provider question",
          detail: null,
          questions: event.event.payload.questions,
          state: "open",
        });
        break;
      case "request_resolved":
      case "user_input_resolved": {
        const request = requests.get(event.event.payload.requestId);
        if (request) request.state = event.event.payload.state;
        break;
      }
      case "task_lifecycle":
        upsertActivity(rows, event, `task:${event.event.payload.taskId}`, "task", event.event.payload.status, event.event.payload.title, event.event.payload.detail, event.event.payload.safeMetadata);
        break;
      case "hook_lifecycle":
        upsertActivity(rows, event, `hook:${event.event.payload.hookId}`, "hook", event.event.payload.status, event.event.payload.title, event.event.payload.detail, null);
        break;
      case "tool_progress":
        upsertActivity(rows, event, `tool:${event.event.payload.toolId}`, "tool", event.event.payload.status, event.event.payload.title, event.event.payload.summary, null);
        break;
      case "mcp_status":
        upsertActivity(rows, event, `mcp:${event.event.payload.serverId}`, "mcp", event.event.payload.status, event.event.payload.serverId, event.event.payload.detail, null);
        break;
      case "mcp_oauth_completed":
        upsertActivity(rows, event, `mcp-oauth:${event.event.payload.serverId}:${event.sequence}`, "mcp", event.event.payload.successful ? "completed" : "failed", event.event.payload.serverId, event.event.payload.detail, null);
        break;
      case "configuration_warning":
      case "deprecation_notice":
      case "runtime_warning":
        upsertActivity(rows, event, `notice:${event.eventId}`, "notice", "unknown", event.event.payload.title, event.event.payload.detail, null);
        break;
      case "runtime_error":
        upsertActivity(rows, event, `error:${event.eventId}`, "error", "failed", event.event.payload.message, event.event.payload.code, event.event.payload.safeDetails);
        break;
      case "unknown":
        upsertActivity(rows, event, `unknown:${event.eventId}`, "unknown", "unknown", event.event.payload.summary, event.event.payload.sourceType, event.event.payload.safePayload);
        break;
      default:
        break;
    }
  }

  attachPendingTurnRows(rows, turns);
  attachTerminalAssistantMetadata(rows, turns);
  return {
    rows: [...rows.values()].sort(compareRows),
    turns: [...turns.values()].sort((left, right) => left.startedAt.localeCompare(right.startedAt)),
    pendingRequests: [...requests.values()].sort((left, right) => left.sequence - right.sequence),
    threadUsage,
    ignoredDuplicateEventIds,
  };
}

/**
 * Converts validated paged SQLite projection DTOs into the shared timeline model.
 */
export function projectTimelineReadModel(
  items: readonly ChatTimelineItemRead[],
  turnReads: readonly ChatTimelineTurnRead[],
): Pick<TimelineProjection, "rows" | "turns"> {
  const turns = new Map<ChatTurnId, TimelineTurn>();
  for (const turn of turnReads) {
    turns.set(turn.turnId, {
      id: turn.turnId,
      state: turn.state,
      startedAt: turn.startedAt ?? turn.completedAt ?? "1970-01-01T00:00:00.000Z",
      completedAt: turn.completedAt,
      durationMs: turn.startedAt && turn.completedAt ? elapsedMilliseconds(turn.startedAt, turn.completedAt) : null,
      modes: turn.modes,
      modelId: turn.modelId,
      effectiveModelId: turn.modelId,
      usage: turn.usage,
      changedFiles: mergeChangedFiles([], turn.changedFiles),
      diffSources: [],
      stopReason: turn.stopReason,
      recoverable: false,
    });
  }
  const rows = new Map<string, TimelineRow>();
  for (const item of items) {
    const data = jsonRecord(item.data.value);
    if (!data) continue;
    const createdAt = jsonString(data.createdAt);
    if (!createdAt) continue;
    if (item.kind === "message") {
      const role = data.role === "user" || data.role === "assistant" ? data.role : null;
      const markdown = jsonString(data.markdown);
      const state = jsonString(data.streamingState);
      if (!role || markdown === null || !state || !isKnownValue(MESSAGE_STREAMING_STATES, state)) continue;
      rows.set(item.activityId, {
        id: item.activityId,
        kind: "message",
        role,
        turnId: item.turnId,
        sequence: item.sequenceAnchor,
        createdAt,
        markdown,
        state,
        phase: parseMessagePhase(data.metadata),
        userContext: role === "user" ? parseTimelineUserContext(data.metadata) : null,
        metadata: null,
        sourceThreadId: item.sourceThreadId,
      });
      continue;
    }
    if (item.kind === "activity") {
      const activityKind = jsonString(data.activityKind);
      const status = jsonString(data.status);
      const title = jsonString(data.title);
      if (!activityKind || !status || title === null || !isKnownValue(ACTIVITY_STATUSES, status)) continue;
      const knownKind = activityKind === "channel_session_boundary"
        ? activityKind
        : isKnownValue(CANONICAL_ITEM_KINDS, activityKind) || isKnownValue(CONTENT_STREAM_KINDS, activityKind)
        ? activityKind
        : "unknown";
      if (knownKind === "user_message") continue;
      if (knownKind === "assistant_message") {
        const current = rows.get(item.activityId);
        rows.set(item.activityId, {
          id: item.activityId,
          kind: "message",
          role: "assistant",
          turnId: item.turnId,
          sequence: current?.sequence ?? item.sequenceAnchor,
          createdAt: current?.createdAt ?? createdAt,
          markdown: current?.kind === "message" && current.markdown
            ? current.markdown
            : jsonNullableString(data.detail) ?? "",
          state: messageState(status),
          phase: parseMessagePhase(data.metadata),
          userContext: null,
          metadata: current?.kind === "message" ? current.metadata : null,
          sourceThreadId: item.sourceThreadId,
        });
        continue;
      }
      rows.set(item.activityId, {
        id: item.activityId,
        kind: "activity",
        turnId: item.turnId,
        sequence: item.sequenceAnchor,
        createdAt,
        activityKind: knownKind,
        status,
        title,
        detail: jsonNullableString(data.detail),
        metadata: data.metadata === undefined ? null : { schemaVersion: item.data.schemaVersion, value: data.metadata },
        sourceThreadId: item.sourceThreadId,
      });
      continue;
    }
    if (item.kind === "plan") {
      const markdown = jsonString(data.markdown);
      if (markdown === null) continue;
      rows.set(item.activityId, {
        id: item.activityId,
        kind: "plan",
        turnId: item.turnId,
        sequence: item.sequenceAnchor,
        createdAt,
        planKind: "proposed",
        markdown,
        steps: parseProjectedPlanSteps(data.steps),
        state: data.state === "proposed" ? "streaming" : "complete",
        sourceThreadId: item.sourceThreadId,
      });
    }
  }
  attachPendingTurnRows(rows, turns);
  attachTerminalAssistantMetadata(rows, turns);
  return { rows: [...rows.values()].sort(compareRows), turns: [...turns.values()] };
}

/**
 * Converts projected rows into the compact display model used by the timeline.
 *
 * @param rows Stable projected timeline rows.
 * @param turns Turn summaries associated with the rows.
 * @param expandedTurnIds Turn folds expanded in the current window session.
 * @param expandedGroupIds Activity groups expanded in the current window session.
 * @returns Folded and grouped display rows with stable identifiers.
 */
export function buildTimelineDisplayRows(
  rows: readonly TimelineRow[],
  turns: readonly TimelineTurn[],
  expandedTurnIds: ReadonlySet<ChatTurnId> = new Set(),
  expandedGroupIds: ReadonlySet<string> = new Set(),
): TimelineDisplayRow[] {
  const turnsById = new Map(turns.map((turn) => [turn.id, turn]));
  const processRowsByTurn = new Map<ChatTurnId, (TimelineActivityRow | TimelineMessageRow)[]>();
  const processRowIds = new Set<string>();
  const liveVisibleRowIds = new Set<string>();
  for (const row of rows) {
    if (!row.turnId || !turnsById.has(row.turnId)) continue;
    if (row.kind !== "activity" && !(row.kind === "message" && row.role === "assistant")) continue;
    const processRows = processRowsByTurn.get(row.turnId) ?? [];
    processRows.push(row);
    processRowsByTurn.set(row.turnId, processRows);
    processRowIds.add(row.id);
  }

  for (const [turnId, processRows] of processRowsByTurn) {
    const turn = turnsById.get(turnId);
    if (!turn || isTerminalTurn(turn.state)) continue;
    for (const processRow of processRows) {
      if (processRow.kind === "message") {
        if (processRow.markdown.trim()) liveVisibleRowIds.add(processRow.id);
      } else if (!isTransientThinkingActivity(processRow)) {
        liveVisibleRowIds.add(processRow.id);
      }
    }
    const currentPlaceholder = [...processRows]
      .reverse()
      .find((processRow) => (
        processRow.kind === "message"
          ? processRow.markdown.trim().length > 0
          : true
      ));
    if (currentPlaceholder?.kind === "activity" && isTransientThinkingActivity(currentPlaceholder)) {
      liveVisibleRowIds.add(currentPlaceholder.id);
    }
  }

  const emittedFolds = new Set<ChatTurnId>();
  const displayRows: TimelineDisplayRow[] = [];
  for (const row of rows) {
    const processRows = row.turnId ? processRowsByTurn.get(row.turnId) : undefined;
    if (!row.turnId || !processRows || !processRowIds.has(row.id)) {
      displayRows.push(row);
      continue;
    }
    const turn = turnsById.get(row.turnId);
    if (!turn) continue;

    if (!isTerminalTurn(turn.state)) {
      if (liveVisibleRowIds.has(row.id)) displayRows.push(row);
      continue;
    }

    const phasedFinalAssistant = [...processRows]
      .reverse()
      .find((processRow): processRow is TimelineMessageRow => (
        processRow.kind === "message"
        && processRow.phase === "final_answer"
        && processRow.markdown.trim().length > 0
      ));
    const hasMessagePhases = processRows.some(
      (processRow) => processRow.kind === "message" && processRow.phase !== null,
    );
    const finalCandidate = hasMessagePhases
      ? phasedFinalAssistant
      : [...processRows]
          .reverse()
          .find((processRow) => (
            processRow.kind === "message"
              ? processRow.markdown.trim().length > 0
              : !isTransientThinkingActivity(processRow)
          ));
    const finalAssistant = finalCandidate?.kind === "message" ? finalCandidate : undefined;
    const hiddenRows = processRows.filter((processRow) => (
      processRow.id !== finalAssistant?.id
      && !(processRow.kind === "message" && processRow.markdown.trim().length === 0)
      && !(processRow.kind === "activity" && isTransientThinkingActivity(processRow))
    ));
    const foldRequired = hiddenRows.length > 0 || turn.state !== "completed";
    if (foldRequired && !emittedFolds.has(row.turnId)) {
      displayRows.push({
        id: `turn-fold:${row.turnId}`,
        kind: "turn_fold",
        turnId: row.turnId,
        sequence: processRows[0]?.sequence ?? row.sequence,
        createdAt: processRows[0]?.createdAt ?? row.createdAt,
        state: turn.state,
        durationMs: turn.durationMs,
        hiddenRows: groupConsecutiveActivities(hiddenRows, expandedGroupIds),
        expanded: hiddenRows.length > 0 && expandedTurnIds.has(row.turnId),
        sourceThreadId: processRows[0]?.sourceThreadId,
      });
      emittedFolds.add(row.turnId);
    }
    if (row.id === finalAssistant?.id) displayRows.push(finalAssistant);
  }

  return groupConsecutiveActivities(displayRows, expandedGroupIds);
}

function isTransientThinkingActivity(row: TimelineActivityRow): boolean {
  return row.id.startsWith("turn-pending:")
    || row.activityKind === "reasoning"
    || row.activityKind === "reasoning_text"
    || row.activityKind === "reasoning_summary";
}

function ensureTurn(turns: Map<ChatTurnId, TimelineTurn>, event: CanonicalStoredEvent): TimelineTurn | null {
  if (!event.turnId) return null;
  const existing = turns.get(event.turnId);
  if (existing) return existing;
  const turn: TimelineTurn = {
    id: event.turnId,
    state: "active",
    startedAt: event.createdAt,
    completedAt: null,
    durationMs: null,
    modes: DEFAULT_MODES,
    modelId: null,
    effectiveModelId: null,
    usage: null,
    changedFiles: [],
    diffSources: [],
    stopReason: null,
    recoverable: false,
  };
  turns.set(event.turnId, turn);
  return turn;
}

function appendContentDelta(
  rows: Map<string, TimelineRow>,
  streams: Map<string, MutableStreamRow>,
  event: CanonicalStoredEvent,
  payload: ContentDeltaEvent,
): void {
  const assistant = payload.streamKind === "assistant_text";
  const id = assistant ? `message:${payload.itemId}` : `activity:${payload.itemId}`;
  if (payload.streamKind === "reasoning_text") {
    if (!rows.has(id)) {
      rows.set(id, {
        id,
        kind: "activity",
        turnId: event.turnId,
        sequence: event.sequence,
        createdAt: event.createdAt,
        activityKind: "reasoning_text",
        status: "active",
        title: activityTitle(payload.streamKind),
        detail: null,
        metadata: null,
      });
    }
    return;
  }
  let stream = streams.get(id);
  if (!stream) {
    const row: TimelineMessageRow | TimelineActivityRow = assistant
      ? { id, kind: "message", role: "assistant", turnId: event.turnId, sequence: event.sequence, createdAt: event.createdAt, markdown: "", state: "streaming", phase: null, userContext: null, metadata: null }
      : { id, kind: "activity", turnId: event.turnId, sequence: event.sequence, createdAt: event.createdAt, activityKind: payload.streamKind, status: "active", title: activityTitle(payload.streamKind), detail: null, metadata: null };
    stream = { row, parts: new Map() };
    streams.set(id, stream);
    rows.set(id, row);
  }
  stream.parts.set(payload.contentIndex, `${stream.parts.get(payload.contentIndex) ?? ""}${payload.delta}`);
  const content = [...stream.parts.entries()].sort(([left], [right]) => left - right).map(([, value]) => value).join("");
  if (stream.row.kind === "message") stream.row.markdown = content;
  else if (stream.row.kind === "activity") {
    stream.row.activityKind = payload.streamKind;
    stream.row.detail = content;
  }
}

function appendPlanDelta(
  rows: Map<string, TimelineRow>,
  streams: Map<string, MutableStreamRow>,
  event: CanonicalStoredEvent,
  payload: ProposedPlanDeltaEvent,
): void {
  const id = `plan:proposed:${payload.planId}`;
  let stream = streams.get(id);
  if (!stream) {
    const row: TimelinePlanRow = { id, kind: "plan", turnId: event.turnId, sequence: event.sequence, createdAt: event.createdAt, planKind: "proposed", markdown: "", steps: [], state: "streaming" };
    stream = { row, parts: new Map() };
    streams.set(id, stream);
    rows.set(id, row);
  }
  stream.parts.set(payload.contentIndex, `${stream.parts.get(payload.contentIndex) ?? ""}${payload.delta}`);
  if (stream.row.kind === "plan") {
    stream.row.markdown = [...stream.parts.entries()].sort(([left], [right]) => left - right).map(([, value]) => value).join("");
  }
}

function upsertLifecycleItem(
  rows: Map<string, TimelineRow>,
  event: CanonicalStoredEvent,
  payload: ItemLifecycleEvent,
): void {
  if (payload.kind === "user_message") return;
  if (payload.kind === "assistant_message") {
    const id = `message:${payload.itemId}`;
    const current = rows.get(id);
    rows.set(id, {
      id,
      kind: "message",
      role: "assistant",
      turnId: event.turnId,
      sequence: current?.sequence ?? event.sequence,
      createdAt: current?.createdAt ?? event.createdAt,
      markdown: current?.kind === "message" && current.markdown ? current.markdown : payload.detail ?? "",
      state: messageState(payload.status),
      phase: parseMessagePhase(payload.safeMetadata?.value),
      userContext: null,
      metadata: current?.kind === "message" ? current.metadata : null,
    });
    return;
  }
  upsertActivity(rows, event, `activity:${payload.itemId}`, payload.kind, payload.status, payload.title ?? activityTitle(payload.kind), payload.detail, payload.safeMetadata);
}

function upsertActivity(
  rows: Map<string, TimelineRow>,
  event: CanonicalStoredEvent,
  id: string,
  activityKind: TimelineActivityRow["activityKind"],
  status: ActivityStatus,
  title: string,
  detail: string | null,
  metadata: VersionedJson | null,
): void {
  const current = rows.get(id);
  const resolvedActivityKind = activityKind === "reasoning"
    && current?.kind === "activity"
    && (current.activityKind === "reasoning_summary" || current.activityKind === "reasoning_text")
    ? current.activityKind
    : activityKind;
  rows.set(id, {
    id,
    kind: "activity",
    turnId: event.turnId,
    sequence: current?.sequence ?? event.sequence,
    createdAt: current?.createdAt ?? event.createdAt,
    activityKind: resolvedActivityKind,
    status,
    title,
    detail: detail && detail.length > 0
      ? detail
      : current?.kind === "activity" ? current.detail : null,
    metadata,
  });
}

function attachTerminalAssistantMetadata(rows: Map<string, TimelineRow>, turns: Map<ChatTurnId, TimelineTurn>): void {
  for (const turn of turns.values()) {
    if (!isTerminalTurn(turn.state)) continue;
    const assistantRows = [...rows.values()].filter((row): row is TimelineMessageRow => row.kind === "message" && row.role === "assistant" && row.turnId === turn.id);
    assistantRows.sort((left, right) => right.sequence - left.sequence);
    const assistant = assistantRows.find((row) => row.phase === "final_answer")
      ?? assistantRows[0];
    if (!assistant) continue;
    assistant.state = turn.state === "completed" ? "complete" : turn.state === "interrupted" ? "interrupted" : "failed";
    assistant.metadata = {
      durationMs: turn.durationMs,
      modelId: turn.effectiveModelId,
      usage: turn.usage,
      changedFiles: turn.changedFiles,
      diffSources: turn.diffSources,
    };
  }
}

function attachPendingTurnRows(rows: Map<string, TimelineRow>, turns: Map<ChatTurnId, TimelineTurn>): void {
  for (const turn of turns.values()) {
    if (turn.state !== "pending" && turn.state !== "dispatching" && turn.state !== "active") continue;
    const turnRows = [...rows.values()].filter((row) => row.turnId === turn.id);
    const hasProviderOutput = turnRows.some((row) => row.kind !== "message"
      || (row.role === "assistant" && row.markdown.trim().length > 0));
    if (hasProviderOutput) continue;
    const latest = turnRows.sort((left, right) => right.sequence - left.sequence)[0];
    rows.set(`turn-pending:${turn.id}`, {
      id: `turn-pending:${turn.id}`,
      kind: "activity",
      turnId: turn.id,
      sequence: (latest?.sequence ?? 0) + 1,
      createdAt: latest?.createdAt ?? turn.startedAt,
      activityKind: "notice",
      status: "active",
      title: "Working",
      detail: null,
      metadata: null,
    });
  }
}

function mergeChangedFiles(current: readonly ChangedFileSummary[], incoming: readonly ChangedFileSummary[]): ChangedFileSummary[] {
  return normalizeChangedFileSummaries([...current, ...incoming]);
}

function elapsedMilliseconds(start: UtcTimestamp, end: UtcTimestamp): number | null {
  const elapsed = Date.parse(end) - Date.parse(start);
  return Number.isFinite(elapsed) && elapsed >= 0 ? elapsed : null;
}

function messageState(status: ActivityStatus): TimelineMessageState {
  if (status === "completed") return "complete";
  if (status === "interrupted") return "interrupted";
  if (status === "failed") return "failed";
  return "streaming";
}

function isTerminalTurn(state: ChatTurnState): state is TimelineTurnFoldRow["state"] {
  return state === "completed" || state === "interrupted" || state === "failed";
}

function activityTitle(kind: CanonicalItemKind | ContentStreamKind): string {
  return kind.replaceAll("_", " ");
}

function compareRows(left: TimelineRow, right: TimelineRow): number {
  return left.sequence - right.sequence || left.id.localeCompare(right.id);
}

function jsonRecord(value: VersionedJson["value"] | undefined): Record<string, VersionedJson["value"]> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value
    : null;
}

function jsonString(value: VersionedJson["value"] | undefined): string | null {
  return typeof value === "string" ? value : null;
}

function parseMessagePhase(value: VersionedJson["value"] | undefined): TimelineMessagePhase | null {
  const record = jsonRecord(value);
  return record?.phase === "commentary" || record?.phase === "final_answer"
    ? record.phase
    : null;
}

function jsonNullableString(value: VersionedJson["value"] | undefined): string | null {
  return value === null || value === undefined ? null : jsonString(value);
}

function isKnownValue<const Values extends readonly string[]>(values: Values, value: string): value is Values[number] {
  return values.some((candidate) => candidate === value);
}

function parseProjectedPlanSteps(value: VersionedJson["value"] | undefined): PlanStep[] {
  if (!Array.isArray(value)) return [];
  const steps: PlanStep[] = [];
  for (const candidate of value) {
    const record = jsonRecord(candidate);
    if (!record) continue;
    const id = jsonString(record.id);
    const text = jsonString(record.text);
    const status = jsonString(record.status);
    if (!id || text === null || !status || !isKnownValue(ACTIVITY_STATUSES, status)) continue;
    steps.push({ id, text, status });
  }
  return steps;
}

/** Parses bounded, presentation-safe context metadata attached to a user message. */
export function parseTimelineUserContext(value: VersionedJson["value"] | undefined): TimelineUserContext | null {
  const record = jsonRecord(value);
  if (!record) return null;
  const attachments = parseUserAttachments(record.attachments).slice(0, 20);
  const mentions = parseUserMentions(record.mentions).slice(0, 100);
  const terminalContext = parseTerminalContext(record.terminalContext).slice(0, 20);
  const preCheckpointId = jsonString(record.preCheckpointId);
  if (attachments.length === 0 && mentions.length === 0 && terminalContext.length === 0 && !preCheckpointId) return null;
  return { attachments, mentions, terminalContext, preCheckpointId };
}

function parseUserAttachments(value: VersionedJson["value"] | undefined): TimelineAttachmentSummary[] {
  if (!Array.isArray(value)) return [];
  const attachments: TimelineAttachmentSummary[] = [];
  for (const candidate of value) {
    const record = jsonRecord(candidate);
    const attachmentId = record ? jsonString(record.attachmentId) : null;
    const displayName = record ? jsonString(record.displayName) : null;
    const kind = record ? jsonString(record.kind) : null;
    const status = record ? jsonString(record.status) : null;
    const byteSize = record?.byteSize;
    if (!record || !attachmentId || !displayName || !kind || !status
      || typeof byteSize !== "number" || !Number.isSafeInteger(byteSize) || byteSize < 0) continue;
    attachments.push({
      attachmentId,
      displayName,
      kind,
      byteSize,
      status,
    });
  }
  return attachments;
}

function parseUserMentions(value: VersionedJson["value"] | undefined): TimelineMentionSummary[] {
  if (!Array.isArray(value)) return [];
  const mentions: TimelineMentionSummary[] = [];
  for (const candidate of value) {
    const record = jsonRecord(candidate);
    const relativePath = record ? jsonString(record.relativePath) : null;
    if (!record || !relativePath) continue;
    mentions.push({ relativePath, kind: jsonString(record.kind) });
  }
  return mentions;
}

function parseTerminalContext(value: VersionedJson["value"] | undefined): string[] {
  if (!Array.isArray(value)) return [];
  const context: string[] = [];
  for (const candidate of value) {
    const record = jsonRecord(candidate);
    const attachmentId = record ? jsonString(record.attachmentId) : null;
    const displayName = record ? jsonString(record.displayName) : null;
    const byteSize = record?.byteSize;
    if (attachmentId && displayName?.trim()
      && typeof byteSize === "number" && Number.isSafeInteger(byteSize) && byteSize >= 0) {
      context.push(displayName.slice(0, 240));
    }
  }
  return context;
}

function groupConsecutiveActivities(
  rows: readonly (TimelineActivityRow | TimelineMessageRow)[],
  expandedGroupIds: ReadonlySet<string>,
): (TimelineActivityRow | TimelineMessageRow | TimelineActivityGroupRow)[];
function groupConsecutiveActivities(
  rows: readonly TimelineDisplayRow[],
  expandedGroupIds: ReadonlySet<string>,
): TimelineDisplayRow[];
function groupConsecutiveActivities(
  rows: readonly TimelineDisplayRow[],
  expandedGroupIds: ReadonlySet<string>,
): TimelineDisplayRow[] {
  const result: TimelineDisplayRow[] = [];
  let run: TimelineActivityRow[] = [];
  const flush = (): void => {
    if (run.length === 0) {
      run = [];
      return;
    }
    const first = run[0];
    const latest = run.at(-1);
    if (!first || !latest) {
      run = [];
      return;
    }
    const id = `activity-group:${first.id}`;
    result.push({
      id,
      kind: "activity_group",
      turnId: first.turnId,
      sequence: first.sequence,
      createdAt: first.createdAt,
      latest,
      earlierRows: run.slice(0, -1),
      expanded: expandedGroupIds.has(id),
      sourceThreadId: first.sourceThreadId,
    });
    run = [];
  };

  for (const row of rows) {
    if (row.kind === "activity"
      && !isTransientThinkingActivity(row)
      && row.title !== "thread_reverted") {
      if (run.length === 0 || run[0]?.turnId === row.turnId) run.push(row);
      else {
        flush();
        run.push(row);
      }
      continue;
    }
    flush();
    result.push(row);
  }
  flush();
  return result;
}
