export const NOTES_HISTORY_RETENTION_OPTIONS = [0, 7, 30, 90, 180, 365] as const;

export type NotesHistoryRetentionDays =
  (typeof NOTES_HISTORY_RETENTION_OPTIONS)[number];

export const DEFAULT_NOTES_HISTORY_RETENTION_DAYS: NotesHistoryRetentionDays = 30;

export function isNotesHistoryRetentionDays(
  value: unknown,
): value is NotesHistoryRetentionDays {
  return typeof value === "number"
    && NOTES_HISTORY_RETENTION_OPTIONS.some((candidate) => candidate === value);
}

export function effectiveNotesHistoryRetentionDays(
  globalDays: NotesHistoryRetentionDays,
  projectDays: NotesHistoryRetentionDays | null | undefined,
): NotesHistoryRetentionDays {
  return projectDays ?? globalDays;
}

export function isShorterNotesHistoryRetention(
  currentDays: NotesHistoryRetentionDays,
  nextDays: NotesHistoryRetentionDays,
): boolean {
  return nextDays < currentDays;
}
