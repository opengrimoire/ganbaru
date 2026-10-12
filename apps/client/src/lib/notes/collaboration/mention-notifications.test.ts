import { describe, expect, it } from "vitest";
import {
  notesMentionNotificationDueAtMs,
  notesMentionNotificationIsDue,
} from "./mention-notifications";
import type { NotesMentionNotification } from "$lib/notes/types";

const baseNotification: NotesMentionNotification = {
  object: "mention_notification",
  id: "notification-a",
  source_type: "block",
  source_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  page_id: "11111111-1111-4111-8111-111111111111",
  page_title: "Inbox",
  block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  comment_id: null,
  kind: "reminder",
  target_type: "date",
  target_id: null,
  trigger_at: "2026-07-04",
  remind_minutes_before: 0,
  plain_text: "Reminder",
  source_plain_text: "Reminder",
  status: "pending",
  delivered_at: null,
  suppressed: false,
  created_time: "2026-07-01T12:00:00.000Z",
  last_edited_time: "2026-07-01T12:00:00.000Z",
};

const enabledPreferences = {
  mentionNotificationsEnabled: true,
  reminderNotificationsEnabled: true,
  userMentionNotificationsEnabled: true,
  taskMentionNotificationsEnabled: true,
};

describe("notes mention notifications", () => {
  it("treats date-only reminders as local 9 AM reminders by default", () => {
    expect(notesMentionNotificationDueAtMs(baseNotification, { timeZone: "UTC" })).toBe(
      Date.parse("2026-07-04T09:00:00.000Z"),
    );
  });

  it("uses explicit date-time reminder instants when present", () => {
    const notification = {
      ...baseNotification,
      trigger_at: "2026-07-04T14:30:00.000Z",
    } satisfies NotesMentionNotification;

    expect(notesMentionNotificationDueAtMs(notification, { timeZone: "UTC" })).toBe(
      Date.parse("2026-07-04T14:30:00.000Z"),
    );
  });

  it("moves reminders earlier by their offset for timed and date-only dates", () => {
    const timed = {
      ...baseNotification,
      trigger_at: "2026-07-04T14:30",
      remind_minutes_before: 30,
    } satisfies NotesMentionNotification;
    expect(notesMentionNotificationDueAtMs(timed, { timeZone: "UTC" })).toBe(
      Date.parse("2026-07-04T14:00:00.000Z"),
    );
    const dayBefore = { ...baseNotification, remind_minutes_before: 1440 } satisfies NotesMentionNotification;
    expect(notesMentionNotificationDueAtMs(dayBefore, { timeZone: "UTC" })).toBe(
      Date.parse("2026-07-03T09:00:00.000Z"),
    );
    expect(notesMentionNotificationIsDue(dayBefore, enabledPreferences, {
      nowMs: Date.parse("2026-07-03T09:00:00.000Z"),
      timeZone: "UTC",
    })).toBe(true);
  });

  it("treats user mentions as due at their local notification row creation time", () => {
    const notification = {
      ...baseNotification,
      kind: "user_mention",
      target_type: "user",
      target_id: "12121212-1212-4212-8212-121212121212",
      trigger_at: null,
    } satisfies NotesMentionNotification;

    expect(notesMentionNotificationIsDue(notification, enabledPreferences, {
      nowMs: Date.parse("2026-07-01T12:00:00.000Z"),
      timeZone: "UTC",
    })).toBe(true);
  });

  it("respects the master and category preference gates", () => {
    expect(notesMentionNotificationIsDue(baseNotification, {
      ...enabledPreferences,
      mentionNotificationsEnabled: false,
    }, {
      nowMs: Date.parse("2026-07-05T00:00:00.000Z"),
      timeZone: "UTC",
    })).toBe(false);

    expect(notesMentionNotificationIsDue(baseNotification, {
      ...enabledPreferences,
      reminderNotificationsEnabled: false,
    }, {
      nowMs: Date.parse("2026-07-05T00:00:00.000Z"),
      timeZone: "UTC",
    })).toBe(false);
  });
});
