import type { Calendar } from "$lib/calendar/types";

const EMAIL_PATTERN = /^[^\s@<>()[\],;:]+@[^\s@<>()[\],;:]+\.[^\s@<>()[\],;:]+$/i;
const DATE_ONLY_PATTERN = /^(\d{4})-(\d{2})-(\d{2})$/;
function importDateFormatter(locale?: string | readonly string[]): Intl.DateTimeFormat {
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

function importDateTimeFormatter(locale?: string | readonly string[]): Intl.DateTimeFormat {
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

function stripIcsExtension(value: string): string {
  return value.replace(/\.ics$/i, "");
}

function basename(value: string): string {
  const parts = value.split(/[\\/]/);
  return parts[parts.length - 1] ?? value;
}

function sourceName(calendar: Calendar): string | undefined {
  if (!calendar.sourceUrl) return undefined;
  return stripIcsExtension(basename(calendar.sourceUrl));
}

function validDate(value: Date): Date | undefined {
  return Number.isNaN(value.getTime()) ? undefined : value;
}

function localDateOnly(value: string): Date | undefined {
  const match = DATE_ONLY_PATTERN.exec(value);
  if (!match) return undefined;

  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  return validDate(new Date(year, month - 1, day, 12));
}

function parseImportTimestamp(value: string): Date | undefined {
  const trimmed = value.trim();
  if (!trimmed) return undefined;

  const dateOnly = localDateOnly(trimmed);
  if (dateOnly) return dateOnly;

  return validDate(new Date(trimmed)) ?? validDate(new Date(trimmed.replace(" ", "T")));
}

function formatImportDateOnly(value: string, locale?: string | readonly string[]): string | undefined {
  const date = localDateOnly(value);
  return date ? importDateFormatter(locale).format(date) : undefined;
}

function formatImportTimestamp(value: string, locale?: string | readonly string[]): string | undefined {
  const trimmed = value.trim();
  const dateOnly = formatImportDateOnly(trimmed, locale);
  if (dateOnly) return dateOnly;

  const date = parseImportTimestamp(value);
  if (!date) return undefined;

  return importDateTimeFormatter(locale).format(date);
}

export function calendarDisplayName(calendar: Calendar): string {
  if (calendar.source === "ics") {
    return sourceName(calendar) ?? calendar.name;
  }
  return calendar.name;
}

export function calendarImportDate(
  calendar: Calendar,
  locale?: string | readonly string[],
): string | undefined {
  if (calendar.source !== "ics") return undefined;
  return calendar.createdAt ? formatImportTimestamp(calendar.createdAt, locale) : undefined;
}

export function calendarIdentityEmail(calendar: Calendar | undefined): string | undefined {
  if (!calendar || calendar.source !== "ics") return undefined;

  const candidates = [
    sourceName(calendar),
    calendar.name,
  ];

  for (const candidate of candidates) {
    const trimmed = candidate?.trim();
    if (trimmed && EMAIL_PATTERN.test(trimmed)) return trimmed;
  }

  return undefined;
}
