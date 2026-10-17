import { describe, expect, it, vi } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import type { EditSessionState } from "$lib/components/calendar/edit-session.svelte";
import type { createCalendarViewToastController } from "./toasts.svelte";
import { CalendarViewSaveController, type CalendarViewSaveControllerOptions } from "./save-controller.svelte";

const selected: CalendarEvent = { id: "event-a", title: "Event",
  start: "2026-07-12 09:00", end: "2026-07-12 10:00", timezone: "UTC", calendarId: "calendar-a" };
const data = { ...selected, description: "" };

function setup(overrides: Partial<CalendarViewSaveControllerOptions> = {}) {
  let state: EditSessionState = { mode: "edit", sessionKey: 1, originalEvent: selected,
    instanceEvent: selected, templateId: selected.id, detailsLoaded: true,
    anchor: { x: 0, y: 0, width: 0, height: 0 } };
  const order: string[] = [];
  const persist = vi.fn(async () => { order.push("persist"); return { saveRefreshedVisibleWindow: false }; });
  const toasts = { saveToast: null, showSavePendingToast: vi.fn(() => "toast-a"),
    showSaveSuccessToast: vi.fn(), showSaveErrorToast: vi.fn(),
    dismissSaveToastIfCurrent: vi.fn() } as unknown as ReturnType<typeof createCalendarViewToastController>;
  let confirmed: (() => Promise<void>) | undefined;
  const options: CalendarViewSaveControllerOptions = {
    toasts, commitService: { persist }, getSessionState: () => state,
    canEnablePomodoro: () => false, isSelectedEndable: () => true,
    wouldSaveStopSession: () => false, endWouldStopProductivity: () => false,
    confirmSaveStop: (action) => { confirmed = action; },
    confirmEndStop: (action) => { confirmed = action; },
    buildFreeze: () => [selected], setDisplayState: vi.fn(),
    refreshWindow: async () => { order.push("refresh"); },
    closeSession: () => { order.push("close"); }, afterRender: async () => undefined,
    savePendingLabel: () => "saving", saveSuccessLabel: () => "saved",
    saveErrorLabel: () => "failed", logError: vi.fn(), ...overrides,
  };
  return { controller: new CalendarViewSaveController(options), persist, toasts, order, options,
    setState: (value: EditSessionState) => { state = value; },
    confirm: async () => { if (!confirmed) throw Error("Missing confirmation"); await confirmed(); } };
}

describe("Calendar panel native actions", () => {
  it("confirms once and refreshes only after the compound Save returns", async () => {
    const test = setup({ wouldSaveStopSession: () => true });
    expect(await test.controller.save(data)).toBe(false);
    expect(test.persist).not.toHaveBeenCalled();
    await test.confirm();
    expect(test.order).toEqual(["persist", "refresh", "close"]);
    expect(test.persist).toHaveBeenCalledWith(data, undefined, { action: "save" });
  });

  it("discards a confirmation whose selected editor has closed", async () => {
    const test = setup({ wouldSaveStopSession: () => true });
    await test.controller.save(data);
    test.setState({ mode: "closed" });
    await test.confirm();
    expect(test.persist).not.toHaveBeenCalled();
  });

  it("requests End now without replacing the end with a browser timestamp", async () => {
    const test = setup();
    await test.controller.end(data, "all");
    expect(test.persist).toHaveBeenCalledWith(data, "all", { action: "end_now" });
    expect(test.controller.endingActiveEvent).toBe(false);
  });

  it("requests atomic Enable Focus when the panel adds an eligible configuration", async () => {
    const test = setup({ canEnablePomodoro: () => true });
    const configured = { ...data, pomodoroConfig: {
      rhythm: { kind: "count" as const, focusDurationMinutes: 25, shortBreakMinutes: 5,
        longBreakMinutes: 15, longBreakAfterFocusCount: 4 },
      rhythmSource: "custom" as const, presetKey: null, idleTimeoutMinutes: null,
    } };
    await test.controller.save(configured);
    expect(test.persist).toHaveBeenCalledWith(configured, undefined, { action: "enable_focus" });
  });

  it("keeps the draft open and reports End now failures", async () => {
    const test = setup();
    test.persist.mockRejectedValueOnce(new Error("Response lost"));
    await test.controller.end(data);
    expect(test.order).toEqual([]);
    expect(test.toasts.showSaveErrorToast).toHaveBeenCalledWith("toast-a", "failed");
    expect(test.options.setDisplayState).toHaveBeenLastCalledWith({
      suppressGlow: false, suppressPreview: false, frozenEvents: null,
    });
    expect(test.controller.endingActiveEvent).toBe(false);
  });

  it("does not close a new editor when an earlier commit finishes", async () => {
    const test = setup({ refreshWindow: async () => { test.setState({ mode: "create", sessionKey: 2,
      start: selected.start, end: selected.end, anchor: { x: 0, y: 0, width: 0, height: 0 } }); } });
    await test.controller.save(data);
    expect(test.order).toEqual(["persist"]);
  });

  it("keeps the panel open when canonical refresh fails after acceptance", async () => {
    const test = setup({ refreshWindow: async () => { throw Error("Refresh unavailable"); } });
    expect(await test.controller.save(data)).toBe(false);
    expect(test.order).toEqual(["persist"]);
    expect(test.toasts.showSaveErrorToast).toHaveBeenCalled();
  });

  it("reports successful persistence explicitly and suppresses a concurrent Save", async () => {
    let resolve!: () => void;
    const waiting = new Promise<void>((done) => { resolve = done; });
    const test = setup({ afterRender: () => waiting });
    const saving = test.controller.save(data);
    await vi.waitFor(() => expect(test.order).toEqual(["persist", "refresh", "close"]));
    expect(test.controller.saving).toBe(true);
    expect(await test.controller.save(data)).toBe(false);
    resolve();
    expect(await saving).toBe(true);
    expect(test.controller.saving).toBe(false);
    expect(test.persist).toHaveBeenCalledOnce();
  });
});
