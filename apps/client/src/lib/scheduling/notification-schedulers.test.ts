import { describe, expect, it, vi } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import type { NotesMentionNotification } from "$lib/notes/types";
import type { SchedulerClock, SchedulerTimeout } from "./lifecycle-scheduler";
import {
  createEventNotificationScheduler,
  createNotesNotificationScheduler,
} from "./notification-schedulers";

class FakeSchedulerClock implements SchedulerClock {
  private nextId = 1;
  private timers = new Map<number, { at: number; callback: () => void }>();

  constructor(private nowMs: number) {}

  now(): number {
    return this.nowMs;
  }

  setTimeout(callback: () => void, delayMs: number): SchedulerTimeout {
    const id = this.nextId++;
    this.timers.set(id, { at: this.nowMs + Math.max(0, delayMs), callback });
    return id as unknown as SchedulerTimeout;
  }

  clearTimeout(timeout: SchedulerTimeout): void {
    this.timers.delete(timeout as unknown as number);
  }

  jumpTo(nowMs: number): void {
    this.nowMs = nowMs;
  }

  async advanceTo(nowMs: number): Promise<void> {
    while (true) {
      const next = [...this.timers.entries()]
        .filter(([, timer]) => timer.at <= nowMs)
        .sort((left, right) => left[1].at - right[1].at || left[0] - right[0])[0];
      if (!next) break;
      const [id, timer] = next;
      this.timers.delete(id);
      this.nowMs = timer.at;
      timer.callback();
      await settleScheduler();
    }
    this.nowMs = nowMs;
    await settleScheduler();
  }
}

async function settleScheduler(): Promise<void> {
  for (let index = 0; index < 10; index += 1) await Promise.resolve();
}

function calendarEvent(start: string): CalendarEvent {
  return {
    id: "event-a",
    title: "Plan",
    start,
    end: start,
    timezone: "UTC",
    calendarId: "calendar-a",
    notifications: [0],
  };
}

const notesPreferences = {
  mentionNotificationsEnabled: true,
  reminderNotificationsEnabled: true,
  userMentionNotificationsEnabled: true,
  taskMentionNotificationsEnabled: true,
};

function notesNotification(createdTime: string): NotesMentionNotification {
  return {
    object: "mention_notification",
    id: "notification-a",
    source_type: "block",
    source_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    page_id: "11111111-1111-4111-8111-111111111111",
    page_title: "Inbox",
    block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    comment_id: null,
    kind: "user_mention",
    target_type: "user",
    target_id: "12121212-1212-4212-8212-121212121212",
    trigger_at: null,
    remind_minutes_before: 0,
    plain_text: "Mention",
    source_plain_text: "Mention",
    status: "pending",
    delivered_at: null,
    suppressed: false,
    created_time: createdTime,
    last_edited_time: createdTime,
  };
}

describe("notification deadline schedulers", () => {
  it("delivers one calendar notification at its exact deadline", async () => {
    const nowMs = new Date(2026, 6, 11, 12, 0, 0).getTime();
    const clock = new FakeSchedulerClock(nowMs);
    const event = calendarEvent("2026-07-11 12:00:05");
    const deliver = vi.fn(async () => undefined);
    const scheduler = createEventNotificationScheduler({
      clock,
      getEvents: () => [event],
      deliver,
    });

    scheduler.setEnabled(true);
    await settleScheduler();
    expect(deliver).not.toHaveBeenCalled();
    await clock.advanceTo(nowMs + 5_000);
    expect(deliver).toHaveBeenCalledOnce();

    scheduler.resume();
    await settleScheduler();
    expect(deliver).toHaveBeenCalledOnce();
  });

  it("deduplicates repeated deadlines and retries a failed delivery once", async () => {
    const nowMs = new Date(2026, 6, 11, 12, 0, 0).getTime();
    const clock = new FakeSchedulerClock(nowMs);
    const event = calendarEvent("2026-07-11 12:00:05");
    event.notifications = [0, 0];
    const deliver = vi.fn()
      .mockRejectedValueOnce(new Error("notification unavailable"))
      .mockResolvedValue(undefined);
    const scheduler = createEventNotificationScheduler({
      clock,
      getEvents: () => [event],
      deliver,
    });

    scheduler.setEnabled(true);
    await settleScheduler();
    await clock.advanceTo(nowMs + 5_000);
    expect(deliver).toHaveBeenCalledOnce();

    await clock.advanceTo(nowMs + 65_000);
    expect(deliver).toHaveBeenCalledTimes(2);
    scheduler.resume();
    await settleScheduler();
    expect(deliver).toHaveBeenCalledTimes(2);
  });

  it("catches up exactly once after a suspended calendar clock jump", async () => {
    const nowMs = new Date(2026, 6, 11, 12, 0, 0).getTime();
    const clock = new FakeSchedulerClock(nowMs);
    const deliver = vi.fn(async () => undefined);
    const scheduler = createEventNotificationScheduler({
      clock,
      getEvents: () => [calendarEvent("2026-07-11 12:00:05")],
      deliver,
    });
    scheduler.setEnabled(true);
    await settleScheduler();

    clock.jumpTo(nowMs + 30_000);
    scheduler.resume();
    await settleScheduler();
    scheduler.resume();
    await settleScheduler();

    expect(deliver).toHaveBeenCalledOnce();
  });

  it("schedules pending Notes rows and does not redeliver stale pending data", async () => {
    const nowMs = Date.parse("2026-07-11T12:00:00.000Z");
    const clock = new FakeSchedulerClock(nowMs);
    const notification = notesNotification("2026-07-11T12:00:05.000Z");
    const deliver = vi.fn(async () => undefined);
    const scheduler = createNotesNotificationScheduler({
      clock,
      listPending: async () => [notification],
      getPreferences: () => notesPreferences,
      deliver,
      dismiss: vi.fn(async () => undefined),
    });

    scheduler.setEnabled(true);
    await settleScheduler();
    expect(deliver).not.toHaveBeenCalled();
    await clock.advanceTo(nowMs + 5_000);
    expect(deliver).toHaveBeenCalledOnce();

    scheduler.resume();
    await settleScheduler();
    expect(deliver).toHaveBeenCalledOnce();
  });

  it("cancels Notes deadlines while notification preferences are disabled", async () => {
    const nowMs = Date.parse("2026-07-11T12:00:00.000Z");
    const clock = new FakeSchedulerClock(nowMs);
    const deliver = vi.fn(async () => undefined);
    const scheduler = createNotesNotificationScheduler({
      clock,
      listPending: async () => [notesNotification("2026-07-11T12:00:05.000Z")],
      getPreferences: () => notesPreferences,
      deliver,
      dismiss: vi.fn(async () => undefined),
    });

    scheduler.setEnabled(true);
    await settleScheduler();
    scheduler.setEnabled(false);
    await clock.advanceTo(nowMs + 10_000);

    expect(deliver).not.toHaveBeenCalled();
  });

  it("retires muted Notes rows at their deadline without showing them", async () => {
    const nowMs = Date.parse("2026-07-11T12:00:00.000Z");
    const clock = new FakeSchedulerClock(nowMs);
    const muted = { ...notesNotification("2026-07-11T12:00:05.000Z"), suppressed: true };
    const deliver = vi.fn(async () => undefined);
    const dismiss = vi.fn(async () => undefined);
    const scheduler = createNotesNotificationScheduler({
      clock,
      listPending: async () => [muted],
      getPreferences: () => ({ ...notesPreferences, mentionNotificationsEnabled: false }),
      deliver,
      dismiss,
    });

    scheduler.setEnabled(true);
    await settleScheduler();
    expect(dismiss).not.toHaveBeenCalled();
    await clock.advanceTo(nowMs + 5_000);

    expect(dismiss).toHaveBeenCalledExactlyOnceWith(muted);
    expect(deliver).not.toHaveBeenCalled();
  });
});
