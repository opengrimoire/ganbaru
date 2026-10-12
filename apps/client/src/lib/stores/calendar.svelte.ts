import { invoke } from "@tauri-apps/api/core";
import { Temporal } from "@js-temporal/polyfill";
import { dbUrl } from "$lib/api/db";
import type {
  Calendar, CalendarEvent, CalendarViewMode,
} from "$lib/calendar/types";
import { computeViewWindow } from "$lib/calendar/utils";
import {
  localTimezone,
} from "$lib/stores/calendar/event-payloads";
import {
  loadNativeCalendarWindow,
  nativeCalendarEventsInWindow,
  type CalendarExpansionDiagnostic,
} from "$lib/stores/calendar/native-window";
import { adjacentCalendarWindowRequests, calendarWindowCovers } from "$lib/stores/calendar/window-prefetch";
import {
  BoundedWindowCache,
  LatestWindowLoadCoordinator,
  type WindowLoadEvent,
  type WindowLoadOutcome,
} from "$lib/stores/calendar/window-load-coordinator";
import type { IcsImportSummary } from "$lib/calendar/ics/types";
import { mark as perfMark } from "./perf-log.svelte";
import { getPreferences } from "$lib/stores/preferences.svelte";
import {
  initCalendarWindowSync,
  publishCalendarWindowSync,
} from "$lib/stores/calendar/window-sync";
import {
  bulkImportCalendarEvents,
  exportCalendarAsIcs as exportCalendarIcs,
  type CalendarBulkImportOptions,
} from "$lib/stores/calendar/import-export";
import {
  clearPanelEventCache,
  loadFullEvent,
  loadPanelEvent,
  prefetchPanelEvent,
} from "$lib/stores/calendar/event-loaders";
import { calendarWindowIncludesGlobalCount } from "$lib/stores/calendar/window-count";
import { loadPomodoroSchedulerEventsFromDb } from "$lib/stores/calendar/pomodoro-window";

/** DB-backed template events for the current render window plus recurring templates. */
let sourceEvents = $state<CalendarEvent[]>([]);
let windowEvents = $state<CalendarEvent[]>([]);
let expansionDiagnostics = $state<CalendarExpansionDiagnostic[]>([]);
let loaded = $state(false);
let totalEventCount = $state(0);
let currentWindowKey: string | null = null;
let currentWindowRenderZone: string | null = null;
let currentWindowStart: Temporal.PlainDate | null = null;
let currentWindowEnd: Temporal.PlainDate | null = null;
let currentWindowSourceVersion = 0;
const WINDOW_CACHE_LIMIT = 12;

type CalendarWindowLoadMode = "apply" | "ensure" | "prefetch";

interface CalendarWindowSnapshot {
  key: string;
  renderZone: string;
  windowStart: Temporal.PlainDate;
  windowEnd: Temporal.PlainDate;
  sourceEvents: CalendarEvent[];
  windowEvents: CalendarEvent[];
  diagnostics: CalendarExpansionDiagnostic[];
  totalEventCount: number;
}

interface CalendarWindowLoadRequest {
  key: string;
  renderZone: string;
  windowStart: Temporal.PlainDate;
  windowEnd: Temporal.PlainDate;
  markBoot: boolean;
  force: boolean;
  mode: CalendarWindowLoadMode;
}

const windowCache = new BoundedWindowCache<CalendarWindowSnapshot>(WINDOW_CACHE_LIMIT);
let sourceVersion = 0;
let prefetchGeneration = 0;
let foregroundWindowBusy = false;
let foregroundWindowRequestId = 0;
let foregroundWindowIdleWaiters: Array<() => void> = [];

/**
 * Reactivity token. `eventsInWindow` reads it so any `$derived` / `$effect`
 * that depends on the visible-event set re-runs after a mutation. Bumped
 * by `invalidate()` and the other mutation paths. External callers that
 * need to react to mutations without forcing an expansion subscribe via
 * `void indexVersion`.
 */
let indexVersion = $state(0);

/**
 * Refresh canonical occurrences after committed mutations without expanding local templates.
 */
async function invalidate(): Promise<void> {
  clearPanelEventCache();
  indexVersion++;
  await reloadCurrentWindowFromDb();
}

/**
 * Resolve an event to its DB-backed template.
 * For recurring instances returns the parent; for normal events returns itself.
 */
function resolveToTemplate(event: CalendarEvent): CalendarEvent | undefined {
  const parentId = event.recurringParentId ?? event.id;
  return sourceEvents.find((b) => b.id === parentId);
}

function calendarWindowKey(
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
  renderZone: string,
): string {
  return `${renderZone}:${windowStart.toString()}:${windowEnd.toString()}`;
}

function currentWindowCovers(
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
  renderZone: string,
): boolean {
  return loaded
    && currentWindowSourceVersion === sourceVersion
    && currentWindowStart !== null
    && currentWindowEnd !== null
    && currentWindowRenderZone === renderZone
    && calendarWindowCovers(currentWindowStart, currentWindowEnd, windowStart, windowEnd);
}

function findCachedWindowCovering(
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
  renderZone: string,
): CalendarWindowSnapshot | undefined {
  return windowCache.find((snapshot) =>
    snapshot.renderZone === renderZone
    && calendarWindowCovers(snapshot.windowStart, snapshot.windowEnd, windowStart, windowEnd)
  );
}

function windowQueueKey(mode: CalendarWindowLoadMode, key: string): string {
  return `${sourceVersion}:${mode}:${key}`;
}

function markWindowLoadEvent(event: WindowLoadEvent<CalendarWindowLoadRequest>): void {
  const request = "request" in event ? event.request : undefined;
  if (event.type === "start") {
    perfMark("window.load-start", {
      mode: request?.mode ?? "unknown",
      queued: windowLoadCoordinator.queuedKey ? 1 : 0,
    });
  } else if (event.type === "queue") {
    perfMark("window.load-queued", {
      mode: request?.mode ?? "unknown",
      replaced: event.replacedKey ? 1 : 0,
    });
  } else if (event.type === "drop") {
    perfMark("window.load-dropped", { reason: event.reason });
  } else if (event.type === "finish") {
    perfMark("window.load-finished", { outcome: event.outcome });
  } else {
    perfMark("window.load-error");
  }
}

function applyWindowSnapshot(snapshot: CalendarWindowSnapshot): void {
  sourceEvents = snapshot.sourceEvents;
  windowEvents = snapshot.windowEvents;
  expansionDiagnostics = snapshot.diagnostics;
  totalEventCount = snapshot.totalEventCount;
  currentWindowKey = snapshot.key;
  currentWindowRenderZone = snapshot.renderZone;
  currentWindowStart = snapshot.windowStart;
  currentWindowEnd = snapshot.windowEnd;
  currentWindowSourceVersion = sourceVersion;
  loaded = true;
  clearPanelEventCache();
  indexVersion++;
  perfMark("window.applied", {
    rows: snapshot.sourceEvents.length,
    expanded: snapshot.windowEvents.length,
    cache: windowCache.size,
  });
}

function rememberWindowSnapshot(snapshot: CalendarWindowSnapshot): void {
  windowCache.set(snapshot.key, snapshot);
  perfMark("window.cache-put", {
    rows: snapshot.sourceEvents.length,
    expanded: snapshot.windowEvents.length,
    size: windowCache.size,
  });
}

function beginForegroundWindowLoad(): number {
  foregroundWindowBusy = true;
  return ++foregroundWindowRequestId;
}

function finishForegroundWindowLoad(requestId: number): void {
  if (requestId !== foregroundWindowRequestId) return;
  foregroundWindowBusy = false;
  resolveForegroundWindowIdle();
}

function resolveForegroundWindowIdle(): void {
  if (foregroundWindowBusy) return;
  const waiters = foregroundWindowIdleWaiters;
  foregroundWindowIdleWaiters = [];
  for (const resolve of waiters) resolve();
}

async function waitForForegroundWindowIdle(): Promise<void> {
  if (!foregroundWindowBusy) return;
  await new Promise<void>((resolve) => {
    foregroundWindowIdleWaiters.push(resolve);
  });
}

function adjacentWindowRequests(snapshot: CalendarWindowSnapshot): Array<{
  start: Temporal.PlainDate;
  end: Temporal.PlainDate;
}> {
  return adjacentCalendarWindowRequests(snapshot.windowStart, snapshot.windowEnd);
}


async function runWindowLoadRequest(
  request: CalendarWindowLoadRequest,
  isSuperseded: () => boolean,
): Promise<WindowLoadOutcome> {
  const {
    key,
    renderZone,
    windowStart,
    windowEnd,
    markBoot,
    force,
    mode,
  } = request;
  const requestedSourceVersion = sourceVersion;
  const windowStartDate = windowStart.toString();
  const windowEndDate = windowEnd.toString();
  if (!force && !markBoot && mode !== "apply") {
    const cached = findCachedWindowCovering(windowStart, windowEnd, renderZone);
    if (cached || currentWindowCovers(windowStart, windowEnd, renderZone)) {
      perfMark("window.load-covered", { mode });
      return "applied";
    }
  }
  const mapped = await loadNativeCalendarWindow({
    windowStartDate,
    windowEndDate,
    renderZone,
    includeTotalEventCount: calendarWindowIncludesGlobalCount(markBoot) || force,
  });
  perfMark("window.rows-done", {
    mode,
    rows: mapped.sourceEvents.length,
    total: mapped.totalEventCount ?? totalEventCount,
  });

  if (requestedSourceVersion !== sourceVersion || isSuperseded()) {
    perfMark("window.load-superseded", { stage: "rows", mode });
    return "superseded";
  }

  if (markBoot) perfMark("boot.sql-main-done", { rows: mapped.sourceEvents.length, total: mapped.totalEventCount ?? totalEventCount });
  if (markBoot) perfMark("boot.maprow-done");
  perfMark("window.expand-done", {
    mode,
    rows: mapped.sourceEvents.length,
    expanded: mapped.windowEvents.length,
  });

  const snapshot: CalendarWindowSnapshot = {
    key,
    renderZone,
    windowStart,
    windowEnd,
    sourceEvents: mapped.sourceEvents,
    windowEvents: mapped.windowEvents,
    diagnostics: mapped.diagnostics,
    totalEventCount: mapped.totalEventCount ?? totalEventCount,
  };
  rememberWindowSnapshot(snapshot);

  if (mode === "apply") {
    applyWindowSnapshot(snapshot);
    if (markBoot) {
      perfMark("boot.sql-children-done");
      perfMark("boot.rawblocks-set", { events: sourceEvents.length, total: totalEventCount });
    }
    scheduleAdjacentPrefetch(snapshot);
  }

  return "applied";
}

const windowLoadCoordinator = new LatestWindowLoadCoordinator<CalendarWindowLoadRequest>(
  runWindowLoadRequest,
  markWindowLoadEvent,
);

async function loadWindowIntoState(
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
  markBoot: boolean,
  force = false,
): Promise<void> {
  const renderZone = localTimezone();
  const key = calendarWindowKey(windowStart, windowEnd, renderZone);
  if (!force && !markBoot && loaded && currentWindowSourceVersion === sourceVersion && currentWindowKey === key) return;
  if (!force && !markBoot && currentWindowCovers(windowStart, windowEnd, renderZone)) return;
  if (!force && !markBoot) {
    const cached = windowCache.get(key) ?? findCachedWindowCovering(windowStart, windowEnd, renderZone);
    if (cached) {
      const foregroundId = ++foregroundWindowRequestId;
      foregroundWindowBusy = false;
      resolveForegroundWindowIdle();
      prefetchGeneration++;
      windowLoadCoordinator.supersedePending();
      perfMark("window.cache-hit", {
        rows: cached.sourceEvents.length,
        expanded: cached.windowEvents.length,
        size: windowCache.size,
      });
      applyWindowSnapshot(cached);
      scheduleAdjacentPrefetch(cached);
      finishForegroundWindowLoad(foregroundId);
      return;
    }
  }

  prefetchGeneration++;
  const foregroundId = beginForegroundWindowLoad();
  try {
    await windowLoadCoordinator.enqueue(windowQueueKey("apply", key), {
      key,
      renderZone,
      windowStart,
      windowEnd,
      markBoot,
      force,
      mode: "apply",
    });
  } finally {
    finishForegroundWindowLoad(foregroundId);
  }
}

async function prefetchWindow(
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
  generation: number,
): Promise<void> {
  if (generation !== prefetchGeneration || !loaded) return;
  const renderZone = localTimezone();
  const key = calendarWindowKey(windowStart, windowEnd, renderZone);
  if (currentWindowCovers(windowStart, windowEnd, renderZone)
    || findCachedWindowCovering(windowStart, windowEnd, renderZone)) return;

  await windowLoadCoordinator.enqueue(windowQueueKey("prefetch", key), {
    key,
    renderZone,
    windowStart,
    windowEnd,
    markBoot: false,
    force: false,
    mode: "prefetch",
  });
}

async function ensureWindowSnapshotReady(
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
): Promise<void> {
  const renderZone = localTimezone();
  const key = calendarWindowKey(windowStart, windowEnd, renderZone);
  if (currentWindowCovers(windowStart, windowEnd, renderZone)
    || findCachedWindowCovering(windowStart, windowEnd, renderZone)) return;

  prefetchGeneration++;
  const foregroundId = beginForegroundWindowLoad();
  try {
    await windowLoadCoordinator.enqueue(windowQueueKey("ensure", key), {
      key,
      renderZone,
      windowStart,
      windowEnd,
      markBoot: false,
      force: false,
      mode: "ensure",
    });
  } finally {
    finishForegroundWindowLoad(foregroundId);
  }
}

function scheduleAdjacentPrefetch(snapshot: CalendarWindowSnapshot): void {
  const requests = adjacentWindowRequests(snapshot);
  if (requests.length === 0) return;
  const generation = ++prefetchGeneration;

  void (async () => {
    for (const request of requests) {
      if (generation !== prefetchGeneration) return;
      await prefetchWindow(request.start, request.end, generation);
    }
  })().catch((error: unknown) => {
    console.warn("[calendar] adjacent window prefetch failed", error);
  });
}

function scheduleWindowPrefetches(requests: Array<{
  start: Temporal.PlainDate;
  end: Temporal.PlainDate;
}>): void {
  if (requests.length === 0) return;
  const generation = prefetchGeneration;

  void (async () => {
    for (const request of requests) {
      if (generation !== prefetchGeneration) return;
      await prefetchWindow(request.start, request.end, generation);
    }
  })().catch((error: unknown) => {
    console.warn("[calendar] requested window prefetch failed", error);
  });
}

async function waitForWindowIdle(): Promise<void> {
  await windowLoadCoordinator.whenIdle();
}

function clearWindowCache(): void {
  sourceVersion++;
  windowCache.clear();
  prefetchGeneration++;
  windowLoadCoordinator.supersedePending();
}

async function reloadCurrentWindowFromDb(): Promise<void> {
  if (!currentWindowStart || !currentWindowEnd) {
    clearWindowCache();
    windowEvents = [];
    expansionDiagnostics = [];
    return;
  }
  await reloadWindowFromDb(currentWindowStart, currentWindowEnd);
}

async function reloadWindowFromDb(
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
): Promise<void> {
  clearWindowCache();
  await loadWindowIntoState(windowStart, windowEnd, false, true);
}


export function getCalendar() {
  initCalendarWindowSync(() => reloadCurrentWindowFromDb());
  const store = {
    /**
     * Select native occurrences from a loaded covering window. This bounded
     * projection never evaluates recurrence rules in the frontend.
     */
    eventsInWindow(
      windowStart: Temporal.PlainDate,
      windowEnd: Temporal.PlainDate,
    ): CalendarEvent[] {
      void indexVersion;
      const renderZone = localTimezone();
      const key = calendarWindowKey(windowStart, windowEnd, renderZone);
      if (currentWindowSourceVersion === sourceVersion && currentWindowKey === key) {
        return windowEvents;
      }
      if (currentWindowCovers(windowStart, windowEnd, renderZone)) {
        return nativeCalendarEventsInWindow(windowEvents, windowStart, windowEnd);
      }
      const cached = windowCache.peek(key);
      if (cached) return cached.windowEvents;
      const covering = findCachedWindowCovering(windowStart, windowEnd, renderZone);
      if (covering) {
        return nativeCalendarEventsInWindow(covering.windowEvents, windowStart, windowEnd);
      }
      return [];
    },

    /**
     * Reactivity token; consumers can `void store.indexVersion` inside an
     * `$effect` to re-run on any mutation without paying a wide-window
     * expansion just to subscribe.
     */
    get indexVersion(): number {
      return indexVersion;
    },

    get sourceEvents(): CalendarEvent[] {
      return sourceEvents;
    },

    /** Diagnostics for source events the native projection could not expand; their source rows stay loaded. */
    get expansionDiagnostics(): readonly CalendarExpansionDiagnostic[] {
      return expansionDiagnostics;
    },

    get eventCount(): number {
      return totalEventCount;
    },

    get loaded(): boolean {
      return loaded;
    },

    get windowLoadBusy(): boolean {
      return windowLoadCoordinator.busy;
    },

    get foregroundWindowLoadBusy(): boolean {
      return foregroundWindowBusy;
    },

    /** Applies project assignments returned by a committed task-link mutation. */
    async applyProjectAssignments(assignments: readonly { eventId: string; projectId: string }[]): Promise<void> {
      if (assignments.length === 0) return;
      const projectIdByEventId = new Map(
        assignments.map((assignment) => [assignment.eventId, assignment.projectId]),
      );
      let changed = false;
      sourceEvents = sourceEvents.map((event) => {
        const projectId = projectIdByEventId.get(event.id);
        if (!projectId || event.projectId === projectId) return event;
        changed = true;
        return { ...event, projectId };
      });
      if (changed) await invalidate();
    },

    isWindowCurrent(
      windowStart: Temporal.PlainDate,
      windowEnd: Temporal.PlainDate,
    ): boolean {
      return currentWindowSourceVersion === sourceVersion
        && currentWindowKey === calendarWindowKey(windowStart, windowEnd, localTimezone());
    },

    hasWindow(
      windowStart: Temporal.PlainDate,
      windowEnd: Temporal.PlainDate,
    ): boolean {
      const renderZone = localTimezone();
      return currentWindowCovers(windowStart, windowEnd, renderZone)
        || findCachedWindowCovering(windowStart, windowEnd, renderZone) !== undefined;
    },

    async ensureWindowReady(
      windowStart: Temporal.PlainDate,
      windowEnd: Temporal.PlainDate,
    ): Promise<void> {
      await ensureWindowSnapshotReady(windowStart, windowEnd);
    },

    prefetchWindows(requests: Array<{
      start: Temporal.PlainDate;
      end: Temporal.PlainDate;
    }>): void {
      scheduleWindowPrefetches(requests);
    },

    async whenWindowIdle(): Promise<void> {
      await waitForWindowIdle();
    },

    async whenForegroundWindowIdle(): Promise<void> {
      await waitForForegroundWindowIdle();
    },

    async load(initialViewMode: CalendarViewMode = getPreferences().calendarViewMode) {
      perfMark("boot.sql-start");
      try {
        loaded = false;
        const initialWindow = computeViewWindow(new Date(), initialViewMode);
        await loadWindowIntoState(initialWindow.start, initialWindow.end, true);
      } catch (e) {
        console.error("[calendar] load() failed:", e);
        throw e;
      }
    },

    async loadWindow(
      windowStart: Temporal.PlainDate,
      windowEnd: Temporal.PlainDate,
    ): Promise<void> {
      await loadWindowIntoState(windowStart, windowEnd, false);
    },

    async refreshCurrentWindow(): Promise<void> {
      await reloadCurrentWindowFromDb();
    },

    /** Invalidate derived reads and notify other windows after a native Save receipt. */
    acceptNativeEdit(): void {
      clearPanelEventCache();
      clearWindowCache();
      indexVersion++;
      publishCalendarWindowSync();
    },

    async refreshWindow(
      windowStart: Temporal.PlainDate,
      windowEnd: Temporal.PlainDate,
    ): Promise<void> {
      await reloadWindowFromDb(windowStart, windowEnd);
    },

    async loadPomodoroSchedulerEvents(
      windowStart: Temporal.PlainDate,
      windowEnd: Temporal.PlainDate,
    ): Promise<CalendarEvent[]> {
      return loadPomodoroSchedulerEventsFromDb(windowStart, windowEnd, indexVersion);
    },

    /**
     * Fetch just the fields the event panel needs for first paint. This is
     * intentionally lighter than `loadFullEvent`: no alarms and no recurring
     * override mirrors. Results are cached until the next calendar mutation,
     * and event tiles prefetch this on pointer hover / pointer down.
     */
    async loadPanelEvent(id: string): Promise<CalendarEvent | undefined> {
      return loadPanelEvent(id);
    },

    prefetchPanelEvent(id: string): void {
      prefetchPanelEvent(id);
    },

    /**
     * Fetch the full DB row for one event id and return a fully populated
     * `CalendarEvent`. The render path holds only a slim subset of columns in
     * the current window. Heavy reads retain alarms and override mirrors;
     * export uses a consistent native snapshot and Undo uses a native preimage.
     */
    async loadFullEvent(id: string): Promise<CalendarEvent | undefined> {
      return loadFullEvent(id);
    },

    /**
     * Resolve an event (possibly a recurring instance) to its DB-backed template.
     * Returns the template event, or undefined if not found.
     */
    getTemplate(event: CalendarEvent): CalendarEvent | undefined {
      return resolveToTemplate(event);
    },

    /**
     * Insert or update a batch of events into a target calendar, deduplicated
     * by (calendar_id, source_uid). Newer revisions (higher SEQUENCE) win;
     * equal SEQUENCE counts as an update so re-importing the same file leaves
     * the DB clean. Child rows (attendees, alarms, overrides) are replaced.
     *
     * The whole batch ships as a typed payload to the Rust
     * `calendar_bulk_import` command. Rust deduplicates by UID, compares
     * SEQUENCE, replaces child rows, and commits once.
     */
    async bulkImport(
      events: CalendarEvent[],
      targetCalendarId: string,
      options: CalendarBulkImportOptions = {},
    ): Promise<IcsImportSummary> {
      const result = await bulkImportCalendarEvents(events, targetCalendarId, options);
      if (!result.applied) return result.summary;

      totalEventCount += result.added;
      if (result.refreshWindow) {
        await reloadCurrentWindowFromDb();
      }
      publishCalendarWindowSync();
      return result.summary;
    },

    /**
     * Serialize every event of `calendar` into a `.ics` string ready to write
     * to disk. Rust reads the calendar, full events, and preservation in one
     * bounded SQLite snapshot; the frontend owns iCalendar serialization.
     */
    async exportCalendarAsIcs(calendar: Calendar): Promise<string> {
      return exportCalendarIcs(calendar);
    },

  };
  return store;
}
