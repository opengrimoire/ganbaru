import { Temporal } from "@js-temporal/polyfill";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import type { MappedNativeCalendarWindow } from "./native-window";
import { loadNativeCalendarWindow } from "./native-window";
import { clearPomodoroSchedulerWindowCache, loadPomodoroSchedulerEventsFromDb } from "./pomodoro-window";

vi.mock("./native-window", () => ({ loadNativeCalendarWindow: vi.fn() }));
vi.mock("./event-payloads", () => ({ localTimezone: () => "America/New_York" }));

const start = Temporal.PlainDate.from("2024-03-09");
const end = Temporal.PlainDate.from("2024-03-15");

function snapshot(): MappedNativeCalendarWindow {
  const commitment: CalendarEvent = {
    id: "series::2024-03-11", recurringParentId: "series", recurrenceDate: "2024-03-11", title: "Native commitment",
    start: "2024-03-11 09:00", end: "2024-03-11 10:00", timezone: "America/New_York", calendarId: "calendar",
    pomodoroConfig: { rhythm: { kind: "count", focusDurationMinutes: 40, shortBreakMinutes: 5, longBreakMinutes: 10, longBreakAfterFocusCount: 4 },
      rhythmSource: "custom", presetKey: null, idleTimeoutMinutes: null },
  };
  return { sourceEvents: [], windowEvents: [commitment], totalEventCount: null, diagnostics: [] };
}

describe("native Pomodoro Calendar window cache", () => {
  beforeEach(() => { vi.clearAllMocks(); clearPomodoroSchedulerWindowCache(); });

  it("coalesces a current window into one native read and retains its canonical occurrence", async () => {
    vi.mocked(loadNativeCalendarWindow).mockResolvedValue(snapshot());
    const [first, second] = await Promise.all([
      loadPomodoroSchedulerEventsFromDb(start, end, 1), loadPomodoroSchedulerEventsFromDb(start, end, 1),
    ]);
    expect(first).toEqual(second);
    expect(first[0].id).toBe("series::2024-03-11");
    expect(loadNativeCalendarWindow).toHaveBeenCalledExactlyOnceWith({ windowStartDate: "2024-03-09", windowEndDate: "2024-03-15",
      renderZone: "America/New_York", includeTotalEventCount: false }, "focus");
  });

  it("reads again when canonical Calendar invalidation changes the version", async () => {
    vi.mocked(loadNativeCalendarWindow).mockResolvedValue(snapshot());
    await loadPomodoroSchedulerEventsFromDb(start, end, 1);
    await loadPomodoroSchedulerEventsFromDb(start, end, 2);
    expect(loadNativeCalendarWindow).toHaveBeenCalledTimes(2);
  });

  it("propagates a read failure and permits a retry instead of caching the failure", async () => {
    vi.mocked(loadNativeCalendarWindow).mockRejectedValueOnce(new Error("native read failed")).mockResolvedValue(snapshot());
    await expect(loadPomodoroSchedulerEventsFromDb(start, end, 1)).rejects.toThrow("native read failed");
    expect(await loadPomodoroSchedulerEventsFromDb(start, end, 1)).toHaveLength(1);
    expect(loadNativeCalendarWindow).toHaveBeenCalledTimes(2);
  });
});
