import type { AppLocale } from "$lib/i18n/locales";
import type { Translate } from "$lib/i18n/translator.svelte";
import { formatDateTime, formatList, formatNumber } from "$lib/i18n/formatters";
import type { NotesDataSourceNumberFormat, NotesPage } from "$lib/notes/types";
import { notesDatabaseDateBoundaryOrder, notesDatabaseIsDateBoundaryValid, notesDatabaseDateValue, notesDatabaseIsTimeZoneValid, type NotesDatabaseDateValue } from "./date";
import { notesDatabaseTableCellText, type NotesDatabaseTableColumn } from "./table";

export type NotesDatabaseDateFormat = "locale" | "iso" | "relative";
export type NotesDatabaseTimeFormat = "locale" | "12_hour" | "24_hour" | "hidden";
export interface NotesDatabasePropertyDisplayFormat {
  wrap?: boolean;
  date_format?: NotesDatabaseDateFormat;
  time_format?: NotesDatabaseTimeFormat;
}
export interface NotesDatabasePropertyDisplayContext {
  locale: AppLocale;
  t: Translate;
  now?: Date;
}
export interface NotesDatabaseFileDisplay {
  name: string;
  url: string | null;
}

const CURRENCIES: Partial<Record<NotesDataSourceNumberFormat, string>> = {
  dollar: "USD", euro: "EUR", pound: "GBP", yen: "JPY", yuan: "CNY", won: "KRW", ruble: "RUB",
  rupee: "INR", franc: "CHF", real: "BRL", lira: "TRY", krona: "SEK", ringgit: "MYR",
};
const MILLISECONDS_PER_DAY = 24 * 60 * 60 * 1000;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Obtain the canonical property payload by its stable identity and type. */
export function notesDatabasePropertyPayload(row: NotesPage, column: NotesDatabaseTableColumn): unknown {
  const property = Object.values(row.properties).find((value) => isRecord(value) && value.id === column.id && value.type === column.type);
  return isRecord(property) ? property[column.type] : null;
}

/** Read safe file labels and usable URLs while retaining non-linkable files in display. */
export function notesDatabasePropertyFiles(row: NotesPage, column: NotesDatabaseTableColumn): NotesDatabaseFileDisplay[] {
  const payload = notesDatabasePropertyPayload(row, column);
  if (!Array.isArray(payload)) return [];
  return payload.filter(isRecord).map((file) => {
    const source = isRecord(file.external) ? file.external : isRecord(file.file) ? file.file : null;
    const candidate = source && typeof source.url === "string" ? source.url : null;
    const url = candidate && /^(?:https?:|asset:|tauri:)/i.test(candidate) ? candidate : null;
    return { name: typeof file.name === "string" && file.name ? file.name : candidate ?? "", url };
  }).filter((file) => file.name || file.url);
}

/** Apply a property's durable numeric format while retaining its canonical scalar for editing. */
export function notesDatabaseNumberDisplay(locale: AppLocale, value: number, format: NotesDataSourceNumberFormat = "number"): string {
  const currency = CURRENCIES[format];
  if (currency) return formatNumber(locale, value, { style: "currency", currency });
  if (format === "percent") return formatNumber(locale, value, { style: "percent", maximumFractionDigits: 6 });
  return formatNumber(locale, value, { useGrouping: format === "number_with_commas", maximumFractionDigits: 20 });
}

function dateDisplayBoundary(value: string, date: NotesDatabaseDateValue, context: NotesDatabasePropertyDisplayContext, format: NotesDatabasePropertyDisplayFormat): string {
  if (!notesDatabaseIsDateBoundaryValid(value)) return value;
  const hasTime = value.includes("T");
  const isWallTime = !hasTime || !/(?:Z|[+-]\d{2}:\d{2})$/.test(value);
  const timeZone = isWallTime ? "UTC" : date.time_zone && notesDatabaseIsTimeZoneValid(date.time_zone) ? date.time_zone : undefined;
  const instant = new Date(notesDatabaseDateBoundaryOrder(value));
  const dateOptions: Intl.DateTimeFormatOptions = { year: "numeric", month: "short", day: "numeric", timeZone };
  let label: string;
  if (format.date_format === "iso") {
    const parts = new Intl.DateTimeFormat("en-CA", { year: "numeric", month: "2-digit", day: "2-digit", timeZone }).formatToParts(instant);
    label = ["year", "month", "day"].map((type) => parts.find((part) => part.type === type)?.value ?? "").join("-");
  } else if (format.date_format === "relative") {
    const now = context.now ?? new Date();
    const relativeZone = date.time_zone && notesDatabaseIsTimeZoneValid(date.time_zone) ? date.time_zone : undefined;
    const dayNumber = (value: Date, zone: string | undefined): number => {
      const parts = new Intl.DateTimeFormat("en", { year: "numeric", month: "numeric", day: "numeric", timeZone: zone }).formatToParts(value);
      const field = (type: string): number => Number(parts.find((part) => part.type === type)?.value);
      return Date.UTC(field("year"), field("month") - 1, field("day")) / MILLISECONDS_PER_DAY;
    };
    label = new Intl.RelativeTimeFormat(context.locale, { numeric: "auto" }).format(dayNumber(instant, timeZone) - dayNumber(now, relativeZone), "day");
  } else label = formatDateTime(context.locale, instant, dateOptions);
  if (hasTime && format.time_format !== "hidden") {
    const time = formatDateTime(context.locale, instant, {
      hour: "numeric", minute: "2-digit", timeZone,
      ...(format.time_format === "12_hour" ? { hour12: true } : format.time_format === "24_hour" ? { hour12: false } : {}),
    });
    label = `${label}, ${time}`;
  }
  return label;
}

/** Display a complete date range in the current locale and chosen per-view format. */
export function notesDatabaseDateDisplay(date: NotesDatabaseDateValue | null, context: NotesDatabasePropertyDisplayContext, format: NotesDatabasePropertyDisplayFormat = {}): string {
  if (!date) return "";
  const start = dateDisplayBoundary(date.start, date, context, format);
  const end = date.end ? dateDisplayBoundary(date.end, date, context, format) : null;
  return end && end !== start ? context.t("notes.databaseDisplayRange", start, end) : start;
}

/** Format scalar, computed, and read-only canonical properties without changing editable values. */
export function notesDatabasePropertyDisplayText(row: NotesPage, column: NotesDatabaseTableColumn, context: NotesDatabasePropertyDisplayContext, format: NotesDatabasePropertyDisplayFormat = column.displayFormat ?? {}): string {
  const payload = notesDatabasePropertyPayload(row, column);
  switch (column.type) {
    case "number": return typeof payload === "number" && Number.isFinite(payload) ? notesDatabaseNumberDisplay(context.locale, payload, column.numberFormat) : "";
    case "date": return notesDatabaseDateDisplay(notesDatabaseDateValue(payload), context, format);
    case "created_time":
    case "last_edited_time": {
      const value = typeof payload === "string" ? payload : row[column.type];
      return notesDatabaseDateDisplay({ start: value, end: null, time_zone: null }, context, format);
    }
    case "created_by":
    case "last_edited_by": return isRecord(payload) ? typeof payload.name === "string" && payload.name ? payload.name : typeof payload.id === "string" ? payload.id : "" : "";
    case "people": return Array.isArray(payload) ? formatList(context.locale, payload.filter(isRecord).flatMap((person) => {
      const name = typeof person.name === "string" && person.name ? person.name : typeof person.id === "string" ? person.id : "";
      return name ? [name] : [];
    })) : "";
    case "files": return formatList(context.locale, notesDatabasePropertyFiles(row, column).map((file) => file.name));
    case "formula":
    case "rollup": {
      if (isRecord(payload) && payload.type === "number" && typeof payload.number === "number") return notesDatabaseNumberDisplay(context.locale, payload.number);
      if (isRecord(payload) && payload.type === "date") return notesDatabaseDateDisplay(notesDatabaseDateValue(payload.date), context, format);
      return notesDatabaseTableCellText(row, column);
    }
    default: return notesDatabaseTableCellText(row, column);
  }
}
