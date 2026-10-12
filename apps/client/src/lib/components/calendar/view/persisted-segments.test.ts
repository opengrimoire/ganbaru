import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPersistedPomodoroSegmentsController } from "./persisted-segments.svelte";
import { createPresetPomodoroConfig } from "$lib/pomodoro/rhythm";
import type { CalendarEvent } from "$lib/calendar/types";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("$lib/api/db", () => ({ dbUrl: () => "sqlite:active" }));

const target = { mode: "week" as const, date: new Date(2026, 4, 4) };
const event: CalendarEvent = {
  id: "source::2026-05-04", title: "Focus", start: "2026-05-04 09:00", end: "2026-05-04 10:00",
  timezone: "UTC", calendarId: "local", pomodoroConfig: createPresetPomodoroConfig("deep"),
};
const row = {
  id: "segment", event_id: event.id, event_date: "2026-05-03", run_id: "run", rhythm_position: 1,
  phase: "focus", status: "completed", planned_start: "2026-05-04T09:00:00Z",
  planned_end: "2026-05-04T09:25:00Z", actual_start: "2026-05-04T09:00:00Z",
  actual_end: "2026-05-04T09:25:00Z", pauses: [],
};

function controller() {
  return createPersistedPomodoroSegmentsController({
    getSegmentVersion: () => 1,
    visibleStoreEventsForWindow: () => [event],
  });
}

beforeEach(() => {
  mocks.invoke.mockReset();
  vi.spyOn(console, "warn").mockImplementation(() => {});
});
afterEach(() => vi.restoreAllMocks());

describe("Calendar recorded Focus timeline", () => {
  it("allows navigation to an oversized history while showing its unavailable state", async () => {
    mocks.invoke.mockRejectedValueOnce(new Error("Focus Calendar segments exceed their row budget"))
      .mockResolvedValueOnce([row]);
    const history = controller();
    const failed = await history.ensureForTarget(target);
    history.applySnapshot(failed);
    expect(history.unavailable).toBe(true);
    expect(history.byEvent.size).toBe(0);

    const retried = await history.ensureForTarget(target);
    history.applySnapshot(retried);
    expect(history.unavailable).toBe(false);
    expect(history.byEvent.get(event.id)).toHaveLength(1);
    expect(mocks.invoke).toHaveBeenCalledTimes(2);
    expect(mocks.invoke).toHaveBeenCalledWith("pomodoro_load_segments_for_events", {
      dbUrl: "sqlite:active", eventIds: [event.id],
    });
  });

  it("treats malformed native history as unavailable rather than displaying inferred evidence", async () => {
    mocks.invoke.mockResolvedValueOnce([{ ...row, phase: "unknown" }]);
    const history = controller();
    history.applySnapshot(await history.ensureForTarget(target));
    expect(history.unavailable).toBe(true);
    expect(history.byEvent.size).toBe(0);
    history.applySnapshot(await history.ensureForTarget({ ...target, mode: "month" }));
    expect(history.unavailable).toBe(false);
    expect(mocks.invoke).toHaveBeenCalledOnce();
  });

  it("discards a failed older refresh after a newer target has loaded", async () => {
    let reject!: (error: Error) => void;
    mocks.invoke.mockImplementationOnce(() => new Promise<unknown>((_, fail) => { reject = fail; }))
      .mockResolvedValueOnce([row]);
    const history = controller();
    history.refreshForTarget(target);
    history.refreshForTarget({ ...target, mode: "day" });
    await vi.waitFor(() => expect(history.byEvent.get(event.id)).toHaveLength(1));
    reject(new Error("Old request failed"));
    await vi.waitFor(() => expect(console.warn).toHaveBeenCalledOnce());
    expect(history.unavailable).toBe(false);
    expect(history.byEvent.get(event.id)).toHaveLength(1);
  });
});
