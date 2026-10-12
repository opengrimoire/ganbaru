import { afterEach, describe, expect, it, vi } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import type { Translate } from "$lib/i18n/translator.svelte";
import { createPresetPomodoroConfig } from "$lib/pomodoro/rhythm";
import { buildMobileFocusNotificationCopy, buildMobilePomodoroSchedule, reconcileMobilePomodoroSchedule } from "./mobile-pomodoro-schedule";

const { invokeMock, loadEventsMock } = vi.hoisted(() => ({
  invokeMock: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  loadEventsMock: vi.fn<() => Promise<CalendarEvent[]>>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("./mobile-calendar-notifications", () => ({ loadNotificationSchedulerEvents: loadEventsMock }));
afterEach(() => {
  vi.restoreAllMocks();
  invokeMock.mockReset();
  loadEventsMock.mockReset();
});

const t = ((key: string) => key) as Translate;

it("supplies notification language without any execution or deadline fields", () => {
  const copy = buildMobileFocusNotificationCopy(t);
  expect(Object.keys(copy).sort()).toEqual([
    "alertsChannelDescription", "alertsChannelName", "breakCompleteTitle", "channelDescription",
    "channelName", "focusCompleteTitle", "focusTitle", "longBreakTitle", "pausedText",
    "sessionCompleteText", "shortBreakTitle",
  ]);
  const translated = buildMobileFocusNotificationCopy(((key: string) => `translated:${key}`) as Translate);
  expect(translated.focusTitle).toBe("translated:pomodoroNotification.focusTitle");
  expect(translated.sessionCompleteText).toBe("translated:pomodoroNotification.sessionCompleteText");
});

it("reconciles canonical reminders while sending only localized copy to the native owner", async () => {
  invokeMock.mockResolvedValue(undefined);
  loadEventsMock.mockResolvedValue([]);
  await reconcileMobilePomodoroSchedule(t);
  expect(invokeMock.mock.calls).toEqual([
    ["focus_notification_copy", { copy: buildMobileFocusNotificationCopy(t) }],
    ["plugin:ganbaru-mobile-notifications|reconcilePomodoroSchedule", { schedule: [] }],
  ]);
  expect(loadEventsMock).toHaveBeenCalledOnce();
});

it("keeps commitment reminders independent of failed notification language delivery", async () => {
  invokeMock.mockImplementation(async (command) => {
    if (command === "focus_notification_copy") throw new Error("Language delivery unavailable");
  });
  loadEventsMock.mockResolvedValue([]);
  await expect(reconcileMobilePomodoroSchedule(t)).rejects.toThrow("Language delivery unavailable");
  expect(invokeMock).toHaveBeenCalledWith(
    "plugin:ganbaru-mobile-notifications|reconcilePomodoroSchedule", { schedule: [] },
  );
});

function focusEvent(overrides: Partial<CalendarEvent> = {}): CalendarEvent {
  return {
    id: "focus-a",
    title: "Write release notes",
    start: "2026-08-28 10:00",
    end: "2026-08-28 11:00",
    timezone: "America/Monterrey",
    calendarId: "calendar-a",
    pomodoroConfig: createPresetPomodoroConfig("creative"),
    ...overrides,
  };
}

describe("mobile focus commitment reminders", () => {
  it("schedules a reminder without run or phase state", () => {
    const [projection] = buildMobilePomodoroSchedule(
      [focusEvent()],
      t,
      new Date(2026, 7, 28, 9, 0).getTime(),
    );

    expect(projection).toEqual({
      id: expect.stringMatching(/^reminder-/),
      eventId: "focus-a",
      title: "Write release notes",
      body: "pomodoroNotification.commitmentDueText",
      channelName: "pomodoroNotification.alertsChannelName",
      channelDescription: "pomodoroNotification.alertsChannelDescription",
      startsAtEpochMs: new Date(2026, 7, 28, 10, 0).getTime(),
      endsAtEpochMs: new Date(2026, 7, 28, 11, 0).getTime(),
    });
  });

  it("keeps an ongoing commitment for a due reminder and excludes expired events", () => {
    const schedule = buildMobilePomodoroSchedule(
      [
        focusEvent({ id: "ongoing" }),
        focusEvent({ id: "expired", start: "2026-08-28 08:00", end: "2026-08-28 09:00" }),
      ],
      t,
      new Date(2026, 7, 28, 10, 30).getTime(),
    );

    expect(schedule.map((projection) => projection.eventId)).toEqual(["ongoing"]);
  });

  it("ignores cancelled, all-day, and ordinary calendar events", () => {
    expect(buildMobilePomodoroSchedule([
      focusEvent({ id: "cancelled", status: "cancelled" }),
      focusEvent({ id: "all-day", allDay: true }),
      focusEvent({ id: "ordinary", pomodoroConfig: undefined }),
    ], t, new Date(2026, 7, 28, 9, 0).getTime())).toEqual([]);
  });
});

it("keeps a reminder identity through end edits and deduplicates repeated occurrences", () => {
  const now = new Date(2026, 7, 28, 9, 0).getTime();
  const original = focusEvent();
  const changed = focusEvent({ end: "2026-08-28 12:00" });
  expect(buildMobilePomodoroSchedule([original], t, now)[0]?.id)
    .toBe(buildMobilePomodoroSchedule([changed], t, now)[0]?.id);
  expect(buildMobilePomodoroSchedule([original, original], t, now)).toHaveLength(1);
  expect(buildMobilePomodoroSchedule([original], t, Number.NaN)).toEqual([]);
});
