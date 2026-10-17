import {
  flushDueNotesProjectHistory,
  type NotesProjectHistorySchedule,
} from "$lib/api/notes/project-history";
import {
  createLifecycleScheduler,
  systemSchedulerClock,
  type LifecycleScheduler,
  type SchedulerClock,
} from "$lib/scheduling/lifecycle-scheduler";

const ERROR_RETRY_MS = 60_000;

export interface NotesProjectHistoryScheduler {
  setEnabled(enabled: boolean): void;
  applyMutationDeadline(deadline: string | null): void;
  resume(): void;
  switchVault(): void;
  shutdown(): Promise<void>;
  dispose(): void;
  hasScheduledDeadline(): boolean;
}

export interface NotesProjectHistorySchedulerOptions {
  clock?: SchedulerClock;
  flush?: () => Promise<NotesProjectHistorySchedule>;
  onError?: (error: unknown) => void;
}

function deadlineMs(value: string | null): number | null {
  if (!value) return null;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function nextScheduleDeadline(schedule: NotesProjectHistorySchedule): number | null {
  const checkpoint = deadlineMs(schedule.nextCheckpointAt);
  const maintenance = deadlineMs(schedule.nextMaintenanceAt);
  if (checkpoint === null) return maintenance;
  if (maintenance === null) return checkpoint;
  return Math.min(checkpoint, maintenance);
}

/** Schedule Notes history only at dirty or bounded maintenance deadlines. */
export function createNotesProjectHistoryScheduler(
  options: NotesProjectHistorySchedulerOptions = {},
): NotesProjectHistoryScheduler {
  const clock = options.clock ?? systemSchedulerClock;
  const flush = options.flush ?? flushDueNotesProjectHistory;
  let lifecycle: LifecycleScheduler;

  lifecycle = createLifecycleScheduler({
    clock,
    errorRetryMs: ERROR_RETRY_MS,
    onError: options.onError,
    run: async () => {
      const schedule = await flush();
      return nextScheduleDeadline(schedule);
    },
  });

  function applyMutationDeadline(deadline: string | null): void {
    const authoritativeDeadline = deadlineMs(deadline);
    if (authoritativeDeadline === null) return;
    lifecycle.scheduleAt(authoritativeDeadline);
  }

  function switchVault(): void {
    lifecycle.resume();
  }

  async function shutdown(): Promise<void> {
    if (!lifecycle.isEnabled()) return;
    lifecycle.dispose();
    await flush();
  }

  return {
    setEnabled: lifecycle.setEnabled,
    applyMutationDeadline,
    resume: lifecycle.resume,
    switchVault,
    shutdown,
    dispose: lifecycle.dispose,
    hasScheduledDeadline: lifecycle.hasScheduledDeadline,
  };
}

const sharedNotesProjectHistoryScheduler = createNotesProjectHistoryScheduler({
  onError: (error) => console.error("Notes project history scheduler failed", error),
});

export function getNotesProjectHistoryScheduler(): NotesProjectHistoryScheduler {
  return sharedNotesProjectHistoryScheduler;
}

export function applyNotesProjectHistoryMutationDeadline(
  deadline: string | null,
): void {
  sharedNotesProjectHistoryScheduler.applyMutationDeadline(deadline);
}
