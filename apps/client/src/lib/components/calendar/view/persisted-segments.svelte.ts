import { invoke } from "@tauri-apps/api/core";
import { dbUrl } from "$lib/api/db";
import type { CalendarEvent, CalendarViewMode, PersistedSegment } from "$lib/calendar/types";
import { computeViewWindow } from "$lib/calendar/utils";
import {
  mapPomodoroSegmentRows,
  parsePomodoroSegmentRows,
  pomodoroSegmentSnapshotKey,
  visiblePomodoroEventIds,
} from "./pomodoro-rail-segments";

type ViewState = { mode: CalendarViewMode; date: Date };
type ViewWindow = ReturnType<typeof computeViewWindow>;
type SegmentSnapshot = {
  key: string;
  map: ReadonlyMap<string, PersistedSegment[]>;
  unavailable: boolean;
};

interface PersistedSegmentsControllerOptions {
  getSegmentVersion: () => number;
  visibleStoreEventsForWindow: (window: ViewWindow) => CalendarEvent[];
}

function shouldLoadRailSegmentsFor(mode: CalendarViewMode): boolean {
  return mode === "day" || mode === "workweek" || mode === "week";
}

export function createPersistedPomodoroSegmentsController({
  getSegmentVersion,
  visibleStoreEventsForWindow,
}: PersistedSegmentsControllerOptions) {
  let byEvent = $state<ReadonlyMap<string, PersistedSegment[]>>(new Map());
  let snapshotKey = $state("");
  let unavailable = $state(false);
  let requestSeq = 0;

  async function loadForEvents(events: readonly CalendarEvent[]): Promise<SegmentSnapshot> {
    const visibleIds = visiblePomodoroEventIds(events);
    const key = pomodoroSegmentSnapshotKey(getSegmentVersion(), visibleIds);
    if (key === snapshotKey) {
      return { key, map: byEvent, unavailable: false };
    }
    if (visibleIds.length === 0) {
      return { key, map: new Map(), unavailable: false };
    }

    try {
      const rows = parsePomodoroSegmentRows(await invoke<unknown>("pomodoro_load_segments_for_events", {
        dbUrl: dbUrl(),
        eventIds: visibleIds,
      }), visibleIds);
      return { key, map: mapPomodoroSegmentRows(rows, visibleIds), unavailable: false };
    } catch (error) {
      console.warn("[CalendarView] Failed to load pomodoro rail segments:", error);
      return { key: "", map: new Map(), unavailable: true };
    }
  }

  async function ensureForTarget(target: ViewState): Promise<SegmentSnapshot> {
    if (!shouldLoadRailSegmentsFor(target.mode)) {
      return { key: "", map: new Map(), unavailable: false };
    }
    const targetWindow = computeViewWindow(target.date, target.mode);
    return loadForEvents(visibleStoreEventsForWindow(targetWindow));
  }

  function applySnapshot(snapshot: SegmentSnapshot): void {
    if (snapshot.key === snapshotKey && snapshot.map === byEvent && snapshot.unavailable === unavailable) return;
    snapshotKey = snapshot.key;
    byEvent = snapshot.map;
    unavailable = snapshot.unavailable;
  }

  function refreshForTarget(target: ViewState): void {
    const seq = ++requestSeq;
    void ensureForTarget(target)
      .then((snapshot) => {
        if (seq !== requestSeq) return;
        applySnapshot(snapshot);
      })
      .catch((error) => {
        if (seq !== requestSeq) return;
        console.warn("[CalendarView] Failed to load pomodoro rail segments:", error);
        applySnapshot({ key: "", map: new Map(), unavailable: true });
      });
  }

  return {
    get byEvent() {
      return byEvent;
    },
    get unavailable() {
      return unavailable;
    },
    ensureForTarget,
    applySnapshot,
    refreshForTarget,
  };
}
