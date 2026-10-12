import {
  DEFAULT_CALENDAR_TIME_FORMAT,
  type CalendarTimeFormat,
} from "$lib/stores/preference-options";
import { formatTimeLabel } from "./utils";

export const GUTTER_WIDTH_PER_TIMEZONE = 46;

export function getLocalTimezone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone;
}

export function getTimezoneAbbr(tz: string, date?: Date): string {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: tz,
    timeZoneName: "short",
  }).formatToParts(date ?? new Date());
  return parts.find((p) => p.type === "timeZoneName")?.value ?? tz;
}

export function getHourInTimezone(
  baseDate: Date,
  hour: number,
  tz: string,
  timeFormat: CalendarTimeFormat = DEFAULT_CALENDAR_TIME_FORMAT,
  variant: "short" | "compact" = "short",
): string {
  const d = new Date(baseDate);
  d.setHours(hour, 0, 0, 0);
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: tz,
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).formatToParts(d);
  const timeHour = parts.find((p) => p.type === "hour")?.value ?? "00";
  const minute = parts.find((p) => p.type === "minute")?.value ?? "00";
  return formatTimeLabel(`${timeHour === "24" ? "00" : timeHour}:${minute}`, timeFormat, variant);
}

export function getTimezoneOffset(tz: string, date?: Date): string {
  const d = date ?? new Date();
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: tz,
    timeZoneName: "longOffset",
  }).formatToParts(d);
  return parts.find((p) => p.type === "timeZoneName")?.value ?? "";
}

export function formatTimezoneName(tz: string): string {
  return tz.replace(/_/g, " ").replace(/\//g, " / ");
}

export function getTimezoneLongName(tz: string, date?: Date): string {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: tz,
    timeZoneName: "long",
  }).formatToParts(date ?? new Date());
  return parts.find((p) => p.type === "timeZoneName")?.value ?? formatTimezoneName(tz);
}

export function getTimezoneCity(tz: string): string {
  const segments = tz.split("/");
  const last = segments[segments.length - 1] ?? tz;
  return last.replace(/_/g, " ");
}

export function getTimezoneRegion(tz: string): string {
  const segments = tz.split("/");
  if (segments.length < 2) return "";
  return segments[0] ?? "";
}

export function getTimezoneOffsetMinutes(tz: string, date?: Date): number {
  const raw = getTimezoneOffset(tz, date);
  if (!raw || raw === "GMT") return 0;
  const match = raw.match(/^GMT([+-])(\d{1,2})(?::(\d{2}))?$/);
  if (!match) return 0;
  const sign = match[1] === "-" ? -1 : 1;
  const hours = Number(match[2] ?? "0");
  const minutes = Number(match[3] ?? "0");
  return sign * (hours * 60 + minutes);
}

const DEPRECATED_TIMEZONE_IDS = new Set([
  "GMT",
  "UTC",
  "Universal",
  "Zulu",
  "GB",
  "GB-Eire",
  "NZ",
  "NZ-CHAT",
  "PRC",
  "ROK",
  "ROC",
  "W-SU",
  "EST",
  "MST",
  "HST",
  "EST5EDT",
  "CST6CDT",
  "MST7MDT",
  "PST8PDT",
  "Iran",
  "Israel",
  "Egypt",
  "Cuba",
  "Jamaica",
  "Libya",
  "Navajo",
  "Poland",
  "Portugal",
  "Singapore",
  "Turkey",
  "Hongkong",
  "Japan",
  "Eire",
  "Greenwich",
  "UCT",
]);

let _filteredTimezones: string[] | null = null;

export function listAllTimezones(): string[] {
  if (_filteredTimezones) return _filteredTimezones;
  const all = getIanaTimezones();
  _filteredTimezones = all.filter((tz) => {
    if (tz.startsWith("Etc/")) return false;
    if (DEPRECATED_TIMEZONE_IDS.has(tz)) return false;
    return true;
  });
  return _filteredTimezones;
}

let _ianaTimezones: string[] | null = null;

function getIanaTimezones(): string[] {
  if (_ianaTimezones) return _ianaTimezones;
  try {
    _ianaTimezones = Intl.supportedValuesOf("timeZone");
  } catch {
    _ianaTimezones = [];
  }
  return _ianaTimezones;
}

export type TimezoneAbbrMode = "acronym" | "utc" | "utc-fallback";

const ACRONYM_STOP_WORDS = new Set(["of", "the", "and"]);

export function deriveAcronymFromLongName(longName: string): string | null {
  const trimmed = longName.trim();
  if (!trimmed) return null;
  if (/^(GMT|UTC|Coordinated)/.test(trimmed)) return null;
  const words = trimmed
    .split(/[\s\-&]+/)
    .filter((w) => {
      if (w.length === 0) return false;
      if (ACRONYM_STOP_WORDS.has(w.toLowerCase())) return false;
      const head = w[0];
      if (head !== head.toUpperCase()) return false;
      if (!/^[A-Za-z]/.test(w)) return false;
      return true;
    });
  if (words.length === 0) return null;
  const initials = words
    .map((w) => w[0])
    .join("")
    .toUpperCase();
  if (!/^[A-Z]+$/.test(initials)) return null;
  if (initials.length < 2 || initials.length > 5) return null;
  return initials;
}

export function compactOffsetFromLong(longOffset: string): string {
  if (!longOffset || longOffset === "GMT") return "+0";
  const m = longOffset.match(/^GMT([+-])(\d{1,2}):(\d{2})$/);
  if (!m) {
    const stripped = longOffset.replace(/^GMT/, "");
    return stripped || "+0";
  }
  const sign = m[1];
  const hours = String(parseInt(m[2], 10));
  const minutes = m[3];
  return minutes === "00" ? `${sign}${hours}` : `${sign}${hours}:${minutes}`;
}

export interface TimezoneInfo {
  tz: string;
  abbr: string;
  columnAbbr: string;
  acronym: string;
  numericOffset: string;
  offset: string;
  offsetUtc: string;
  offsetMinutes: number;
  longName: string;
  city: string;
  region: string;
  longNameLower: string;
  cityLower: string;
  regionLower: string;
  abbrLower: string;
  ianaLower: string;
}

const timezoneInfoCache = new Map<string, TimezoneInfo>();

function buildTimezoneInfo(tz: string): TimezoneInfo {
  const abbr = getTimezoneAbbr(tz);
  const stripped = abbr.replace(/^GMT(?=[+-])/, "");
  const columnAbbr = stripped || abbr;
  const rawOffset = getTimezoneOffset(tz);
  const offset = rawOffset || "GMT";
  const offsetUtc = offset.replace(/^GMT/, "UTC");
  const numericOffset = compactOffsetFromLong(offset);
  const offsetMinutes = getTimezoneOffsetMinutes(tz);
  const longName = getTimezoneLongName(tz);
  const city = getTimezoneCity(tz);
  const region = getTimezoneRegion(tz);
  let acronym: string;
  if (!/^GMT/.test(abbr)) {
    acronym = abbr;
  } else {
    const derived = deriveAcronymFromLongName(longName);
    acronym = derived ?? columnAbbr;
  }
  return {
    tz,
    abbr,
    columnAbbr,
    acronym,
    numericOffset,
    offset,
    offsetUtc,
    offsetMinutes,
    longName,
    city,
    region,
    longNameLower: longName.toLowerCase(),
    cityLower: city.toLowerCase(),
    regionLower: region.toLowerCase(),
    abbrLower: abbr.toLowerCase(),
    ianaLower: tz.toLowerCase(),
  };
}

export function getTimezoneInfo(tz: string): TimezoneInfo {
  const cached = timezoneInfoCache.get(tz);
  if (cached) return cached;
  const info = buildTimezoneInfo(tz);
  timezoneInfoCache.set(tz, info);
  return info;
}

let _sortedByOffset: TimezoneInfo[] | null = null;

function getSortedByOffset(): TimezoneInfo[] {
  if (_sortedByOffset) return _sortedByOffset;
  const list = listAllTimezones().map(getTimezoneInfo);
  list.sort((a, b) => {
    if (a.offsetMinutes !== b.offsetMinutes) {
      return a.offsetMinutes - b.offsetMinutes;
    }
    return a.longName.localeCompare(b.longName);
  });
  _sortedByOffset = list;
  return list;
}

export function searchTimezones(
  query: string,
  exclude: string[],
): string[] {
  const sorted = getSortedByOffset();
  const excludeSet = new Set(exclude);
  const normalizedQuery = query.toLowerCase().trim();

  if (!normalizedQuery) {
    const out: string[] = [];
    for (const info of sorted) {
      if (!excludeSet.has(info.tz)) out.push(info.tz);
    }
    return out;
  }

  type Scored = { info: TimezoneInfo; tier: number };
  const scored: Scored[] = [];

  for (const info of sorted) {
    if (excludeSet.has(info.tz)) continue;

    let tier = Infinity;
    if (info.longNameLower.startsWith(normalizedQuery)) tier = 0;
    else if (info.longNameLower.includes(normalizedQuery)) tier = 1;
    if (tier > 2) {
      if (info.cityLower.startsWith(normalizedQuery)) tier = Math.min(tier, 2);
      else if (info.cityLower.includes(normalizedQuery)) tier = Math.min(tier, 3);
    }
    if (tier > 4) {
      if (info.regionLower.startsWith(normalizedQuery)) tier = Math.min(tier, 4);
      else if (info.regionLower.includes(normalizedQuery)) tier = Math.min(tier, 5);
    }
    if (tier > 6) {
      if (info.abbrLower.startsWith(normalizedQuery)) tier = Math.min(tier, 6);
      else if (info.abbrLower.includes(normalizedQuery)) tier = Math.min(tier, 7);
    }
    if (tier > 8) {
      if (info.ianaLower.startsWith(normalizedQuery)) tier = Math.min(tier, 8);
      else if (info.ianaLower.includes(normalizedQuery)) tier = Math.min(tier, 9);
    }

    if (tier !== Infinity) {
      scored.push({ info, tier });
    }
  }

  scored.sort((a, b) => {
    if (a.tier !== b.tier) return a.tier - b.tier;
    if (a.info.offsetMinutes !== b.info.offsetMinutes) {
      return a.info.offsetMinutes - b.info.offsetMinutes;
    }
    return a.info.longName.localeCompare(b.info.longName);
  });

  return scored.map((s) => s.info.tz);
}
