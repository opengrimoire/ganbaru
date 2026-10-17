import type {
  PomodoroConfig,
  PlannedSegment,
  PauseInterval,
  PersistedSegment,
  TimelineBand,
  SegmentPhase,
  SegmentStatus,
} from "$lib/calendar/types";
import {
  breakAfterFocusPosition,
  configEquals,
  deriveRhythmPlan,
  focusDurationMinutesAtPosition,
  nextRhythmPosition,
  phaseDurationMinutesAtPosition,
  type RhythmState,
} from "./rhythm";
import { selectFocusOwner } from "./ownership";

/** Visual overtime allowance never extends native execution or the Calendar deadline. */
const BREAK_OVERTIME_RAIL_GRACE_SECONDS = 10;

/**
 * Compute the planned sequence of focus and break segments for a pomodoro
 * session within a given event duration.
 *
 * @param config - Pomodoro timing settings.
 * @param eventDurationMinutes - Total duration of the calendar event in minutes.
 * @param initialFocusOffsetMinutes - Focus time already accumulated from a preceding event (inheritance).
 *     If >= focusDuration, the event starts with a break instead of focus.
 * @param initialRhythmPosition - Position to start at from a preceding event's trailing state.
 *     Default 1. Count rhythms use the long-break interval as their position count.
 * @returns Ordered list of segments with minute offsets from the event start.
 */
export function computePlannedSegments(
  config: PomodoroConfig,
  eventDurationMinutes: number,
  initialFocusOffsetMinutes: number = 0,
  initialRhythmPosition: number = 1,
): PlannedSegment[] {
  return deriveRhythmPlan(
    config,
    eventDurationMinutes,
    initialFocusOffsetMinutes,
    initialRhythmPosition,
  ).segments;
}

export function computeTrailingRhythmState(
  config: PomodoroConfig,
  eventDurationMinutes: number,
  initialFocusOffsetMinutes: number = 0,
  initialRhythmPosition: number = 1,
): RhythmState {
  return deriveRhythmPlan(
    config,
    eventDurationMinutes,
    initialFocusOffsetMinutes,
    initialRhythmPosition,
  ).trailingState;
}

/**
 * Compute the accumulated focus time at the end of a planned segment sequence.
 * Returns the minutes of focus in the last segment if it is a focus segment.
 * Returns 0 when the last segment is a break because focus resets after a break.
 */
export function computeTrailingFocusMinutes(segments: PlannedSegment[]): number {
  if (segments.length === 0) return 0;
  const last = segments[segments.length - 1];
  if (last.phase === "focus") {
    return last.endOffsetMinutes - last.startOffsetMinutes;
  }
  return 0;
}

/** A Pomodoro-enabled Calendar event placed on one day's timeline. */
export interface TimelineEvent {
  id: string;
  createdAt?: string;
  config: PomodoroConfig;
  /** Full event start timestamp in epoch milliseconds. */
  startMs: number;
  /** Full event end timestamp in epoch milliseconds. */
  endMs: number;
  /** Start minute of day, clipped to the rendered day. */
  startMinute: number;
  /** End minute of day, clipped to the rendered day. */
  endMinute: number;
}

export interface ActivePomodoroState {
  activeOccurrenceId: string | null;
  segments: PersistedSegment[];
  remainingSeconds: number;
  phaseElapsedSeconds?: number;
  phaseWorkDurationSeconds?: number;
  currentConfig?: PomodoroConfig;
  breakOvertimeSeconds: number;
}

/**
 * Project recorded history and a non-overlapping future rhythm under the shared
 * Calendar owner rule. Planned bands remain proposals and never create history.
 */
export function computeDayTimelineBands(
  events: TimelineEvent[],
  activeState: ActivePomodoroState | null,
  dayStartMs: number,
  nowMs: number,
  persistedSegments?: ReadonlyMap<string, PersistedSegment[]>,
): TimelineBand[] {
  if (!Number.isFinite(dayStartMs) || !Number.isFinite(nowMs)) return [];
  const eligible = events.filter((event) => Number.isFinite(event.startMs)
    && Number.isFinite(event.endMs) && event.startMs < event.endMs
    && Number.isFinite(event.startMinute) && Number.isFinite(event.endMinute)
    && event.startMinute < event.endMinute);
  if (eligible.length === 0) return [];
  const bands: TimelineBand[] = [];
  const activeEvent = eligible.find((event) => event.id === activeState?.activeOccurrenceId);

  // Retain recorded evidence even when another event currently owns the same window.
  for (const event of eligible) {
    const history = persistedSegments?.get(event.id) ?? [];
    if (event === activeEvent && activeState && activeState.segments.length > 0) {
      const activeRunIds = new Set(activeState.segments.map((segment) => segment.runId));
      bands.push(...projectPersistedSegments(
        history.filter((segment) => !activeRunIds.has(segment.runId)), event, dayStartMs,
      ));
      bands.push(...projectActiveSegments(
        activeState.segments, activeState.remainingSeconds, activeState.phaseElapsedSeconds,
        activeState.phaseWorkDurationSeconds, activeState.currentConfig,
        event, dayStartMs, nowMs,
      ));
    } else {
      bands.push(...projectPersistedSegments(history, event, dayStartMs));
    }
  }

  const candidates = eligible.map((event) => ({
    ...event,
    startMs: Math.max(event.startMs, dayStartMs + event.startMinute * 60_000),
    endMs: Math.min(event.endMs, dayStartMs + event.endMinute * 60_000),
    event,
  }));
  let cursorMs = Math.max(nowMs, Math.min(...candidates.map((event) => event.startMs)));
  let previousOwnerId = activeState?.activeOccurrenceId ?? null;
  let inheritedFocusMinutes = 0;
  let inheritedRhythmPosition = 1;
  let firstWindow = true;

  while (true) {
    const owner = selectFocusOwner(candidates, cursorMs, previousOwnerId);
    if (!owner) {
      const nextStartMs = Math.min(...candidates
        .filter((candidate) => candidate.startMs > cursorMs)
        .map((candidate) => candidate.startMs));
      if (!Number.isFinite(nextStartMs)) break;
      cursorMs = nextStartMs;
      previousOwnerId = null;
      inheritedFocusMinutes = 0;
      inheritedRhythmPosition = 1;
      continue;
    }
    const event = owner.event;
    // An already recorded rhythm retains its phase alignment. A commitment with
    // no execution starts its proposal at now and carries no assumed past work.
    if (firstWindow && persistedSegments?.get(event.id)?.length && cursorMs > owner.startMs) {
      const previous = computeTrailingRhythmState(event.config, (cursorMs - owner.startMs) / 60_000);
      inheritedFocusMinutes = previous.focusOffsetMinutes;
      inheritedRhythmPosition = previous.rhythmPosition;
    }
    const durationMinutes = (owner.endMs - cursorMs) / 60_000;
    if (event !== activeEvent || !activeState?.segments.length) {
      const planned = computePlannedSegments(
        event.config, durationMinutes, inheritedFocusMinutes, inheritedRhythmPosition,
      );
      for (const segment of planned) {
        if (segment.phase === "focus") continue;
        bands.push({
          topMinute: (cursorMs - dayStartMs) / 60_000 + segment.startOffsetMinutes,
          heightMinutes: segment.endOffsetMinutes - segment.startOffsetMinutes,
          phase: segment.phase,
          status: "planned",
        });
      }
    }
    const trailing = computeTrailingRhythmState(
      event.config, durationMinutes, inheritedFocusMinutes, inheritedRhythmPosition,
    );
    inheritedFocusMinutes = trailing.focusOffsetMinutes;
    inheritedRhythmPosition = trailing.rhythmPosition;
    previousOwnerId = owner.id;
    cursorMs = owner.endMs;
    firstWindow = false;
  }
  return bands.sort((left, right) => left.topMinute - right.topMinute);
}

function samePomodoroConfig(a: PomodoroConfig | undefined, b: PomodoroConfig | undefined): boolean {
  return !!a && !!b && configEquals(a, b);
}

function phaseDurationMinutes(
  phase: SegmentPhase,
  config: PomodoroConfig,
  rhythmPosition: number,
): number {
  return phaseDurationMinutesAtPosition(phase, config, rhythmPosition);
}

function cappedBreakBandEndMs(segment: PersistedSegment, endMs: number): number {
  if (segment.phase === "focus") return endMs;
  const plannedEndMs = new Date(segment.plannedEnd).getTime();
  return Math.min(endMs, plannedEndMs + BREAK_OVERTIME_RAIL_GRACE_SECONDS * 1000);
}

function projectedActiveSegmentEndMs(
  activeSegment: PersistedSegment,
  remainingSeconds: number,
  ev: TimelineEvent,
  nowMs: number,
  activeConfig?: PomodoroConfig,
  phaseElapsedSeconds?: number,
  phaseWorkDurationSeconds?: number,
): number {
  const storedEndMs = nowMs + remainingSeconds * 1000;
  const sameConfig = samePomodoroConfig(activeConfig, ev.config);
  if (sameConfig && phaseWorkDurationSeconds === undefined) return storedEndMs;

  const configuredDurationSeconds = phaseDurationMinutes(
    activeSegment.phase,
    ev.config,
    activeSegment.rhythmPosition,
  ) * 60;
  const targetDurationSeconds = sameConfig
    ? Math.max(0, phaseWorkDurationSeconds ?? configuredDurationSeconds)
    : configuredDurationSeconds;
  const elapsedSeconds = phaseElapsedSeconds ?? Math.max(
    0,
    phaseDurationMinutes(
      activeSegment.phase,
      activeConfig ?? ev.config,
      activeSegment.rhythmPosition,
    ) * 60 - remainingSeconds,
  );
  const projectedRemainingSeconds = Math.max(0, targetDurationSeconds - elapsedSeconds);
  return Math.min(ev.endMs, nowMs + projectedRemainingSeconds * 1000);
}

/**
 * Split a time range [startMs, endMs] into sub-ranges excluding pause intervals.
 * Each returned range is a filled period where focus was actually running.
 */
function splitAroundPauses(
  startMs: number,
  endMs: number,
  pauseLog: PauseInterval[],
): Array<{ start: number; end: number }> {
  if (pauseLog.length === 0) return [{ start: startMs, end: endMs }];

  const ranges: Array<{ start: number; end: number }> = [];
  let cursor = startMs;

  for (const pause of pauseLog) {
    const pStartMs = new Date(pause.startedAt).getTime();
    const pEndMs = pause.endedAt ? new Date(pause.endedAt).getTime() : endMs;

    if (pStartMs > cursor) {
      ranges.push({ start: cursor, end: Math.min(pStartMs, endMs) });
    }
    cursor = Math.min(pEndMs, endMs);
    if (cursor >= endMs) break;
  }

  if (cursor < endMs) {
    ranges.push({ start: cursor, end: endMs });
  }

  return ranges;
}

/**
 * Emit focus fill bands for a segment, splitting around pause gaps.
 */
function emitFocusFillBands(
  startMs: number,
  endMs: number,
  pauseLog: PauseInterval[],
  dayStartMs: number,
  ev: TimelineEvent,
  status: SegmentStatus,
  bands: TimelineBand[],
): void {
  const ranges = splitAroundPauses(startMs, endMs, pauseLog);
  for (const r of ranges) {
    const rawTopMinute = (r.start - dayStartMs) / 60000;
    const rawEndMinute = (r.end - dayStartMs) / 60000;
    const topMinute = Math.max(rawTopMinute, ev.startMinute);
    const endMinute = Math.min(rawEndMinute, ev.endMinute);
    const heightMinutes = endMinute - topMinute;
    if (heightMinutes > 0) {
      bands.push({ topMinute, heightMinutes, phase: "focus", status });
    }
  }
}

/**
 * Project the active run's segments onto minute-of-day coordinates.
 * Outputs focus fill bands, break bands, and projected future breaks.
 */
function projectActiveSegments(
  segments: PersistedSegment[],
  remainingSeconds: number,
  phaseElapsedSeconds: number | undefined,
  phaseWorkDurationSeconds: number | undefined,
  activeConfig: PomodoroConfig | undefined,
  ev: TimelineEvent,
  dayStartMs: number,
  nowMs: number,
): TimelineBand[] {
  const bands: TimelineBand[] = [];
  const activeIdx = segments.findIndex((s) => s.status === "active");
  const activeSegment = activeIdx >= 0 ? segments[activeIdx] : null;
  const currentEndMs = activeSegment
    ? projectedActiveSegmentEndMs(
        activeSegment,
        remainingSeconds,
        ev,
        nowMs,
        activeConfig,
        phaseElapsedSeconds,
        phaseWorkDurationSeconds,
      )
    : nowMs + remainingSeconds * 1000;

  for (let i = 0; i < segments.length; i++) {
    const seg = segments[i];
    const plannedDurMs = new Date(seg.plannedEnd).getTime() - new Date(seg.plannedStart).getTime();

    if (seg.status === "completed" || seg.status === "skipped" || seg.status === "interrupted") {
      const startMs = new Date(seg.actualStart ?? seg.plannedStart).getTime();
      const rawEndMs = new Date(seg.actualEnd ?? seg.plannedEnd).getTime();
      const endMs = cappedBreakBandEndMs(seg, rawEndMs);
      if (seg.phase === "focus") {
        emitFocusFillBands(startMs, endMs, seg.pauseLog, dayStartMs, ev, seg.status, bands);
      } else {
        const topMinute = (startMs - dayStartMs) / 60000;
        const heightMinutes = (endMs - startMs) / 60000;
        if (topMinute + heightMinutes > ev.startMinute && topMinute < ev.endMinute) {
          bands.push({ topMinute, heightMinutes, phase: seg.phase, status: seg.status });
        }
      }
    } else if (i === activeIdx) {
      if (seg.phase === "focus") {
        // Active focus: fill from actual_start to now (capped at segment end),
        // split around pause gaps.
        const startMs = new Date(seg.actualStart!).getTime();
        const segEndMs = startMs + plannedDurMs;
        const fillEndMs = Math.min(nowMs, segEndMs);
        if (fillEndMs > startMs) {
          emitFocusFillBands(startMs, fillEndMs, seg.pauseLog, dayStartMs, ev, "active", bands);
        }
      } else {
        const startMs = new Date(seg.actualStart!).getTime();
        const endMs = cappedBreakBandEndMs(seg, currentEndMs);
        const topMinute = (startMs - dayStartMs) / 60000;
        const heightMinutes = (endMs - startMs) / 60000;
        if (heightMinutes > 0 && topMinute + heightMinutes > ev.startMinute && topMinute < ev.endMinute) {
          bands.push({ topMinute, heightMinutes, phase: seg.phase, status: "active" });
        }
      }
    }
  }

  if (activeSegment) {
    emitProjectedFutureBreakBands(activeSegment, currentEndMs, ev, dayStartMs, bands);
  }

  return bands;
}

function emitProjectedFutureBreakBands(
  activeSegment: PersistedSegment,
  currentEndMs: number,
  ev: TimelineEvent,
  dayStartMs: number,
  bands: TimelineBand[],
): void {
  let cursor = currentEndMs;
  let rhythmPosition = activeSegment.rhythmPosition;
  let nextIsFocus = activeSegment.phase !== "focus";

  if (activeSegment.phase === "short_break" || activeSegment.phase === "long_break") {
    rhythmPosition = nextRhythmPosition(ev.config, rhythmPosition);
  }

  while (cursor < ev.endMs) {
    if (nextIsFocus) {
      cursor += focusDurationMinutesAtPosition(ev.config, rhythmPosition) * 60_000;
      nextIsFocus = false;
      continue;
    }

    const breakInfo = breakAfterFocusPosition(ev.config, rhythmPosition);
    const breakPhase: SegmentPhase = breakInfo.phase;
    const breakDurationMs = breakInfo.durationMinutes * 60_000;
    const breakEnd = Math.min(cursor + breakDurationMs, ev.endMs);
    const topMinute = (cursor - dayStartMs) / 60000;
    const heightMinutes = (breakEnd - cursor) / 60000;
    if (heightMinutes > 0 && topMinute + heightMinutes > ev.startMinute && topMinute < ev.endMinute) {
      bands.push({ topMinute, heightMinutes, phase: breakPhase, status: "planned" });
    }

    cursor += breakDurationMs;
    rhythmPosition = nextRhythmPosition(ev.config, rhythmPosition);
    nextIsFocus = true;
  }
}

/**
 * Project recorded segments outside the active run onto minute-of-day
 * coordinates. Planned segments and segments without an actual start produce
 * no bands.
 */
function projectPersistedSegments(
  segments: PersistedSegment[],
  ev: TimelineEvent,
  dayStartMs: number,
): TimelineBand[] {
  const bands: TimelineBand[] = [];
  for (const seg of segments) {
    if (seg.status === "planned") continue;
    if (!seg.actualStart) continue;
    const startMs = new Date(seg.actualStart).getTime();
    const rawEndMs = seg.actualEnd ? new Date(seg.actualEnd).getTime() : startMs;
    const endMs = cappedBreakBandEndMs(seg, rawEndMs);
    if (endMs <= startMs) continue;
    if (seg.phase === "focus") {
      emitFocusFillBands(startMs, endMs, seg.pauseLog, dayStartMs, ev, seg.status, bands);
    } else {
      const topMinute = (startMs - dayStartMs) / 60000;
      const heightMinutes = (endMs - startMs) / 60000;
      if (topMinute + heightMinutes > ev.startMinute && topMinute < ev.endMinute) {
        bands.push({ topMinute, heightMinutes, phase: seg.phase, status: seg.status });
      }
    }
  }
  return bands;
}
