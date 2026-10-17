import { Temporal } from "@js-temporal/polyfill";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import type { CalendarEvent } from "$lib/calendar/types";
import { loadNativeCalendarWindow, type MappedNativeCalendarWindow } from "$lib/stores/calendar/native-window";
import { publishCalendarWindowSync } from "$lib/stores/calendar/window-sync";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/stores/calendar/native-window", async (original) => ({
  ...await original<typeof import("$lib/stores/calendar/native-window")>(), loadNativeCalendarWindow: vi.fn(),
}));
vi.mock("$lib/stores/calendar/window-prefetch", async (original) => ({
  ...await original<typeof import("$lib/stores/calendar/window-prefetch")>(), adjacentCalendarWindowRequests: () => [],
}));
vi.mock("$lib/stores/calendar/window-sync", () => ({ initCalendarWindowSync: vi.fn(), publishCalendarWindowSync: vi.fn() }));
vi.mock("./perf-log.svelte", () => ({ mark: vi.fn() }));
vi.mock("$lib/stores/preferences.svelte", () => ({ getPreferences: () => ({ calendarViewMode: "week" }) }));

const start = Temporal.PlainDate.from("2024-03-09");
const end = Temporal.PlainDate.from("2024-03-15");

function event(id = "series::2024-03-11", startTime = "2024-03-11 09:00"): CalendarEvent {
  return { id, title: "Native occurrence", start: startTime, end: "2024-03-11 10:00",
    timezone: "UTC", calendarId: "calendar", recurrenceDate: "2024-03-11", recurringParentId: "series" };
}

function snapshot(events = [event()]): MappedNativeCalendarWindow {
  return { sourceEvents: [{ ...event("series"), recurringParentId: undefined,
    recurrence: { frequency: "daily", interval: 1, end: { type: "never" } } }],
  windowEvents: events, totalEventCount: 1, diagnostics: [] };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => { resolve = accept; });
  return { promise, resolve };
}

describe("Calendar store native occurrence ownership", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    vi.mocked(invoke).mockResolvedValue(undefined);
    vi.mocked(loadNativeCalendarWindow).mockResolvedValue(snapshot());
  });

  it("loads the native snapshot and filters covering windows without expanding templates", async () => {
    const { getCalendar } = await import("./calendar.svelte");
    const store = getCalendar();
    await store.loadWindow(start, end);
    expect(loadNativeCalendarWindow).toHaveBeenCalledTimes(1);
    expect(invoke).not.toHaveBeenCalled();
    expect(store.eventsInWindow(start, end)).toEqual([event()]);
    expect(store.eventsInWindow(Temporal.PlainDate.from("2024-03-11"), Temporal.PlainDate.from("2024-03-11"))).toEqual([event()]);
    expect(store.eventsInWindow(start, start)).toEqual([]);
    await store.loadWindow(start, end);
    expect(loadNativeCalendarWindow).toHaveBeenCalledTimes(1);
  });

  it("refreshes canonical occurrences after a committed deletion instead of regenerating local rows", async () => {
    const { getCalendar } = await import("./calendar.svelte");
    const store = getCalendar();
    await store.loadWindow(start, end);
    vi.mocked(loadNativeCalendarWindow).mockResolvedValue({ ...snapshot([]), sourceEvents: [], totalEventCount: 0 });
    store.acceptNativeEdit();
    await store.refreshWindow(start, end);
    expect(invoke).not.toHaveBeenCalled();
    expect(loadNativeCalendarWindow).toHaveBeenLastCalledWith(expect.objectContaining({ includeTotalEventCount: true }));
    expect(loadNativeCalendarWindow).toHaveBeenCalledTimes(2);
    expect(store.eventsInWindow(start, end)).toEqual([]);
    expect(store.sourceEvents).toEqual([]);
    expect(store.eventCount).toBe(0);
  });

  it("does not reuse a stale in-flight read after a committed mutation of the same window", async () => {
    const { getCalendar } = await import("./calendar.svelte");
    const store = getCalendar();
    await store.loadWindow(start, end);
    const staleRead = deferred<MappedNativeCalendarWindow>();
    vi.mocked(loadNativeCalendarWindow).mockReturnValueOnce(staleRead.promise)
      .mockResolvedValueOnce({ ...snapshot([]), sourceEvents: [], totalEventCount: null });
    const refresh = store.refreshCurrentWindow();
    await vi.waitFor(() => expect(loadNativeCalendarWindow).toHaveBeenCalledTimes(2));
    store.acceptNativeEdit();
    const deletion = store.refreshWindow(start, end);
    staleRead.resolve(snapshot([event("stale")]));
    await Promise.all([refresh, deletion]);
    expect(loadNativeCalendarWindow).toHaveBeenCalledTimes(3);
    expect(store.eventsInWindow(start, end)).toEqual([]);
    expect(store.sourceEvents).toEqual([]);
  });

  it("refreshes the global count from native acceptance rather than inferring removed rows from the viewport", async () => {
    const { getCalendar } = await import("./calendar.svelte");
    const store = getCalendar();
    await store.loadWindow(start, end);
    store.acceptNativeEdit();
    expect(loadNativeCalendarWindow).toHaveBeenCalledTimes(1);
    vi.mocked(loadNativeCalendarWindow).mockResolvedValue({ ...snapshot([]), sourceEvents: [], totalEventCount: 4_000 });
    await store.refreshWindow(start, end);
    expect(loadNativeCalendarWindow).toHaveBeenCalledTimes(2);
    expect(store.eventsInWindow(start, end)).toEqual([]);
    expect(store.eventCount).toBe(4_000);
  });

  it("retains projection diagnostics and permits a retry after a failed read", async () => {
    const { getCalendar } = await import("./calendar.svelte");
    const store = getCalendar();
    vi.mocked(loadNativeCalendarWindow).mockRejectedValueOnce(new Error("Native window unavailable"));
    await expect(store.loadWindow(start, end)).rejects.toThrow("Native window unavailable");
    vi.mocked(loadNativeCalendarWindow).mockResolvedValue({ ...snapshot([]), diagnostics: [{ event_id: "series", message: "Unsupported rule" }] });
    await store.loadWindow(start, end);
    expect(store.expansionDiagnostics).toEqual([{ event_id: "series", message: "Unsupported rule" }]);
    expect(store.sourceEvents).toHaveLength(1);
    expect(store.eventsInWindow(start, end)).toEqual([]);
  });

  it("invalidates cached windows and publishes acceptance even if the next visible read fails", async () => {
    const { getCalendar } = await import("./calendar.svelte");
    const store = getCalendar();
    await store.loadWindow(start, end);
    store.acceptNativeEdit();
    expect(store.isWindowCurrent(start, end)).toBe(false);
    expect(store.eventsInWindow(start, end)).toEqual([]);
    expect(publishCalendarWindowSync).toHaveBeenCalledOnce();
    vi.mocked(loadNativeCalendarWindow).mockRejectedValueOnce(new Error("Read unavailable"));
    await expect(store.refreshWindow(start, end)).rejects.toThrow("Read unavailable");
    expect(publishCalendarWindowSync).toHaveBeenCalledOnce();
  });

  it("does not report an old window as current after a failed canonical refresh", async () => {
    const { getCalendar } = await import("./calendar.svelte");
    const store = getCalendar();
    await store.loadWindow(start, end);
    vi.mocked(loadNativeCalendarWindow).mockRejectedValueOnce(new Error("Refresh unavailable"));
    await expect(store.refreshCurrentWindow()).rejects.toThrow("Refresh unavailable");
    expect(store.hasWindow(start, end)).toBe(false);
    expect(store.isWindowCurrent(start, end)).toBe(false);
    expect(store.eventsInWindow(start, end)).toEqual([]);
    await store.loadWindow(start, end);
    expect(store.isWindowCurrent(start, end)).toBe(true);
  });
});

describe("Calendar stores of split panes", () => {
  const nextStart = Temporal.PlainDate.from("2024-03-16");
  const nextEnd = Temporal.PlainDate.from("2024-03-22");

  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    vi.mocked(loadNativeCalendarWindow).mockResolvedValue(snapshot());
  });

  it("loads the secondary pane's first window from the shared cache while the app boots only the primary", async () => {
    const { getCalendarStore } = await import("./calendar.svelte");
    const primary = getCalendarStore("primary");
    const secondary = getCalendarStore("secondary");
    await primary.loadWindow(start, end);

    await primary.loadInitialWindow(nextStart, nextEnd);
    expect(primary.isWindowCurrent(start, end)).toBe(true);

    await secondary.loadInitialWindow(start, end);
    expect(loadNativeCalendarWindow).toHaveBeenCalledTimes(1);
    expect(secondary.loaded).toBe(true);
    expect(secondary.eventsInWindow(start, end)).toEqual([event()]);

    await secondary.loadInitialWindow(nextStart, nextEnd);
    expect(secondary.isWindowCurrent(start, end)).toBe(true);
  });

  it("reloads the other pane's own window after a commit through either pane", async () => {
    const { getCalendarStore } = await import("./calendar.svelte");
    const primary = getCalendarStore("primary");
    const secondary = getCalendarStore("secondary");
    await primary.loadWindow(start, end);
    await secondary.loadWindow(nextStart, nextEnd);
    expect(loadNativeCalendarWindow).toHaveBeenCalledTimes(2);

    vi.mocked(loadNativeCalendarWindow).mockResolvedValue({ ...snapshot([]), sourceEvents: [] });
    primary.acceptNativeEdit();
    await primary.refreshWindow(start, end);

    expect(loadNativeCalendarWindow).toHaveBeenCalledTimes(4);
    expect(loadNativeCalendarWindow).toHaveBeenCalledWith(expect.objectContaining({ windowStartDate: "2024-03-16" }));
    expect(primary.isWindowCurrent(start, end)).toBe(true);
    expect(secondary.isWindowCurrent(nextStart, nextEnd)).toBe(true);
    expect(secondary.eventsInWindow(nextStart, nextEnd)).toEqual([]);
    expect(secondary.sourceEvents).toEqual([]);
  });

  it("keeps a commit's refresh successful when only the other pane's reload fails", async () => {
    const { getCalendarStore } = await import("./calendar.svelte");
    const primary = getCalendarStore("primary");
    const secondary = getCalendarStore("secondary");
    await primary.loadWindow(start, end);
    await secondary.loadWindow(nextStart, nextEnd);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.mocked(loadNativeCalendarWindow).mockImplementation(async ({ windowStartDate }) => {
      if (windowStartDate === "2024-03-16") throw new Error("Secondary read unavailable");
      return snapshot([]);
    });

    await expect(primary.refreshCurrentWindow()).resolves.toBeUndefined();

    expect(primary.isWindowCurrent(start, end)).toBe(true);
    expect(secondary.isWindowCurrent(nextStart, nextEnd)).toBe(false);
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });
});
