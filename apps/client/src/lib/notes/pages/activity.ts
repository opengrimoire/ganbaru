export interface NotesActivityTimeLabels {
  justNow: string;
  minutesAgo: (minutes: number) => string;
  hoursAgo: (hours: number) => string;
}

export interface NotesActivityTimeFormatOptions {
  locale: string;
  now: Date;
  labels: NotesActivityTimeLabels;
}

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

function parseActivityDate(value: string | Date): Date | null {
  const date = value instanceof Date ? value : new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

export function formatNotesActivityDate(
  value: string | Date,
  locale: string,
  now: Date,
): string {
  const date = parseActivityDate(value);
  if (!date) return "";
  return new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "numeric",
    ...(date.getFullYear() === now.getFullYear() ? {} : { year: "numeric" }),
  }).format(date);
}

export function formatNotesActivityTime(
  value: string | Date,
  { locale, now, labels }: NotesActivityTimeFormatOptions,
): string {
  const date = parseActivityDate(value);
  if (!date) return "";

  const elapsedMs = now.getTime() - date.getTime();
  if (elapsedMs < MINUTE_MS) return labels.justNow;
  if (elapsedMs < HOUR_MS) {
    return labels.minutesAgo(Math.max(1, Math.floor(elapsedMs / MINUTE_MS)));
  }
  if (elapsedMs < DAY_MS) {
    return labels.hoursAgo(Math.max(1, Math.floor(elapsedMs / HOUR_MS)));
  }
  return formatNotesActivityDate(date, locale, now);
}
