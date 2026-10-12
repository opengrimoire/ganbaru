let version = $state(0);

/** Signals that a Projects write may have changed upcoming task date reminders. */
export function invalidateProjectTaskReminderSchedule(): void {
  version += 1;
}

/** Exposes the schedule version without importing the full Projects store. */
export function getProjectTaskReminderSchedule() {
  return {
    get version(): number {
      return version;
    },
  };
}
