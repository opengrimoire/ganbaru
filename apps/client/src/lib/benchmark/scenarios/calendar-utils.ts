import { invoke } from "@tauri-apps/api/core";
import { dbUrl } from "$lib/api/db";
import { getCalendar } from "$lib/stores/calendar.svelte";
import { getCalendars } from "$lib/stores/calendars.svelte";
import type { CalendarEvent } from "$lib/calendar/types";
import type { CalendarViewMode } from "$lib/calendar/types";
import { computeViewWindow } from "$lib/calendar/utils";
import {
  buildBulkImportPayload,
  type CalendarBulkImportResult,
} from "$lib/stores/calendar/bulk-import";
import { countDenseCalendarEvents, generateDenseCalendarEvents } from "../dense";
import {
  buildDensePomodoroHistoryPayload,
  type BenchmarkPomodoroHistoryPayload,
} from "../pomodoro-history";
import {
  benchmarkDatasetId,
  type BenchmarkDatasetProfile,
  type BenchmarkEventDraft,
  type BenchmarkMetric,
  type BenchmarkSeedHandle,
  type BenchmarkScenarioContext,
} from "../types";

const DENSE_SEED_CHUNK_SIZE = 1_000;

/** Calendar grouping name template. Renaming requires bumping `DENSE_DATASET_VERSION`. */
export function denseCalendarFilename(dataset: BenchmarkDatasetProfile): string {
  return `benchmark-dense-${dataset.version}-s${dataset.stackCount}-${dataset.detailProfile}.ics`;
}

export function parseCalendarBenchmarkAnchor(anchorDate: string): Date {
  const [y, m, d] = anchorDate.split("-").map(Number);
  if (
    !Number.isInteger(y)
    || !Number.isInteger(m)
    || !Number.isInteger(d)
    || y < 1
    || m < 1
    || m > 12
    || d < 1
    || d > 31
  ) {
    throw new Error(`Invalid benchmark anchor date: ${anchorDate}`);
  }
  return new Date(y, m - 1, d);
}

export function calendarBenchmarkAnchorWallClock(anchorDate: string): string {
  return `${anchorDate} 00:00`;
}

export async function loadCalendarBenchmarkWindow(
  anchorDate: string,
  mode: CalendarViewMode = "week",
): Promise<void> {
  const window = computeViewWindow(parseCalendarBenchmarkAnchor(anchorDate), mode);
  await getCalendars().load();
  const calendarStore = getCalendar();
  await calendarStore.loadWindow(window.start, window.end);
  await calendarStore.whenWindowIdle();
}

export function localTimezone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone;
}

function nowIso(): string {
  return new Date().toISOString();
}

/**
 * Convert a deterministic benchmark draft into a full event ready for
 * `calendar_bulk_import`. The source UID doubles as the event id, so
 * re-seeds land as updates instead of duplicates.
 */
export function draftToEvent(
  draft: BenchmarkEventDraft,
  _index: number,
  calendarId: string,
): CalendarEvent {
  const id = draft.sourceUid;
  const event: CalendarEvent = {
    id,
    title: draft.title,
    start: draft.start,
    end: draft.end,
    timezone: localTimezone(),
    calendarId,
    sourceUid: draft.sourceUid,
    sequence: 1,
  };
  if (draft.allDay) event.allDay = true;
  if (draft.description) event.description = draft.description;
  if (draft.recurrence) event.recurrence = draft.recurrence;
  if (draft.notifications) event.notifications = draft.notifications;
  if (draft.pomodoroConfig) event.pomodoroConfig = draft.pomodoroConfig;
  if (draft.color !== undefined) event.color = draft.color;
  if (draft.location) event.location = draft.location;
  if (draft.url) event.url = draft.url;
  if (draft.transparency) event.transparency = draft.transparency;
  if (draft.status) event.status = draft.status;
  if (draft.visibility) event.visibility = draft.visibility;
  if (draft.priority !== undefined) event.priority = draft.priority;
  if (draft.categories) event.categories = draft.categories;
  if (draft.geo) event.geo = draft.geo;
  if (draft.extendedProperties) event.extendedProperties = draft.extendedProperties;
  if (draft.organizer) event.organizer = draft.organizer;
  if (draft.alarms) event.alarms = draft.alarms;
  if (draft.attendees) event.attendees = draft.attendees;
  if (draft.guestPermissions) event.guestPermissions = draft.guestPermissions;
  return event;
}

async function bulkImportBenchmarkEvents(
  events: CalendarEvent[],
  targetCalendarId: string,
): Promise<CalendarBulkImportResult> {
  const eventIds = events.map((event) => event.id);
  let index = 0;
  const payload = buildBulkImportPayload(
    events,
    targetCalendarId,
    nowIso(),
    localTimezone(),
    () => eventIds[index++] ?? crypto.randomUUID(),
  );
  const result = await invoke<CalendarBulkImportResult>("calendar_bulk_import", {
    dbUrl: dbUrl(),
    payload,
  });
  if (result.applied.length !== events.length) {
    throw new Error(
      `Dense calendar seed imported ${result.applied.length} of ${events.length} events`,
    );
  }
  return result;
}

async function seedPomodoroHistory(payload: BenchmarkPomodoroHistoryPayload): Promise<void> {
  if (
    payload.configs.length === 0
    && payload.segments.length === 0
  ) {
    return;
  }
  await invoke("benchmark_seed_pomodoro_history", {
    dbUrl: dbUrl(),
    payload,
  });
}

export async function seedCalendarDataset(
  dataset: BenchmarkDatasetProfile,
  context: BenchmarkScenarioContext,
): Promise<BenchmarkSeedHandle> {
  const calendarsStore = getCalendars();
  const calendar = await calendarsStore.findOrCreateImported(denseCalendarFilename(dataset));
  const anchor = parseCalendarBenchmarkAnchor(context.anchorDate);
  const totalEvents = countDenseCalendarEvents(dataset, anchor);

  for (let offset = 0; offset < totalEvents; offset += DENSE_SEED_CHUNK_SIZE) {
    const drafts = generateDenseCalendarEvents({
      dataset,
      anchor,
      offset,
      count: DENSE_SEED_CHUNK_SIZE,
    });
    const events = drafts.map((d, i) => draftToEvent(d, offset + i, calendar.id));
    await bulkImportBenchmarkEvents(events, calendar.id);
    await seedPomodoroHistory(
      buildDensePomodoroHistoryPayload(
        events,
        calendarBenchmarkAnchorWallClock(context.anchorDate),
      ),
    );
  }

  return {
    calendarId: calendar.id,
    eventCount: totalEvents,
    datasetId: benchmarkDatasetId(dataset),
    dataset,
  };
}

export async function waitForFrames(count: number): Promise<void> {
  for (let i = 0; i < count; i++) {
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => resolve());
    });
  }
}

export function waitForMs(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(new DOMException("aborted", "AbortError"));
      return;
    }
    const timer = window.setTimeout(resolve, ms);
    signal.addEventListener("abort", () => {
      window.clearTimeout(timer);
      reject(new DOMException("aborted", "AbortError"));
    }, { once: true });
  });
}

interface TimingStats {
  meanMs: number;
  minMs: number;
  medianMs: number;
  p95Ms: number;
  maxMs: number;
}

function timingStats(values: number[]): TimingStats | undefined {
  const valid = values.filter(Number.isFinite).sort((a, b) => a - b);
  if (valid.length === 0) return undefined;
  const meanMs = valid.reduce((sum, value) => sum + value, 0) / valid.length;
  return {
    meanMs,
    minMs: valid[0],
    medianMs: percentile(valid, 0.5),
    p95Ms: percentile(valid, 0.95),
    maxMs: valid[valid.length - 1],
  };
}

function percentile(sortedValues: number[], p: number): number {
  if (sortedValues.length === 1) return sortedValues[0];
  const index = Math.min(sortedValues.length - 1, Math.ceil(sortedValues.length * p) - 1);
  return sortedValues[index];
}

export function timingStatsMetric(
  label: string,
  values: number[],
  details: Record<string, string | number> = {},
): BenchmarkMetric {
  const stats = timingStats(values);
  if (!stats) {
    return {
      label,
      unit: "ms",
      value: Number.NaN,
      details: { missing: "valid timing sample", ...details },
    };
  }
  return {
    label,
    unit: "ms",
    value: stats.meanMs,
    details: {
      runs: values.filter(Number.isFinite).length,
      minMs: stats.minMs,
      medianMs: stats.medianMs,
      p95Ms: stats.p95Ms,
      maxMs: stats.maxMs,
      ...details,
    },
  };
}
