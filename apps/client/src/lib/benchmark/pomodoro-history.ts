import type { CalendarEvent, PauseInterval, PomodoroConfig } from "$lib/calendar/types";
import { wallClockToUtcIso } from "$lib/calendar/utils";
import { createPresetPomodoroConfig } from "$lib/pomodoro/rhythm";
import { computePlannedSegments } from "$lib/pomodoro/segments";

export const DENSE_TIMED_POMODORO_CONFIG: PomodoroConfig = createPresetPomodoroConfig("adaptive");

export interface BenchmarkPomodoroConfigSeed {
  eventId: string;
  config: PomodoroConfig;
}

export interface BenchmarkPomodoroSegmentSeed {
  id: string;
  eventId: string;
  eventDate: string;
  runId: string;
  rhythmPosition: number;
  phase: "focus" | "short_break" | "long_break";
  plannedStart: string;
  plannedEnd: string;
  actualStart: string | null;
  actualEnd: string | null;
  pauses: PauseInterval[];
  status: "completed";
}

export interface BenchmarkPomodoroHistoryPayload {
  configs: BenchmarkPomodoroConfigSeed[];
  segments: BenchmarkPomodoroSegmentSeed[];
}

interface WallParts {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
}

function pad2(value: number): string {
  return value < 10 ? `0${value}` : String(value);
}

function parseWallClock(value: string): WallParts {
  const [date, time] = value.split(" ");
  const [year, month, day] = date.split("-").map(Number);
  const [hour, minute] = time.split(":").map(Number);
  return { year, month, day, hour, minute };
}

function wallClockMinuteIndex(value: string): number {
  const parts = parseWallClock(value);
  return Date.UTC(parts.year, parts.month - 1, parts.day, parts.hour, parts.minute) / 60_000;
}

function wallClockAddMinutes(value: string, minutes: number): string {
  const parts = parseWallClock(value);
  const date = new Date(Date.UTC(
    parts.year,
    parts.month - 1,
    parts.day,
    parts.hour,
    parts.minute + minutes,
  ));
  return [
    `${date.getUTCFullYear()}-${pad2(date.getUTCMonth() + 1)}-${pad2(date.getUTCDate())}`,
    `${pad2(date.getUTCHours())}:${pad2(date.getUTCMinutes())}`,
  ].join(" ");
}

function wallClockDurationMinutes(start: string, end: string): number {
  return wallClockMinuteIndex(end) - wallClockMinuteIndex(start);
}

function isPastTimedPomodoroEvent(event: CalendarEvent, pastCutoffWallClock: string): boolean {
  return !event.allDay
    && event.pomodoroConfig !== undefined
    && event.end <= pastCutoffWallClock;
}

function toIso(event: CalendarEvent, wallClock: string): string {
  return wallClockToUtcIso(wallClock, event.timezone);
}

export function buildDensePomodoroHistoryPayload(
  events: CalendarEvent[],
  pastCutoffWallClock: string,
): BenchmarkPomodoroHistoryPayload {
  const payload: BenchmarkPomodoroHistoryPayload = {
    configs: [],
    segments: [],
  };

  for (const event of events) {
    const config = event.pomodoroConfig;
    if (!config || event.allDay) continue;

    payload.configs.push({
      eventId: event.id,
      config,
    });

    if (!isPastTimedPomodoroEvent(event, pastCutoffWallClock)) continue;

    const durationMinutes = wallClockDurationMinutes(event.start, event.end);
    const planned = computePlannedSegments(config, durationMinutes);
    const runId = `${event.id}-run`;
    const eventDate = event.start.split(" ")[0];

    planned.forEach((segment, index) => {
      const plannedStartWall = wallClockAddMinutes(event.start, segment.startOffsetMinutes);
      const plannedEndWall = wallClockAddMinutes(event.start, segment.endOffsetMinutes);
      const plannedStart = toIso(event, plannedStartWall);
      const plannedEnd = toIso(event, plannedEndWall);
      const id = `${event.id}-segment-${index + 1}`;

      payload.segments.push({
        id,
        eventId: event.id,
        eventDate,
        runId,
        rhythmPosition: segment.rhythmPosition,
        phase: segment.phase,
        plannedStart,
        plannedEnd,
        actualStart: plannedStart,
        actualEnd: plannedEnd,
        pauses: [],
        status: "completed",
      });
    });
  }

  return payload;
}
