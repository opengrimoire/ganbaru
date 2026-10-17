import type {
  OrdinalWeekday,
  RecurrenceConfig,
  RecurrenceFrequency,
  Weekday,
} from "./types";
import type { Translate } from "$lib/i18n/translator.svelte";

const ALL_WEEKDAYS: Weekday[] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

const FREQ_MAP: Record<string, RecurrenceFrequency> = {
  DAILY: "daily",
  WEEKLY: "weekly",
  MONTHLY: "monthly",
  YEARLY: "yearly",
};

const FREQ_REVERSE: Record<RecurrenceFrequency, string> = {
  daily: "DAILY",
  weekly: "WEEKLY",
  monthly: "MONTHLY",
  yearly: "YEARLY",
};

const WEEKDAY_LABELS: Record<Weekday, string> = {
  MO: "Mon",
  TU: "Tue",
  WE: "Wed",
  TH: "Thu",
  FR: "Fri",
  SA: "Sat",
  SU: "Sun",
};

const FREQ_UNIT_PLURAL: Record<RecurrenceFrequency, string> = {
  daily: "days",
  weekly: "weeks",
  monthly: "months",
  yearly: "years",
};

const WEEKDAY_LABEL_DATES: Record<Weekday, Date> = {
  MO: new Date(2024, 0, 1),
  TU: new Date(2024, 0, 2),
  WE: new Date(2024, 0, 3),
  TH: new Date(2024, 0, 4),
  FR: new Date(2024, 0, 5),
  SA: new Date(2024, 0, 6),
  SU: new Date(2024, 0, 7),
};

interface RecurrenceLabelOptions {
  readonly t?: Translate;
  readonly locale?: string | readonly string[];
}

const BYDAY_RE = /^(-?\d+)?([A-Z]{2})$/;
const WEEKDAY_ORDER = new Map<Weekday, number>(
  ALL_WEEKDAYS.map((day, index) => [day, index]),
);

function compareWeekdays(a: Weekday, b: Weekday): number {
  return (WEEKDAY_ORDER.get(a) ?? 0) - (WEEKDAY_ORDER.get(b) ?? 0);
}

function uniqueSortedNumbers(values: number[] | undefined): number[] | undefined {
  if (!values || values.length === 0) return undefined;
  return [...new Set(values)].sort((a, b) => a - b);
}

function uniqueSortedWeekdays(values: Weekday[] | undefined): Weekday[] | undefined {
  if (!values || values.length === 0) return undefined;
  return [...new Set(values)].sort(compareWeekdays);
}

function ordinalWeekdayKey(value: OrdinalWeekday): string {
  return `${value.ordinal ?? ""}:${value.day}`;
}

function uniqueSortedOrdinalWeekdays(
  values: OrdinalWeekday[] | undefined,
): OrdinalWeekday[] | undefined {
  if (!values || values.length === 0) return undefined;
  const deduped = new Map<string, OrdinalWeekday>();
  for (const value of values) {
    deduped.set(ordinalWeekdayKey(value), value.ordinal == null
      ? { day: value.day }
      : { day: value.day, ordinal: value.ordinal });
  }
  const result = [...deduped.values()].sort((a, b) => {
    const weekdayDiff = compareWeekdays(a.day, b.day);
    if (weekdayDiff !== 0) return weekdayDiff;
    return (a.ordinal ?? 0) - (b.ordinal ?? 0);
  });
  return result.length > 0 ? result : undefined;
}

function canonicalizeRecurrenceConfig(config: RecurrenceConfig): RecurrenceConfig {
  const ordinalWeekdays = uniqueSortedOrdinalWeekdays(config.ordinalWeekdays);
  return {
    frequency: config.frequency,
    interval: config.interval,
    end: config.end,
    weekdays: ordinalWeekdays ? undefined : uniqueSortedWeekdays(config.weekdays),
    ordinalWeekdays,
    byMonthDay: uniqueSortedNumbers(config.byMonthDay),
    byMonth: uniqueSortedNumbers(config.byMonth),
    bySetPos: uniqueSortedNumbers(config.bySetPos),
    byYearDay: uniqueSortedNumbers(config.byYearDay),
    byWeekNo: uniqueSortedNumbers(config.byWeekNo),
    wkst: config.wkst && config.wkst !== "MO" ? config.wkst : undefined,
  };
}

export function recurrenceConfigsEqual(
  a: RecurrenceConfig | undefined,
  b: RecurrenceConfig | undefined,
): boolean {
  if (a === b) return true;
  if (a === undefined && b === undefined) return true;
  if (!a || !b) return false;
  return JSON.stringify(canonicalizeRecurrenceConfig(a))
    === JSON.stringify(canonicalizeRecurrenceConfig(b));
}

/**
 * Parse a single BYDAY token like "MO", "2TU", or "-1FR".
 * Returns null if the token is not a valid weekday.
 */
function parseBydayToken(token: string): { day: Weekday; ordinal?: number } | null {
  const match = token.match(BYDAY_RE);
  if (!match) return null;
  const day = match[2] as Weekday;
  if (!ALL_WEEKDAYS.includes(day)) return null;
  const ordinal = match[1] ? parseInt(match[1], 10) : undefined;
  return { day, ordinal };
}

/**
 * Parse an UNTIL value which may be date-only (YYYYMMDD) or datetime (YYYYMMDDTHHMMSSZ).
 */
function parseUntil(raw: string): string {
  if (raw.length === 8) {
    // YYYYMMDD -> YYYY-MM-DD
    return `${raw.slice(0, 4)}-${raw.slice(4, 6)}-${raw.slice(6, 8)}`;
  }
  if (raw.includes("T")) {
    // YYYYMMDDTHHMMSSZ -> YYYY-MM-DDTHH:MM:SSZ
    const date = `${raw.slice(0, 4)}-${raw.slice(4, 6)}-${raw.slice(6, 8)}`;
    const time = raw.slice(9).replace("Z", "");
    const h = time.slice(0, 2);
    const m = time.slice(2, 4);
    const s = time.slice(4, 6);
    return `${date}T${h}:${m}:${s}Z`;
  }
  return raw;
}

/**
 * Serialize an until date string back to RRULE UNTIL format.
 */
function serializeUntil(date: string): string {
  if (date.includes("T")) {
    // YYYY-MM-DDTHH:MM:SSZ -> YYYYMMDDTHHMMSSZ
    return date.replace(/-/g, "").replace(/:/g, "");
  }
  // YYYY-MM-DD -> YYYYMMDD
  return date.replace(/-/g, "");
}

/**
 * Parse a comma-separated list of integers.
 */
function parseIntList(raw: string): number[] {
  return raw.split(",").map((s) => parseInt(s.trim(), 10)).filter((n) => !isNaN(n));
}

// Serializer

export function recurrenceToRrule(config: RecurrenceConfig): string {
  const parts: string[] = [`FREQ=${FREQ_REVERSE[config.frequency]}`];

  if (config.interval > 1) {
    parts.push(`INTERVAL=${config.interval}`);
  }

  // BYDAY: ordinalWeekdays takes precedence over simple weekdays
  if (config.ordinalWeekdays && config.ordinalWeekdays.length > 0) {
    const tokens = config.ordinalWeekdays.map((ow) =>
      ow.ordinal != null ? `${ow.ordinal}${ow.day}` : ow.day,
    );
    parts.push(`BYDAY=${tokens.join(",")}`);
  } else if (config.weekdays && config.weekdays.length > 0) {
    parts.push(`BYDAY=${config.weekdays.join(",")}`);
  }

  if (config.byMonthDay && config.byMonthDay.length > 0) {
    parts.push(`BYMONTHDAY=${config.byMonthDay.join(",")}`);
  }

  if (config.byMonth && config.byMonth.length > 0) {
    parts.push(`BYMONTH=${config.byMonth.join(",")}`);
  }

  if (config.bySetPos && config.bySetPos.length > 0) {
    parts.push(`BYSETPOS=${config.bySetPos.join(",")}`);
  }

  if (config.byYearDay && config.byYearDay.length > 0) {
    parts.push(`BYYEARDAY=${config.byYearDay.join(",")}`);
  }

  if (config.byWeekNo && config.byWeekNo.length > 0) {
    parts.push(`BYWEEKNO=${config.byWeekNo.join(",")}`);
  }

  if (config.wkst && config.wkst !== "MO") {
    parts.push(`WKST=${config.wkst}`);
  }

  if (config.end.type === "count") {
    parts.push(`COUNT=${config.end.count}`);
  } else if (config.end.type === "until") {
    parts.push(`UNTIL=${serializeUntil(config.end.date)}`);
  }

  return parts.join(";");
}

// Parser

export function rruleToRecurrence(
  rrule: string,
  repeatUntil?: string,
): RecurrenceConfig {
  const params = new Map<string, string>();
  for (const part of rrule.split(";")) {
    const eq = part.indexOf("=");
    if (eq !== -1) {
      params.set(part.slice(0, eq), part.slice(eq + 1));
    }
  }

  const freqStr = params.get("FREQ") ?? "DAILY";
  const frequency: RecurrenceFrequency = FREQ_MAP[freqStr] ?? "daily";

  const intervalStr = params.get("INTERVAL");
  const interval = intervalStr ? parseInt(intervalStr, 10) : 1;

  // Parse BYDAY, distinguishing simple weekdays from ordinal weekdays
  const byday = params.get("BYDAY");
  let weekdays: Weekday[] | undefined;
  let ordinalWeekdays: OrdinalWeekday[] | undefined;
  if (byday) {
    const simple: Weekday[] = [];
    const ordinal: OrdinalWeekday[] = [];
    for (const token of byday.split(",")) {
      const parsed = parseBydayToken(token.trim());
      if (!parsed) continue;
      if (parsed.ordinal != null) {
        ordinal.push({ day: parsed.day, ordinal: parsed.ordinal });
      } else {
        simple.push(parsed.day);
      }
    }
    if (ordinal.length > 0) {
      ordinalWeekdays = ordinal;
      // If there are also simple entries, include them as ordinalWeekdays without ordinal
      if (simple.length > 0) {
        ordinalWeekdays = [...ordinal, ...simple.map((d) => ({ day: d }))];
      }
    } else if (simple.length > 0) {
      weekdays = simple;
    }
  }

  // Parse additional BY* rules
  const byMonthDayStr = params.get("BYMONTHDAY");
  const byMonthDay = byMonthDayStr ? parseIntList(byMonthDayStr) : undefined;

  const byMonthStr = params.get("BYMONTH");
  const byMonth = byMonthStr ? parseIntList(byMonthStr) : undefined;

  const bySetPosStr = params.get("BYSETPOS");
  const bySetPos = bySetPosStr ? parseIntList(bySetPosStr) : undefined;

  const byYearDayStr = params.get("BYYEARDAY");
  const byYearDay = byYearDayStr ? parseIntList(byYearDayStr) : undefined;

  const byWeekNoStr = params.get("BYWEEKNO");
  const byWeekNo = byWeekNoStr ? parseIntList(byWeekNoStr) : undefined;

  const wkstStr = params.get("WKST");
  const wkst: Weekday | undefined =
    wkstStr && ALL_WEEKDAYS.includes(wkstStr as Weekday) ? (wkstStr as Weekday) : undefined;

  // Parse end condition
  const countStr = params.get("COUNT");
  const untilStr = params.get("UNTIL");

  let end: RecurrenceConfig["end"];
  if (countStr) {
    end = { type: "count", count: parseInt(countStr, 10) };
  } else if (untilStr) {
    end = { type: "until", date: parseUntil(untilStr) };
  } else if (repeatUntil) {
    end = { type: "until", date: repeatUntil };
  } else {
    end = { type: "never" };
  }

  const config: RecurrenceConfig = { frequency, interval, end };
  if (weekdays?.length) config.weekdays = weekdays;
  if (ordinalWeekdays?.length) config.ordinalWeekdays = ordinalWeekdays;
  if (byMonthDay?.length) config.byMonthDay = byMonthDay;
  if (byMonth?.length) config.byMonth = byMonth;
  if (bySetPos?.length) config.bySetPos = bySetPos;
  if (byYearDay?.length) config.byYearDay = byYearDay;
  if (byWeekNo?.length) config.byWeekNo = byWeekNo;
  if (wkst) config.wkst = wkst;
  return config;
}

function formatMonthDay(
  dateStr: string,
  locale: string | readonly string[] = "en-US",
): string {
  const datePart = dateStr.split("T")[0];
  const [y, m, d] = datePart.split("-").map(Number);
  const date = new Date(y, m - 1, d);
  return date.toLocaleDateString(locale, { month: "short", day: "numeric" });
}

const ORDINAL_SUFFIXES = ["th", "st", "nd", "rd"];

function ordinalSuffix(n: number): string {
  const abs = Math.abs(n);
  if (abs === 0) return "th";
  if (n < 0) return "last";
  const mod100 = abs % 100;
  if (mod100 >= 11 && mod100 <= 13) return `${abs}th`;
  const mod10 = abs % 10;
  return `${abs}${ORDINAL_SUFFIXES[mod10] ?? "th"}`;
}

function frequencyLabel(
  frequency: RecurrenceFrequency,
  t: Translate | undefined,
): string {
  if (!t) {
    switch (frequency) {
      case "daily":
        return "Daily";
      case "weekly":
        return "Weekly";
      case "monthly":
        return "Monthly";
      case "yearly":
        return "Yearly";
    }
  }

  switch (frequency) {
    case "daily":
      return t("calendar.recurrence.daily");
    case "weekly":
      return t("calendar.recurrence.weekly");
    case "monthly":
      return t("calendar.recurrence.monthly");
    case "yearly":
      return t("calendar.recurrence.yearly");
  }
}

function everyLabel(
  frequency: RecurrenceFrequency,
  interval: number,
  t: Translate | undefined,
): string {
  if (!t) return `Every ${interval} ${FREQ_UNIT_PLURAL[frequency]}`;
  switch (frequency) {
    case "daily":
      return t("calendar.recurrence.everyDays", interval);
    case "weekly":
      return t("calendar.recurrence.everyWeeks", interval);
    case "monthly":
      return t("calendar.recurrence.everyMonths", interval);
    case "yearly":
      return t("calendar.recurrence.everyYears", interval);
  }
}

function recurrenceOrdinal(value: number, t: Translate | undefined): string {
  return t ? t("calendar.recurrence.ordinal", value) : ordinalSuffix(value);
}

function lastLabel(t: Translate | undefined): string {
  return t ? t("calendar.recurrence.last") : "last";
}

function lastOccurrenceLabel(t: Translate | undefined): string {
  return t ? t("calendar.recurrence.lastOccurrence") : "last";
}

function weekdayLabel(
  weekday: Weekday,
  locale: string | readonly string[],
  localized: boolean,
): string {
  if (!localized) return WEEKDAY_LABELS[weekday];
  return new Intl.DateTimeFormat(locale, { weekday: "short" }).format(
    WEEKDAY_LABEL_DATES[weekday],
  );
}

function joinRecurrenceList(
  values: string[],
  locale: string | readonly string[],
  localized: boolean,
): string {
  if (!localized) return values.join(", ");
  return new Intl.ListFormat(locale, {
    style: "long",
    type: "conjunction",
  }).format(values);
}

function appendRecurrencePart(label: string, part: string): string {
  return part.startsWith("(") ? `${label} ${part}` : `${label}, ${part}`;
}

function appendRecurrenceQualifier(label: string, part: string): string {
  return `${label} ${part}`;
}

export function formatRecurrenceLabel(
  config: RecurrenceConfig,
  options: RecurrenceLabelOptions = {},
): string {
  const { t } = options;
  const locale = options.locale ?? "en-US";
  const localized = t !== undefined || options.locale !== undefined;
  let label: string;

  if (config.interval === 1) {
    label = frequencyLabel(config.frequency, t);
  } else {
    label = everyLabel(config.frequency, config.interval, t);
  }

  // Ordinal weekdays: "on the 2nd Tuesday"
  if (config.ordinalWeekdays && config.ordinalWeekdays.length > 0) {
    const parts = config.ordinalWeekdays.map((ow) => {
      const dayName = weekdayLabel(ow.day, locale, localized);
      if (ow.ordinal != null) {
        if (ow.ordinal === -1) {
          const last = lastLabel(t);
          return t
            ? t("calendar.recurrence.ordinalWeekday", last, dayName)
            : `${last} ${dayName}`;
        }
        const ordinal = recurrenceOrdinal(ow.ordinal, t);
        return t
          ? t("calendar.recurrence.ordinalWeekday", ordinal, dayName)
          : `${ordinal} ${dayName}`;
      }
      return dayName;
    });
    const weekdays = joinRecurrenceList(parts, locale, localized);
    const part = t
      ? t("calendar.recurrence.onTheWeekdays", weekdays)
      : `on the ${weekdays}`;
    label = appendRecurrenceQualifier(label, part);
  }
  // Simple weekdays: shown for weekly frequency, or any frequency with BYSETPOS
  else if (config.weekdays && config.weekdays.length > 0 &&
    (config.frequency === "weekly" || (config.bySetPos && config.bySetPos.length > 0))) {
    const dayLabels = config.weekdays.map((d) =>
      weekdayLabel(d, locale, localized),
    );
    const weekdays = joinRecurrenceList(dayLabels, locale, localized);
    const part = t
      ? t("calendar.recurrence.onWeekdays", weekdays)
      : `on ${weekdays}`;
    label = appendRecurrenceQualifier(label, part);
  }

  // BYMONTHDAY
  if (config.byMonthDay && config.byMonthDay.length > 0) {
    const dayLabels = config.byMonthDay.map((d) => recurrenceOrdinal(d, t));
    const days = joinRecurrenceList(dayLabels, locale, localized);
    const part = t
      ? t("calendar.recurrence.onMonthDays", days)
      : `on the ${days}`;
    label = appendRecurrenceQualifier(label, part);
  }

  // BYSETPOS
  if (config.bySetPos && config.bySetPos.length > 0) {
    const posLabels = config.bySetPos.map((p) =>
      p === -1 ? lastOccurrenceLabel(t) : recurrenceOrdinal(p, t),
    );
    const positions = joinRecurrenceList(posLabels, locale, localized);
    const part = t
      ? t("calendar.recurrence.occurrence", positions)
      : `(${positions} occurrence)`;
    label = appendRecurrencePart(label, part);
  }

  // End condition
  if (config.end.type === "until") {
    const date = formatMonthDay(config.end.date, locale);
    const part = t ? t("calendar.recurrence.ends", date) : `ends ${date}`;
    label = appendRecurrencePart(label, part);
  } else if (config.end.type === "count") {
    const part = t
      ? t("calendar.recurrence.times", config.end.count)
      : `${config.end.count} times`;
    label = appendRecurrencePart(label, part);
  }

  return label;
}
