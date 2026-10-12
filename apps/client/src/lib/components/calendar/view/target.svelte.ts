import { tick } from "svelte";
import { mark as perfMark } from "$lib/stores/perf-log.svelte";
import type { CalendarViewMode } from "$lib/calendar/types";
import { addDays, adjacentWorkCycleAnchor, computeViewWindow } from "$lib/calendar/utils";
import {
  nextDayHeaderReturnMode,
  type DayHeaderReturnMode,
} from "./view-navigation";
import type { createPersistedPomodoroSegmentsController } from "./persisted-segments.svelte";

export type CalendarViewState = { mode: CalendarViewMode; date: Date };
export type CalendarTargetReason = "history" | "nav" | "programmatic" | "view";

type ViewWindow = ReturnType<typeof computeViewWindow>;
type PersistedSegmentsController = ReturnType<typeof createPersistedPomodoroSegmentsController>;

interface CalendarTargetStore {
  foregroundWindowLoadBusy: boolean;
  ensureWindowReady(start: ViewWindow["start"], end: ViewWindow["end"]): Promise<void>;
  hasWindow(start: ViewWindow["start"], end: ViewWindow["end"]): boolean;
  loadWindow(start: ViewWindow["start"], end: ViewWindow["end"]): Promise<void>;
  prefetchWindows(requests: ViewWindow[]): void;
  whenForegroundWindowIdle(): Promise<void>;
}

interface CalendarViewTargetControllerOptions {
  calendarStore: CalendarTargetStore;
  getAnchorDate: () => Date;
  getDayHeaderReturnMode: () => DayHeaderReturnMode;
  getViewMode: () => CalendarViewMode;
  getViewWindow: () => ViewWindow;
  getVisibleEventCount: () => number;
  persistedSegments: PersistedSegmentsController;
  setAnchorDate: (date: Date) => void;
  setDayHeaderReturnMode: (mode: DayHeaderReturnMode) => void;
  setPreferredViewMode: (mode: CalendarViewMode) => void;
  setViewMode: (mode: CalendarViewMode) => void;
}

const VIEW_HISTORY_LIMIT = 50;

function sameAnchorPrefetchRequests(
  calendarStore: CalendarTargetStore,
  target: CalendarViewState,
): ViewWindow[] {
  const modes: CalendarViewMode[] = ["day", "workweek", "week", "month"];
  const seen = new Set<string>();
  const requests: ViewWindow[] = [];
  for (const mode of modes) {
    const requestWindow = computeViewWindow(target.date, mode);
    const key = `${requestWindow.start.toString()}..${requestWindow.end.toString()}`;
    if (seen.has(key)) continue;
    seen.add(key);
    if (calendarStore.hasWindow(requestWindow.start, requestWindow.end)) continue;
    requests.push(requestWindow);
  }
  return requests;
}

export function createCalendarViewTargetController({
  calendarStore,
  getAnchorDate,
  getDayHeaderReturnMode,
  getViewMode,
  getViewWindow,
  getVisibleEventCount,
  persistedSegments,
  setAnchorDate,
  setDayHeaderReturnMode,
  setPreferredViewMode,
  setViewMode,
}: CalendarViewTargetControllerOptions) {
  let history: CalendarViewState[] = $state([{ mode: getViewMode(), date: new Date(getAnchorDate()) }]);
  let historyIndex = $state(0);
  let pendingTarget: CalendarViewState | null = null;
  let targetCommitPromise: Promise<void> | null = null;
  let targetRequestSeq = 0;
  let targetPaintPending = false;

  function currentState(): CalendarViewState {
    return pendingTarget ?? { mode: getViewMode(), date: getAnchorDate() };
  }

  function pushHistory(mode: CalendarViewMode, date: Date): void {
    const base = history.slice(0, historyIndex + 1);
    base[base.length - 1] = { mode: getViewMode(), date: new Date(getAnchorDate()) };
    history = [...base, { mode, date }];
    if (history.length > VIEW_HISTORY_LIMIT) {
      history = history.slice(history.length - VIEW_HISTORY_LIMIT);
    }
    historyIndex = history.length - 1;
  }

  function prefetchSameAnchor(target: CalendarViewState): void {
    calendarStore.prefetchWindows(sameAnchorPrefetchRequests(calendarStore, target));
  }

  function request(
    target: CalendarViewState,
    options: {
      history: boolean;
      reason: CalendarTargetReason;
    },
  ): Promise<void> {
    const requestId = ++targetRequestSeq;
    const normalizedTarget: CalendarViewState = { mode: target.mode, date: new Date(target.date) };
    pendingTarget = normalizedTarget;
    targetPaintPending = true;
    const targetWindow = computeViewWindow(normalizedTarget.date, normalizedTarget.mode);

    const promise = (async () => {
      try {
        await calendarStore.ensureWindowReady(targetWindow.start, targetWindow.end);
        if (requestId !== targetRequestSeq) return;
        const segmentSnapshot = await persistedSegments.ensureForTarget(normalizedTarget);
        if (requestId !== targetRequestSeq) return;

        if (options.history) pushHistory(normalizedTarget.mode, normalizedTarget.date);
        persistedSegments.applySnapshot(segmentSnapshot);
        setDayHeaderReturnMode(nextDayHeaderReturnMode(
          getDayHeaderReturnMode(),
          normalizedTarget.mode,
        ));
        setViewMode(normalizedTarget.mode);
        setPreferredViewMode(normalizedTarget.mode);
        setAnchorDate(normalizedTarget.date);
        void calendarStore.loadWindow(targetWindow.start, targetWindow.end)
          .catch((error) => console.error("[CalendarView] target state apply failed:", error));
        pendingTarget = null;
        prefetchSameAnchor(normalizedTarget);
        void tick().then(() => {
          if (requestId !== targetRequestSeq) return;
          perfMark(
            options.reason === "view" ? "view.mounted" : "nav.display-ready",
            { count: getVisibleEventCount() },
          );
          requestAnimationFrame(() => {
            if (requestId !== targetRequestSeq) return;
            targetPaintPending = false;
            perfMark(options.reason === "view" ? "view.paint-done" : "nav.paint-done");
          });
        });
      } catch (error) {
        if (requestId !== targetRequestSeq) return;
        pendingTarget = null;
        targetPaintPending = false;
        console.error("[CalendarView] target commit failed:", error);
      } finally {
        if (requestId === targetRequestSeq) targetCommitPromise = null;
      }
    })();

    targetCommitPromise = promise;
    return promise;
  }

  function historyBack(): void {
    if (historyIndex <= 0) return;
    historyIndex--;
    void request(history[historyIndex], { history: false, reason: "history" });
  }

  function historyForward(): void {
    if (historyIndex >= history.length - 1) return;
    historyIndex++;
    void request(history[historyIndex], { history: false, reason: "history" });
  }

  function targetAnchorForNavigation(direction: "forward" | "back"): Date {
    const delta = direction === "forward" ? 1 : -1;
    const currentTarget = currentState();
    const base = currentTarget.date;
    if (currentTarget.mode === "week") return addDays(base, 7 * delta);
    if (currentTarget.mode === "workweek") return adjacentWorkCycleAnchor(base, direction);
    if (currentTarget.mode === "day") return addDays(base, delta);
    const d = new Date(base);
    const targetMonth = d.getMonth() + delta;
    d.setDate(1);
    d.setMonth(targetMonth);
    return d;
  }

  function canRepeatHeldNavigation(direction: "forward" | "back"): boolean {
    const viewWindow = getViewWindow();
    if (
      pendingTarget !== null
      || targetPaintPending
      || calendarStore.foregroundWindowLoadBusy
      || !calendarStore.hasWindow(viewWindow.start, viewWindow.end)
    ) {
      return false;
    }
    const targetMode = currentState().mode;
    const targetWindow = computeViewWindow(targetAnchorForNavigation(direction), targetMode);
    return calendarStore.hasWindow(targetWindow.start, targetWindow.end);
  }

  async function waitForSettled(): Promise<void> {
    if (targetCommitPromise) await targetCommitPromise;
    await tick();
    await calendarStore.whenForegroundWindowIdle();
    await tick();
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => resolve());
    });
  }

  function clearPending(): void {
    pendingTarget = null;
    targetPaintPending = false;
  }

  return {
    canRepeatHeldNavigation,
    clearPending,
    currentState,
    historyBack,
    historyForward,
    prefetchSameAnchor,
    request,
    targetAnchorForNavigation,
    waitForSettled,
  };
}
