import {
  ACTIVITY_STATUSES,
  CANONICAL_ITEM_KINDS,
  CONTENT_STREAM_KINDS,
  MESSAGE_STREAMING_STATES,
} from "$lib/chat/contracts";
import type {
  ActivityStatus,
  ChangedFileSummary,
  ChatTimelineItemRead,
  ChatTimelineTurnRead,
  ChatTurnId,
  ChatTurnState,
  PlanStep,
  UtcTimestamp,
  VersionedJson,
} from "$lib/chat/contracts";
import { normalizeChangedFileSummaries } from "$lib/chat/review/changed-files";

import { indexTimelineRowsByTurn } from "./row-index";
import type {
  TimelineActivityGroupRow,
  TimelineActivityRow,
  TimelineAttachmentSummary,
  TimelineDisplayRow,
  TimelineMentionSummary,
  TimelineMessagePhase,
  TimelineMessageRow,
  TimelineMessageState,
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
  if (activity.detailBytes > 0) return true;
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

/**
 * Converts validated paged SQLite projection DTOs into the shared timeline model.
 *
 * @param items Timeline items of the loaded window.
 * @param turnReads Turn summaries of the loaded window.
 * @returns Rows sorted by sequence and the turn summaries, with turn outcomes folded into assistant rows.
 */
export function projectTimelineReadModel(
  items: readonly ChatTimelineItemRead[],
  turnReads: readonly ChatTimelineTurnRead[],
): TimelineProjection {
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
        changeSequence: latestChangeSequence(rows.get(item.activityId), item),
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
            : jsonNullableString(data.detailPreview) ?? "",
          state: messageState(status),
          phase: parseMessagePhase(data.metadata),
          userContext: null,
          metadata: current?.kind === "message" ? current.metadata : null,
          sourceThreadId: item.sourceThreadId,
          changeSequence: latestChangeSequence(current, item),
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
        ...storedActivityDetail(data),
        metadata: data.metadata === undefined ? null : { schemaVersion: item.data.schemaVersion, value: data.metadata },
        sourceThreadId: item.sourceThreadId,
        changeSequence: item.changeSequence,
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
        changeSequence: item.changeSequence,
      });
    }
  }
  const rowsByTurn = indexTimelineRowsByTurn(rows.values());
  attachPendingTurnRows(rows, turns, rowsByTurn);
  attachTerminalAssistantMetadata(turns, rowsByTurn);
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

/**
 * Reuses the previous object of every row that did not change, so keyed rendering skips it.
 *
 * Run it on projected rows first and then on the display rows built from them: a fold or
 * group row is reused only when its children are the same objects.
 *
 * @param previous Rows returned by the previous call.
 * @param next Rows of the current projection.
 * @returns `previous` itself when every row is reused in the same order, else the rows of
 * `next` in its order with unchanged rows replaced by their previous objects.
 */
export function stabilizeTimelineRows<Row extends TimelineDisplayRow>(
  previous: readonly Row[],
  next: readonly Row[],
): readonly Row[] {
  const previousById = new Map<string, Row>();
  for (const row of previous) previousById.set(row.id, row);
  let reusedInOrder = previous.length === next.length;
  const stable = next.map((row, index) => {
    const candidate = previousById.get(row.id);
    const reused = candidate !== undefined && isTimelineRowUnchanged(candidate, row) ? candidate : row;
    if (reused !== previous[index]) reusedInOrder = false;
    return reused;
  });
  return reusedInOrder ? previous : stable;
}

/**
 * Returns whether a row can keep its previous object.
 *
 * Stored rows compare their change clock and the turn outcome folded into assistant rows.
 * Rows built in the client compare their own fields. Folds and groups compare their
 * children by reference.
 */
function isTimelineRowUnchanged(previous: TimelineDisplayRow, next: TimelineDisplayRow): boolean {
  if (previous === next) return true;
  if (previous.id !== next.id) return false;
  if (previous.kind === "turn_fold" || next.kind === "turn_fold") {
    return previous.kind === "turn_fold" && next.kind === "turn_fold" && isTurnFoldUnchanged(previous, next);
  }
  if (previous.kind === "activity_group" || next.kind === "activity_group") {
    return previous.kind === "activity_group" && next.kind === "activity_group" && isActivityGroupUnchanged(previous, next);
  }
  return isProjectedRowUnchanged(previous, next);
}

function isProjectedRowUnchanged(previous: TimelineRow, next: TimelineRow): boolean {
  if (previous.kind !== next.kind) return false;
  if (previous.changeSequence === undefined || next.changeSequence === undefined) return haveSameFields(previous, next);
  if (previous.changeSequence !== next.changeSequence || previous.sequence !== next.sequence) return false;
  if (previous.kind !== "message" || next.kind !== "message") return true;
  return previous.state === next.state && isSameJsonLike(previous.metadata, next.metadata);
}

function isTurnFoldUnchanged(previous: TimelineTurnFoldRow, next: TimelineTurnFoldRow): boolean {
  return previous.state === next.state
    && previous.durationMs === next.durationMs
    && previous.expanded === next.expanded
    && previous.sequence === next.sequence
    && previous.createdAt === next.createdAt
    && previous.sourceThreadId === next.sourceThreadId
    && previous.hiddenRows.length === next.hiddenRows.length
    && previous.hiddenRows.every((row, index) => {
      const nextRow = next.hiddenRows[index];
      if (row === nextRow) return true;
      return row.kind === "activity_group" && nextRow?.kind === "activity_group" && row.id === nextRow.id
        && isActivityGroupUnchanged(row, nextRow);
    });
}

function isActivityGroupUnchanged(previous: TimelineActivityGroupRow, next: TimelineActivityGroupRow): boolean {
  return previous.latest === next.latest
    && previous.expanded === next.expanded
    && previous.earlierRows.length === next.earlierRows.length
    && previous.earlierRows.every((row, index) => row === next.earlierRows[index]);
}

/** Compares the own fields of two rows built in the client, by value for nested data. */
function haveSameFields(previous: TimelineRow, next: TimelineRow): boolean {
  const previousEntries = Object.entries(previous);
  if (previousEntries.length !== Object.keys(next).length) return false;
  const nextRecord: Record<string, unknown> = { ...next };
  return previousEntries.every(([key, value]) => key in nextRecord && isSameJsonLike(value, nextRecord[key]));
}

/** Compares plain data built from JSON (primitives, arrays, and plain objects) by value. */
function isSameJsonLike(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  if (Array.isArray(left)) {
    return Array.isArray(right)
      && left.length === right.length
      && left.every((value, index) => isSameJsonLike(value, right[index]));
  }
  if (typeof left !== "object" || left === null || typeof right !== "object" || right === null || Array.isArray(right)) return false;
  const leftEntries = Object.entries(left);
  if (leftEntries.length !== Object.keys(right).length) return false;
  const rightRecord: Record<string, unknown> = { ...right };
  return leftEntries.every(([key, value]) => key in rightRecord && isSameJsonLike(value, rightRecord[key]));
}

function isTransientThinkingActivity(row: TimelineActivityRow): boolean {
  return row.id.startsWith("turn-pending:")
    || row.activityKind === "reasoning"
    || row.activityKind === "reasoning_text"
    || row.activityKind === "reasoning_summary";
}

type ActivityDetailFields = Pick<TimelineActivityRow, "detailPreview" | "detailBytes" | "detailTruncated" | "updatedAt">;

/** Describes detail text a row holds in full. */
function wholeActivityDetail(detail: string | null): ActivityDetailFields {
  return { detailPreview: detail, detailBytes: detail === null ? 0 : utf8ByteLength(detail), detailTruncated: false };
}

/** Reads the shortened detail a native timeline read stores on an activity row. */
function storedActivityDetail(data: Record<string, VersionedJson["value"]>): ActivityDetailFields {
  const detailPreview = jsonNullableString(data.detailPreview);
  const detailBytes = typeof data.detailBytes === "number" && Number.isSafeInteger(data.detailBytes) && data.detailBytes >= 0
    ? data.detailBytes
    : detailPreview === null ? 0 : utf8ByteLength(detailPreview);
  const updatedAt = jsonString(data.updatedAt);
  return {
    detailPreview,
    detailBytes,
    detailTruncated: data.detailTruncated === true,
    ...(updatedAt ? { updatedAt } : {}),
  };
}

/** Counts the UTF-8 bytes of a string without allocating its encoding. */
function utf8ByteLength(value: string): number {
  let bytes = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code < 0x80) bytes += 1;
    else if (code < 0x800) bytes += 2;
    else if (code >= 0xd800 && code <= 0xdbff && index + 1 < value.length) {
      const next = value.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        bytes += 4;
        index += 1;
      } else bytes += 3;
    } else bytes += 3;
  }
  return bytes;
}

/** Folds each terminal turn's outcome into its final assistant row. */
function attachTerminalAssistantMetadata(
  turns: ReadonlyMap<ChatTurnId, TimelineTurn>,
  rowsByTurn: ReadonlyMap<ChatTurnId, readonly TimelineRow[]>,
): void {
  for (const turn of turns.values()) {
    if (!isTerminalTurn(turn.state)) continue;
    let latestAssistant: TimelineMessageRow | null = null;
    let latestFinalAnswer: TimelineMessageRow | null = null;
    for (const row of rowsByTurn.get(turn.id) ?? []) {
      if (row.kind !== "message" || row.role !== "assistant") continue;
      if (!latestAssistant || row.sequence > latestAssistant.sequence) latestAssistant = row;
      if (row.phase === "final_answer" && (!latestFinalAnswer || row.sequence > latestFinalAnswer.sequence)) latestFinalAnswer = row;
    }
    const assistant = latestFinalAnswer ?? latestAssistant;
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

/** Adds a working placeholder to each live turn that has no provider output yet. */
function attachPendingTurnRows(
  rows: Map<string, TimelineRow>,
  turns: ReadonlyMap<ChatTurnId, TimelineTurn>,
  rowsByTurn: ReadonlyMap<ChatTurnId, readonly TimelineRow[]>,
): void {
  for (const turn of turns.values()) {
    if (turn.state !== "pending" && turn.state !== "dispatching" && turn.state !== "active") continue;
    const turnRows = rowsByTurn.get(turn.id) ?? [];
    const hasProviderOutput = turnRows.some((row) => row.kind !== "message"
      || (row.role === "assistant" && row.markdown.trim().length > 0));
    if (hasProviderOutput) continue;
    let latest: TimelineRow | null = null;
    for (const row of turnRows) {
      if (!latest || row.sequence > latest.sequence) latest = row;
    }
    rows.set(`turn-pending:${turn.id}`, {
      id: `turn-pending:${turn.id}`,
      kind: "activity",
      turnId: turn.id,
      sequence: (latest?.sequence ?? 0) + 1,
      createdAt: latest?.createdAt ?? turn.startedAt,
      activityKind: "notice",
      status: "active",
      title: "Working",
      ...wholeActivityDetail(null),
      metadata: null,
    });
  }
}

/** Returns the newest change clock of a row merged from several stored items. */
function latestChangeSequence(current: TimelineRow | undefined, item: ChatTimelineItemRead): number {
  return Math.max(current?.changeSequence ?? 0, item.changeSequence);
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
