import type { CalendarEvent, PersistedSegment } from "$lib/calendar/types";
import { Temporal } from "@js-temporal/polyfill";

export interface DbPomodoroSegmentRow {
  id: string;
  event_id: string;
  event_date: string;
  run_id: string;
  rhythm_position: number;
  phase: PersistedSegment["phase"];
  planned_start: string;
  planned_end: string;
  actual_start: string | null;
  actual_end: string | null;
  pauses: PersistedSegment["pauseLog"];
  status: PersistedSegment["status"];
}

const MAX_HISTORY_ROWS = 10_000;
const MAX_HISTORY_PAUSES = 10_000;
const MAX_ID_BYTES = 1_036;
const encoder = new TextEncoder();

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isBoundedId(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= MAX_ID_BYTES
    && encoder.encode(value).byteLength <= MAX_ID_BYTES;
}

function isInstantString(value: unknown): value is string {
  if (typeof value !== "string" || value.length > 64) return false;
  try {
    Temporal.Instant.from(value);
    return true;
  } catch {
    return false;
  }
}

function isCivilDateString(value: unknown): value is string {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  try {
    Temporal.PlainDate.from(value);
    return true;
  } catch {
    return false;
  }
}

function isPauseEntry(value: unknown): value is PersistedSegment["pauseLog"][number] {
  return isRecord(value) && isInstantString(value.startedAt)
    && (value.endedAt === null || isInstantString(value.endedAt))
    && (value.reason === "manual" || value.reason === "idle" || value.reason === "suspend");
}

function isSegmentRow(value: unknown): value is DbPomodoroSegmentRow {
  return isRecord(value)
    && isBoundedId(value.id) && isBoundedId(value.event_id) && isBoundedId(value.run_id)
    && isCivilDateString(value.event_date)
    && typeof value.rhythm_position === "number" && Number.isSafeInteger(value.rhythm_position)
    && value.rhythm_position > 0
    && (value.phase === "focus" || value.phase === "short_break" || value.phase === "long_break")
    && isInstantString(value.planned_start) && isInstantString(value.planned_end)
    && (value.actual_start === null || isInstantString(value.actual_start))
    && (value.actual_end === null || isInstantString(value.actual_end))
    && Array.isArray(value.pauses) && value.pauses.length <= MAX_HISTORY_PAUSES
    && value.pauses.every(isPauseEntry)
    && (value.status === "planned" || value.status === "active" || value.status === "completed"
      || value.status === "skipped" || value.status === "interrupted");
}

/** Validate bounded native history before using it as Calendar timeline evidence. */
export function parsePomodoroSegmentRows(value: unknown, visibleIds: readonly string[]): DbPomodoroSegmentRow[] {
  if (!Array.isArray(value) || value.length > MAX_HISTORY_ROWS) {
    throw new Error("Invalid native Focus history size");
  }
  const visible = new Set(visibleIds);
  const identifiers = new Set<string>();
  let pauses = 0;
  const rows: DbPomodoroSegmentRow[] = [];
  for (const candidate of value) {
    if (!isSegmentRow(candidate) || !visible.has(candidate.event_id) || identifiers.has(candidate.id)) {
      throw new Error("Invalid native Focus history row");
    }
    pauses += candidate.pauses.length;
    if (pauses > MAX_HISTORY_PAUSES) throw new Error("Native Focus history exceeds its pause limit");
    identifiers.add(candidate.id);
    rows.push(candidate);
  }
  return rows;
}

export function visiblePomodoroEventIds(events: readonly CalendarEvent[]): string[] {
  return Array.from(
    new Set(
      events
        .filter((event) => event.pomodoroConfig)
        .map((event) => event.id),
    ),
  ).sort();
}

export function pomodoroSegmentSnapshotKey(
  segmentVersion: number,
  visibleIds: readonly string[],
): string {
  return `${segmentVersion}|${[...visibleIds].sort().join(",")}`;
}

/** Map native occurrence identities without deriving them from device-day dates. */
export function mapPomodoroSegmentRows(
  rows: readonly DbPomodoroSegmentRow[],
  visibleIds: readonly string[],
): Map<string, PersistedSegment[]> {
  const visible = new Set(visibleIds);
  const segmentsByEvent = new Map<string, PersistedSegment[]>();

  for (const row of rows) {
    if (!visible.has(row.event_id)) continue;
    const eventId = row.event_id;
    const segment: PersistedSegment = {
      id: row.id,
      eventId,
      eventDate: row.event_date,
      runId: row.run_id,
      rhythmPosition: row.rhythm_position,
      phase: row.phase,
      plannedStart: row.planned_start,
      plannedEnd: row.planned_end,
      actualStart: row.actual_start,
      actualEnd: row.actual_end,
      pauseLog: row.pauses,
      status: row.status,
    };
    const segments = segmentsByEvent.get(eventId);
    if (segments) segments.push(segment);
    else segmentsByEvent.set(eventId, [segment]);
  }

  return segmentsByEvent;
}
