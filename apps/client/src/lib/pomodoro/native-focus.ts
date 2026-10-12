import { isValidPomodoroConfig, type PomodoroConfig } from "./rhythm";
import type { PauseReason, PersistedSegment, SegmentPhase } from "$lib/calendar/types";

export type FocusMode = "stopped" | "running" | "manual_pause" | "idle_pause" | "idle_failed" | "suspended" | "return_wait" | "expired";
export type FocusIntent =
  | { kind: "start_scheduled"; occurrenceId: string | null }
  | { kind: "pause" | "resume" | "stop" | "advance" | "skip_break" | "dismiss_paused_prompts" }
  | { kind: "set_skip_next_break"; enabled: boolean }
  | { kind: "extend_focus" | "extend_break"; seconds: number }
  | { kind: "resolve_idle" | "resolve_suspend"; resume: boolean }
  | { kind: "set_idle_timeout"; minutes: number | null }
  | { kind: "set_automatic_admission_suppressed"; suppressed: boolean };

export interface FocusRun {
  id: string; eventId: string | null; occurrenceId: string; eventDate: string; title: string | null;
  startedAtMs: number; plannedStartMs: number; plannedEndMs: number; endedAtMs: number | null;
  inheritedFocusMs: number; inheritedPhaseMs: number; configuration: PomodoroConfig;
}
export interface FocusPause { startedAtMs: number; endedAtMs: number | null; reason: PauseReason }
export interface FocusSegment {
  id: string; runId: string; eventId: string | null; eventDate: string; phase: SegmentPhase;
  rhythmPosition: number; plannedStartMs: number; plannedEndMs: number; actualStartMs: number;
  actualEndMs: number | null; chosenDurationMs: number; status: PersistedSegment["status"];
  endReason: string | null; pauses: FocusPause[];
}
export interface FocusSnapshot {
  revision: number; observedAtMs: number; mode: FocusMode; run: FocusRun | null; segment: FocusSegment | null;
  changedSegments: FocusSegment[]; phaseDeadlineMs: number | null; remainingMs: number; elapsedMs: number;
  completedFocusCount: number; skipNextBreak: boolean; focusExtensionUsed: boolean; breakExtensionMs: number;
  dismissedOccurrenceId: string | null; automaticAdmissionSuppressed: boolean; pausedPromptsDismissed: boolean;
  idleStartedAtMs: number | null; idleDetectedAtMs: number | null; idleOverlayVisibleAtMs: number | null; focusFailedAtMs: number | null;
  suspendStartedAtMs: number | null; suspendReturnedAtMs: number | null; returnStartedAtMs: number | null;
  activitySourceUnavailable: boolean; effectiveIdleTimeoutMinutes: number | null;
}
export interface FocusProjection { vaultId: string | null; vaultGeneration: number; snapshot: FocusSnapshot | null; error: string | null }
export interface FocusCommand { commandId: string; expectedRevision: number; intent: FocusIntent }
export interface FocusRequest { vaultId: string; vaultGeneration: number; command: FocusCommand }
export interface FocusCommandResult { projection: FocusProjection; receipt: FocusSnapshot }
export interface FocusNotice { vaultGeneration: number; revision: number | null; available: boolean; hasError: boolean }

type Validator<T> = (value: unknown) => value is T;
type Schema<T> = { [K in keyof T]-?: Validator<T[K]> };
const MAX_TEXT_LENGTH = 64 * 1024;
const MAX_CHANGED_SEGMENTS = 128;
const MAX_SEGMENT_PAUSES = 1024;
const MAX_JS_TIMESTAMP_MS = 8_640_000_000_000_000;
const text: Validator<string> = (value): value is string => typeof value === "string" && value.length <= MAX_TEXT_LENGTH;
const integer: Validator<number> = (value): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const instant: Validator<number> = (value): value is number => integer(value) && value <= MAX_JS_TIMESTAMP_MS;
const boolean: Validator<boolean> = (value): value is boolean => typeof value === "boolean";
const nullable = <T>(validator: Validator<T>): Validator<T | null> => (value): value is T | null => value === null || validator(value);
const choice = <T extends string>(values: readonly T[]): Validator<T> => (value): value is T => typeof value === "string" && values.some((allowed) => allowed === value);
const list = <T>(validator: Validator<T>, limit: number): Validator<T[]> => (value): value is T[] => Array.isArray(value) && value.length <= limit && value.every(validator);
function record<T>(schema: Schema<T>): Validator<T> {
  return (value): value is T => {
    if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
    const object = value as Record<string, unknown>;
    return (Object.keys(schema) as (keyof T)[]).every((key) => schema[key](object[String(key)]));
  };
}
const pause = record<FocusPause>({ startedAtMs: instant, endedAtMs: nullable(instant), reason: choice(["manual", "idle", "suspend"]) });
const segment = record<FocusSegment>({
  id: text, runId: text, eventId: nullable(text), eventDate: text, phase: choice(["focus", "short_break", "long_break"]),
  rhythmPosition: (value): value is number => integer(value) && value > 0,
  plannedStartMs: instant, plannedEndMs: instant, actualStartMs: instant, actualEndMs: nullable(instant),
  chosenDurationMs: integer, status: choice(["active", "completed", "interrupted", "skipped", "planned"]),
  endReason: nullable(text), pauses: list(pause, MAX_SEGMENT_PAUSES),
});
const run = record<FocusRun>({
  id: text, eventId: nullable(text), occurrenceId: text, eventDate: text, title: nullable(text),
  startedAtMs: instant, plannedStartMs: instant, plannedEndMs: instant, endedAtMs: nullable(instant),
  inheritedFocusMs: integer, inheritedPhaseMs: integer, configuration: isValidPomodoroConfig,
});
const snapshotFields = record<FocusSnapshot>({
  revision: integer, observedAtMs: instant, mode: choice(["stopped", "running", "manual_pause", "idle_pause", "idle_failed", "suspended", "return_wait", "expired"]),
  run: nullable(run), segment: nullable(segment), changedSegments: list(segment, MAX_CHANGED_SEGMENTS),
  phaseDeadlineMs: nullable(instant), remainingMs: integer, elapsedMs: integer, completedFocusCount: integer,
  skipNextBreak: boolean, focusExtensionUsed: boolean, breakExtensionMs: integer, dismissedOccurrenceId: nullable(text),
  automaticAdmissionSuppressed: boolean, pausedPromptsDismissed: boolean, idleStartedAtMs: nullable(instant),
  idleDetectedAtMs: nullable(instant), idleOverlayVisibleAtMs: nullable(instant), focusFailedAtMs: nullable(instant), suspendStartedAtMs: nullable(instant),
  suspendReturnedAtMs: nullable(instant), returnStartedAtMs: nullable(instant), activitySourceUnavailable: boolean,
  effectiveIdleTimeoutMinutes: nullable(integer),
});
const snapshot: Validator<FocusSnapshot> = (value): value is FocusSnapshot => {
  if (!snapshotFields(value)) return false;
  if (value.idleOverlayVisibleAtMs !== null && (value.idleDetectedAtMs === null
    || value.idleOverlayVisibleAtMs < value.idleDetectedAtMs
    || value.idleOverlayVisibleAtMs > value.observedAtMs)) return false;
  if (value.segment && value.segment.runId !== value.run?.id) return false;
  if (value.mode !== "stopped" && (!value.run || !value.segment)) return false;
  if (value.mode === "running" && (value.run?.endedAtMs !== null
    || value.segment?.status !== "active" || value.segment.actualEndMs !== null || value.phaseDeadlineMs === null)) return false;
  return true;
};
const projection = record<FocusProjection>({ vaultId: nullable(text), vaultGeneration: integer, snapshot: nullable(snapshot), error: nullable(text) });

/** Validate the complete native projection before it enters reactive UI state. */
export function parseFocusProjection(value: unknown): FocusProjection {
  if (!projection(value) || (value.snapshot !== null && value.vaultId === null)) throw new Error("Invalid native Focus projection");
  if (value.snapshot?.segment && value.snapshot.segment.runId !== value.snapshot.run?.id) throw new Error("Native Focus phase belongs to another run");
  return value;
}

/** Validate immutable action evidence separately from the current projection. */
export function parseFocusCommandResult(value: unknown, expectedRevision: number): FocusCommandResult {
  const valid = record<FocusCommandResult>({ projection, receipt: snapshot });
  if (!valid(value) || value.receipt.revision !== expectedRevision + 1) throw new Error("Invalid native Focus action receipt");
  parseFocusProjection(value.projection);
  return value;
}

/** Compact channel notices contain no writable execution fields. */
export function parseFocusNotice(value: unknown): FocusNotice {
  const valid = record<FocusNotice>({ vaultGeneration: integer, revision: nullable(integer), available: boolean, hasError: boolean });
  if (!valid(value)) throw new Error("Invalid native Focus notice");
  return value;
}

/** Older responses cannot restore previous vaults, revisions, or countdown bases. */
export function focusProjectionIsCurrent(previous: FocusProjection | null, next: FocusProjection): boolean {
  if (!previous || next.vaultGeneration > previous.vaultGeneration) return true;
  if (next.vaultGeneration < previous.vaultGeneration) return false;
  if (next.vaultId !== previous.vaultId) return previous.vaultId === null && previous.snapshot === null;
  if (!next.snapshot || !previous.snapshot) return true;
  return next.snapshot.revision > previous.snapshot.revision
    || (next.snapshot.revision === previous.snapshot.revision && next.snapshot.observedAtMs >= previous.snapshot.observedAtMs);
}

/** Interpolate the remaining time for display without passing the accepted event end. */
export function focusDisplayRemainingSeconds(snapshot: FocusSnapshot, visualElapsedMs: number): number {
  const elapsed = Math.max(0, visualElapsedMs);
  const phaseRemaining = snapshot.remainingMs - (snapshot.mode === "running" ? elapsed : 0);
  const eventRemaining = snapshot.run ? snapshot.run.plannedEndMs - snapshot.observedAtMs - elapsed : phaseRemaining;
  return Math.ceil(Math.max(0, Math.min(phaseRemaining, eventRemaining)) / 1000);
}

/**
 * Report whether presentation counters advance between native projections.
 *
 * Open runs clip their countdown by the event end, and idle or break-return
 * prompts count elapsed time. Every other state renders identically until the
 * next native projection, so no visual clock is needed.
 *
 * @param snapshot The latest accepted native snapshot, or null before the first projection.
 * @returns True when a periodic visual clock changes visible output.
 */
export function focusNeedsVisualClock(snapshot: FocusSnapshot | null | undefined): boolean {
  if (!snapshot) return false;
  return (snapshot.run !== null && snapshot.run.endedAtMs === null)
    || snapshot.idleStartedAtMs !== null
    || snapshot.returnStartedAtMs !== null;
}

/** A delayed native reply cannot make the visual counter credit extra phase work. */
export function focusDisplayElapsedSeconds(snapshot: FocusSnapshot, visualElapsedMs: number): number {
  return Math.floor(Math.min(snapshot.segment?.chosenDurationMs ?? 0,
    snapshot.elapsedMs + (snapshot.mode === "running" ? Math.max(0, visualElapsedMs) : 0)) / 1000);
}

/** Map accepted native evidence into the existing Calendar rail presentation DTO. */
export function focusSegmentForRail(segment: FocusSegment, occurrenceId: string): PersistedSegment {
  return {
    id: segment.id, runId: segment.runId, eventId: segment.eventId ?? occurrenceId, eventDate: segment.eventDate,
    rhythmPosition: segment.rhythmPosition, phase: segment.phase,
    plannedStart: new Date(segment.plannedStartMs).toISOString(), plannedEnd: new Date(segment.plannedEndMs).toISOString(),
    actualStart: new Date(segment.actualStartMs).toISOString(), actualEnd: segment.actualEndMs === null ? null : new Date(segment.actualEndMs).toISOString(),
    status: segment.status, pauseLog: segment.pauses.map((pause) => ({
      startedAt: new Date(pause.startedAtMs).toISOString(), endedAt: pause.endedAtMs === null ? null : new Date(pause.endedAtMs).toISOString(), reason: pause.reason,
    })),
  };
}
