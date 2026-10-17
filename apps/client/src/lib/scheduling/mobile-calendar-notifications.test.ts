import { describe, expect, it, vi } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import type { Translate } from "$lib/i18n/translator.svelte";
import {
  buildNativeCalendarNotifications,
  calendarEventIdFromNativeAction,
  calendarNativeNotificationId,
  loadNotificationSchedulerEvents,
} from "./mobile-calendar-notifications";
import { loadNativeCalendarWindow } from "$lib/stores/calendar/native-window";

vi.mock("$lib/stores/calendar/native-window", () => ({ loadNativeCalendarWindow: vi.fn() }));

const t = ((key: string) => key) as Translate;

function event(overrides: Partial<CalendarEvent> = {}): CalendarEvent {
  return {
    id: "event-a",
    title: "Planning",
    start: "2026-08-28 10:00",
    end: "2026-08-28 11:00",
    timezone: "America/Monterrey",
    calendarId: "calendar-a",
    notifications: [0, 30],
    ...overrides,
  };
}

describe("mobile Calendar notification delivery", () => {
  it("uses the native reminder projection as its only occurrence source", async () => {
    const nativeEvent = event({ id: "series::2024-03-11", recurringParentId: "series", recurrenceDate: "2024-03-11" });
    vi.mocked(loadNativeCalendarWindow).mockResolvedValueOnce({ sourceEvents: [], windowEvents: [nativeEvent], totalEventCount: null, diagnostics: [] });
    expect(await loadNotificationSchedulerEvents()).toEqual([nativeEvent]);
    expect(loadNativeCalendarWindow).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ includeTotalEventCount: false }), "notifications");
  });

  it("creates stable, ordered native deliveries and removes duplicate offsets", () => {
    const notifications = buildNativeCalendarNotifications(
      [event({ notifications: [30, 0, 30] })],
      {
        nowMs: new Date(2026, 7, 28, 8, 0).getTime(),
        locale: "en-US",
        t,
        titleFallback: "Calendar event",
      },
    );

    expect(notifications).toHaveLength(2);
    expect(notifications.map((notification) => notification.scheduledAtEpochMs)).toEqual([
      new Date(2026, 7, 28, 9, 30).getTime(),
      new Date(2026, 7, 28, 10, 0).getTime(),
    ]);
    expect(notifications[0]).toMatchObject({
      title: "Planning",
      eventId: "event-a",
    });
    expect(notifications[0]?.id).toBe(
      calendarNativeNotificationId("calendar-notification::event-a::2026-08-28 10:00::30"),
    );
  });

  it("excludes cancelled events and reminders that are already stale", () => {
    const notifications = buildNativeCalendarNotifications(
      [
        event({ id: "cancelled", status: "cancelled" }),
        event({ id: "past", start: "2026-08-28 07:00", end: "2026-08-28 08:00" }),
      ],
      {
        nowMs: new Date(2026, 7, 28, 8, 0).getTime(),
        locale: "en-US",
        t,
        titleFallback: "Calendar event",
      },
    );

    expect(notifications).toEqual([]);
  });

  it("uses the canonical instant for a repeated local time instead of resolving its wall label again", () => {
    const startInstant = "2024-11-03T06:30:00.000Z";
    const deliveries = buildNativeCalendarNotifications([event({
      start: "2024-11-03 01:30", end: "2024-11-03 01:45", timezone: "America/New_York",
      startInstant, endInstant: "2024-11-03T06:45:00.000Z", notifications: [10],
    })], { nowMs: Date.parse("2024-11-03T05:00:00Z"), locale: "en-US", t, titleFallback: "Event" });
    expect(deliveries).toHaveLength(1);
    expect(deliveries[0].scheduledAtEpochMs).toBe(Date.parse(startInstant) - 10 * 60_000);
  });

  it("uses the parent event ID for recurring occurrence actions", () => {
    const [notification] = buildNativeCalendarNotifications(
      [event({ id: "occurrence-a", recurringParentId: "series-a", notifications: [0] })],
      {
        nowMs: new Date(2026, 7, 28, 8, 0).getTime(),
        locale: "en-US",
        t,
        titleFallback: "Calendar event",
      },
    );

    expect(notification?.eventId).toBe("series-a");
  });

  it("bounds the rolling native schedule below Android's per-app alarm limit", () => {
    const notifications = buildNativeCalendarNotifications(
      Array.from({ length: 300 }, (_, index) => event({
        id: `event-${index}`,
        notifications: [0],
      })),
      {
        nowMs: new Date(2026, 7, 28, 8, 0).getTime(),
        locale: "en-US",
        t,
        titleFallback: "Calendar event",
      },
    );

    expect(notifications).toHaveLength(256);
  });

  it("accepts only bounded Calendar tap payloads", () => {
    expect(calendarEventIdFromNativeAction({ eventId: "event-a" })).toBe("event-a");
    expect(calendarEventIdFromNativeAction({ eventId: "" })).toBeNull();
    expect(calendarEventIdFromNativeAction({ eventId: 42 })).toBeNull();
  });
});
