let version = $state(0);

/** Signals that a Notes write may have changed pending notification rows. */
export function invalidateNotesNotificationSchedule(): void {
  version += 1;
}

/** Exposes the schedule version without importing the full Notes store. */
export function getNotesNotificationSchedule() {
  return {
    get version(): number {
      return version;
    },
  };
}
