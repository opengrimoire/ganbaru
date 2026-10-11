import type { CalendarEvent } from "$lib/calendar/types";
import { parseCalendarDate } from "$lib/calendar/utils";
import {
  notesMentionNotificationDueAtMs,
  notesMentionNotificationIsDue,
  type NotesMentionNotificationPreferenceSnapshot,
} from "$lib/notes/collaboration/mention-notifications";
import type { NotesMentionNotification } from "$lib/notes/types";
import {
  createLifecycleScheduler,
  systemSchedulerClock,
  type LifecycleScheduler,
  type SchedulerClock,
} from "./lifecycle-scheduler";

const EVENT_STARTUP_CATCH_UP_MS = 2_500;
const NOTES_DELIVERY_RETRY_MS = 60_000;

export type NotificationSchedulerController = LifecycleScheduler;

export interface EventNotificationDelivery {
  readonly event: CalendarEvent;
  readonly minutesBefore: number;
  readonly deadlineMs: number;
}

interface EventNotificationSchedulerOptions {
  readonly clock?: SchedulerClock;
  readonly getEvents: () => readonly CalendarEvent[];
  readonly deliver: (delivery: EventNotificationDelivery) => void | Promise<void>;
  readonly onError?: (error: unknown) => void;
}

function eventNotificationKey(event: CalendarEvent, minutesBefore: number): string {
  return `${event.id}::${event.start}::${minutesBefore}`;
}

/** Schedules calendar notifications at their exact next in-memory deadline. */
export function createEventNotificationScheduler(
  options: EventNotificationSchedulerOptions,
): NotificationSchedulerController {
  const clock = options.clock ?? systemSchedulerClock;
  const delivered = new Set<string>();
  const retry = new Set<string>();
  let lastCheckMs: number | null = null;

  return createLifecycleScheduler({
    clock,
    onError: options.onError,
    errorRetryMs: 60_000,
    run: async (context) => {
      const nowMs = clock.now();
      const previousCheckMs = lastCheckMs === null
        ? nowMs - EVENT_STARTUP_CATCH_UP_MS
        : Math.min(lastCheckMs, nowMs);
      lastCheckMs = nowMs;
      let nextDeadlineMs: number | null = null;
      const due: EventNotificationDelivery[] = [];
      const dueKeys = new Set<string>();

      for (const event of options.getEvents()) {
        if (!event.notifications || event.notifications.length === 0) continue;
        const startMs = parseCalendarDate(event.start).getTime();
        if (!Number.isFinite(startMs)) continue;
        for (const minutesBefore of event.notifications) {
          const key = eventNotificationKey(event, minutesBefore);
          if (delivered.has(key) || dueKeys.has(key)) continue;
          const deadlineMs = startMs - minutesBefore * 60_000;
          if (deadlineMs <= nowMs && (deadlineMs > previousCheckMs || retry.has(key))) {
            dueKeys.add(key);
            due.push({ event, minutesBefore, deadlineMs });
          } else if (deadlineMs > nowMs) {
            nextDeadlineMs = nextDeadlineMs === null
              ? deadlineMs
              : Math.min(nextDeadlineMs, deadlineMs);
          }
        }
      }

      for (const delivery of due) {
        if (!context.isCurrent()) return null;
        const key = eventNotificationKey(delivery.event, delivery.minutesBefore);
        try {
          await options.deliver(delivery);
          delivered.add(key);
          retry.delete(key);
        } catch (error) {
          retry.add(key);
          throw error;
        }
      }
      return nextDeadlineMs;
    },
  });
}

interface NotesNotificationSchedulerOptions {
  readonly clock?: SchedulerClock;
  readonly listPending: () => Promise<readonly NotesMentionNotification[]>;
  readonly getPreferences: () => NotesMentionNotificationPreferenceSnapshot;
  readonly deliver: (notification: NotesMentionNotification) => Promise<void>;
  /** Retires a muted notification without showing it. */
  readonly dismiss: (notification: NotesMentionNotification) => Promise<void>;
  readonly onError?: (error: unknown) => void;
  readonly onDeliveryError?: (error: unknown) => void;
}

/** Schedules the next pending Notes notification without a polling interval. */
export function createNotesNotificationScheduler(
  options: NotesNotificationSchedulerOptions,
): NotificationSchedulerController {
  const clock = options.clock ?? systemSchedulerClock;
  const delivered = new Set<string>();

  return createLifecycleScheduler({
    clock,
    onError: options.onError,
    errorRetryMs: NOTES_DELIVERY_RETRY_MS,
    run: async (context) => {
      const pending = await options.listPending();
      if (!context.isCurrent()) return null;
      const pendingIds = new Set(pending.map((notification) => notification.id));
      for (const id of delivered) {
        if (!pendingIds.has(id)) delivered.delete(id);
      }

      const nowMs = clock.now();
      const preferences = options.getPreferences();
      let nextDeadlineMs: number | null = null;
      let retryRequired = false;

      for (const notification of pending) {
        if (delivered.has(notification.id)) continue;
        const dueAtMs = notesMentionNotificationDueAtMs(notification);
        if (dueAtMs === null) continue;
        // Muted pages and discussions retire their notifications when due, so unmuting later cannot replay them.
        const isDueAt = (atMs: number) => notification.suppressed
          ? dueAtMs <= atMs
          : notesMentionNotificationIsDue(notification, preferences, { nowMs: atMs });
        if (isDueAt(nowMs)) {
          if (!context.isCurrent()) return null;
          delivered.add(notification.id);
          try {
            await (notification.suppressed ? options.dismiss : options.deliver)(notification);
          } catch (error) {
            delivered.delete(notification.id);
            retryRequired = true;
            options.onDeliveryError?.(error);
          }
        } else if (dueAtMs > nowMs) {
          const enabledAtDeadline = isDueAt(dueAtMs);
          if (enabledAtDeadline) {
            nextDeadlineMs = nextDeadlineMs === null
              ? dueAtMs
              : Math.min(nextDeadlineMs, dueAtMs);
          }
        }
      }

      if (retryRequired) {
        const retryAtMs = nowMs + NOTES_DELIVERY_RETRY_MS;
        nextDeadlineMs = nextDeadlineMs === null
          ? retryAtMs
          : Math.min(nextDeadlineMs, retryAtMs);
      }
      return nextDeadlineMs;
    },
  });
}
