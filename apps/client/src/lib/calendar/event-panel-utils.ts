import { hasOnlyShortcutModifier, hasShortcutModifier } from "$lib/keyboard-shortcuts";
import type { CalendarTimeFormat } from "$lib/stores/preference-options";
import type { ProjectDefaultEventTimeMode } from "$lib/projects/settings/duration";

export type RovingOrientation = "horizontal" | "vertical" | "grid";

export interface RovingMoveInput {
  currentIndex: number;
  itemCount: number;
  key: string;
  orientation: RovingOrientation;
  columns?: number;
}

export interface DraftCommit<T> {
  value: T;
  committed: boolean;
}

export interface ProjectDurationDefaultInput {
  mode: "create" | "edit";
  allDay: boolean;
  activeEdit: boolean;
  defaultEventTimeMode: ProjectDefaultEventTimeMode;
  defaultEventDurationMinutes: number | null;
}

export interface ProjectDefaultEventTitleInput {
  currentTitle: string;
  defaultEventName: string | null;
}

/**
 * Local keydown handler for panel input/textarea elements.
 *
 * Stops propagation for normal text editing while explicitly letting the
 * panel's own shortcuts (Mod+Enter save, Mod+D delete, Escape close) bubble
 * up to the window-level listeners.
 */
export function panelInputKeydown(e: KeyboardEvent): void {
  if (e.key === "Enter" && hasShortcutModifier(e)) return;
  if ((e.key === "d" || e.key === "D") && hasOnlyShortcutModifier(e)) return;
  if (e.key === "Escape") return;
  e.stopPropagation();
}

export function projectDurationDefaultForSelection(
  input: ProjectDurationDefaultInput,
): number | null {
  if (input.allDay || input.activeEdit || input.defaultEventTimeMode === "all_day") return null;
  return input.defaultEventDurationMinutes;
}

export function projectAllDayDefaultForSelection(input: ProjectDurationDefaultInput): boolean {
  if (input.allDay || input.activeEdit) return false;
  return input.defaultEventTimeMode === "all_day";
}

export function projectDefaultEventTitleForSelection(
  input: ProjectDefaultEventTitleInput,
): string {
  if (input.currentTitle.trim()) return input.currentTitle;
  return input.defaultEventName ?? "";
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function isValidHour(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= 23;
}

function isValidMinute(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= 59;
}

function canStartMinute(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= 5;
}

function isValidTwelveHour(value: number): boolean {
  return Number.isInteger(value) && value >= 1 && value <= 12;
}

function sanitizeTimeBodyInput(value: string): string {
  let result = "";
  let colonUsed = false;

  for (const char of value) {
    if (/^\d$/.test(char)) {
      result += char;
    } else if (char === ":" && !colonUsed) {
      result += char;
      colonUsed = true;
    }
  }

  return result;
}

function splitMeridiemDraft(value: string): { body: string; period: "am" | "pm" | null } {
  const match = /([ap])\s*m?/i.exec(value);
  if (!match || match.index === undefined) {
    return { body: sanitizeTimeBodyInput(value), period: null };
  }

  const period = match[1].toLowerCase() === "a" ? "am" : "pm";
  return {
    body: sanitizeTimeBodyInput(value.slice(0, match.index)),
    period,
  };
}

function formatTimeDraftDigits(
  draft: string,
  formatShortCompact: boolean,
  timeFormat: CalendarTimeFormat,
): string {
  const explicitColon = /^(\d{0,2}):(\d*)$/.exec(draft);
  if (explicitColon) {
    return `${explicitColon[1]}:${explicitColon[2].slice(0, 2)}`;
  }

  if (!/^\d+$/.test(draft)) return draft;

  const digits = draft.slice(0, 4);
  const digitAt = (index: number) => Number(digits[index]);
  const firstTwo = digits.length >= 2 ? Number(digits.slice(0, 2)) : Number.NaN;
  const trailingTwo = digits.length >= 3 ? Number(digits.slice(1, 3)) : Number.NaN;
  const validHour = timeFormat === "12h" ? isValidTwelveHour : isValidHour;

  if (digits.length === 2) {
    if (validHour(firstTwo)) return digits;
    if (canStartMinute(digitAt(1))) return `${digits[0]}:${digits[1]}`;
    return timeFormat === "12h" ? `${digits[0]}:0${digits[1]}` : `0${digits[0]}:0${digits[1]}`;
  }

  if (digits.length === 3) {
    if (formatShortCompact && isValidMinute(trailingTwo)) return `${digits[0]}:${digits.slice(1)}`;
    if (validHour(firstTwo) && canStartMinute(digitAt(2))) return `${digits.slice(0, 2)}:${digits[2]}`;
    if (isValidMinute(trailingTwo)) return `${digits[0]}:${digits.slice(1)}`;
    return digits;
  }

  if (digits.length === 4) {
    const hours = Number(digits.slice(0, 2));
    const minutes = Number(digits.slice(2));
    if (validHour(hours) && isValidMinute(minutes)) return `${digits.slice(0, 2)}:${digits.slice(2)}`;
  }

  return digits;
}

/**
 * Move a roving focus index for one-dimensional lists and fixed-column grids.
 */
export function moveRovingIndex(input: RovingMoveInput): number {
  const itemCount = Math.max(0, Math.trunc(input.itemCount));
  if (itemCount === 0) return -1;

  const currentIndex = clamp(Math.trunc(input.currentIndex), 0, itemCount - 1);
  if (input.key === "Home") return 0;
  if (input.key === "End") return itemCount - 1;

  if (input.orientation === "horizontal") {
    if (input.key === "ArrowLeft") return clamp(currentIndex - 1, 0, itemCount - 1);
    if (input.key === "ArrowRight") return clamp(currentIndex + 1, 0, itemCount - 1);
    return currentIndex;
  }

  if (input.orientation === "vertical") {
    if (input.key === "ArrowUp") return clamp(currentIndex - 1, 0, itemCount - 1);
    if (input.key === "ArrowDown") return clamp(currentIndex + 1, 0, itemCount - 1);
    return currentIndex;
  }

  const columns = Math.max(1, Math.trunc(input.columns ?? 1));
  if (input.key === "ArrowLeft") return clamp(currentIndex - 1, 0, itemCount - 1);
  if (input.key === "ArrowRight") return clamp(currentIndex + 1, 0, itemCount - 1);
  if (input.key === "ArrowUp") return clamp(currentIndex - columns, 0, itemCount - 1);
  if (input.key === "ArrowDown") return clamp(currentIndex + columns, 0, itemCount - 1);
  return currentIndex;
}

function parseCanonicalTime(value: string): number | null {
  const match = /^(\d{2}):(\d{2})$/.exec(value);
  if (!match) return null;
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (!isValidHour(hour) || !isValidMinute(minute)) return null;
  return hour * 60 + minute;
}

function hourTwelveToTwentyFour(hour: number, period: "am" | "pm"): number {
  if (period === "am") return hour === 12 ? 0 : hour;
  return hour === 12 ? 12 : hour + 12;
}

function circularMinuteDistance(a: number, b: number): number {
  const direct = Math.abs(a - b);
  return Math.min(direct, 1440 - direct);
}

function chooseNearestPeriod(hour: number, minute: number, previous: string): "am" | "pm" {
  const previousMinute = parseCanonicalTime(previous);
  if (previousMinute === null) return "am";

  const amMinute = hourTwelveToTwentyFour(hour, "am") * 60 + minute;
  const pmMinute = hourTwelveToTwentyFour(hour, "pm") * 60 + minute;
  return circularMinuteDistance(amMinute, previousMinute) <= circularMinuteDistance(pmMinute, previousMinute)
    ? "am"
    : "pm";
}

function normalizeTwentyFourHourDraft(value: string): string | null {
  const draft = displayTimeDraft(value, true);
  if (!draft) return null;

  const colonMatch = /^(\d{1,2}):(\d{1,2})$/.exec(draft);
  if (colonMatch) {
    const hours = Number(colonMatch[1]);
    const minutes = colonMatch[2].length === 1
      ? Number(colonMatch[2]) * 10
      : Number(colonMatch[2]);
    if (!isValidHour(hours) || !isValidMinute(minutes)) return null;
    return `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}`;
  }

  if (!/^\d{1,2}$/.test(draft)) {
    return null;
  }

  const hours = Number(draft);
  if (!isValidHour(hours)) return null;
  return `${String(hours).padStart(2, "0")}:00`;
}

function normalizeTwelveHourDraft(value: string, previous = "00:00"): string | null {
  const { period, body } = splitMeridiemDraft(value);
  const draft = formatTimeDraftDigits(body, true, "12h");
  if (!draft) return null;

  const colonMatch = /^(\d{1,2}):(\d{1,2})$/.exec(draft);
  if (colonMatch) {
    const hour = Number(colonMatch[1]);
    const minute = colonMatch[2].length === 1
      ? Number(colonMatch[2]) * 10
      : Number(colonMatch[2]);
    if (!isValidTwelveHour(hour) || !isValidMinute(minute)) return null;
    const resolvedPeriod = period ?? chooseNearestPeriod(hour, minute, previous);
    const hour24 = hourTwelveToTwentyFour(hour, resolvedPeriod);
    return `${String(hour24).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;
  }

  if (!/^\d{1,2}$/.test(draft)) return null;

  const hour = Number(draft);
  if (!isValidTwelveHour(hour)) return null;
  const resolvedPeriod = period ?? chooseNearestPeriod(hour, 0, previous);
  const hour24 = hourTwelveToTwentyFour(hour, resolvedPeriod);
  return `${String(hour24).padStart(2, "0")}:00`;
}

export function normalizeTimeDraft(
  value: string,
  timeFormat: CalendarTimeFormat = "24h",
  previous?: string,
): string | null {
  if (timeFormat === "12h") return normalizeTwelveHourDraft(value, previous);
  return normalizeTwentyFourHourDraft(value);
}

export function sanitizeTimeDraftInput(value: string, allowMeridiem = false): string {
  if (!allowMeridiem) return sanitizeTimeBodyInput(value);
  const { body, period } = splitMeridiemDraft(value);
  return `${body}${period ?? ""}`;
}

export function displayTimeDraft(
  value: string,
  formatShortCompact = false,
  timeFormat: CalendarTimeFormat = "24h",
): string {
  if (timeFormat === "12h") {
    const { body, period } = splitMeridiemDraft(value);
    const draft = formatTimeDraftDigits(body, formatShortCompact, timeFormat);
    if (!draft) return period ?? "";
    return `${draft}${period ?? ""}`;
  }

  const draft = sanitizeTimeDraftInput(value);
  if (!draft) return "";

  return formatTimeDraftDigits(draft, formatShortCompact, timeFormat);
}

export function commitTimeDraft(
  draft: string,
  previous: string,
  timeFormat: CalendarTimeFormat = "24h",
): DraftCommit<string> {
  const normalized = normalizeTimeDraft(draft, timeFormat, previous);
  if (!normalized) return { value: previous, committed: false };
  return { value: normalized, committed: normalized !== previous };
}

export function restoreTimeDraft(
  canonicalValue: string,
  timeFormat: CalendarTimeFormat = "24h",
): string {
  if (timeFormat === "24h") return canonicalValue;
  const normalized = normalizeTwentyFourHourDraft(canonicalValue);
  if (!normalized) return canonicalValue;
  const [hourPart, minutePart] = normalized.split(":");
  const hour = Number(hourPart);
  const displayHour = hour % 12 === 0 ? 12 : hour % 12;
  const period = hour < 12 ? "am" : "pm";
  if (minutePart === "00") return `${displayHour}${period}`;
  return `${displayHour}:${minutePart}${period}`;
}

export function normalizeIntegerDraft(value: string, min: number, max: number): number | null {
  const draft = value.trim();
  if (!/^\d+$/.test(draft)) return null;
  const parsed = Number(draft);
  if (!Number.isSafeInteger(parsed)) return null;
  return clamp(parsed, min, max);
}

export function commitIntegerDraft(
  draft: string,
  previous: number,
  min: number,
  max: number,
): DraftCommit<number> {
  const normalized = normalizeIntegerDraft(draft, min, max);
  if (normalized === null) return { value: previous, committed: false };
  return { value: normalized, committed: normalized !== previous };
}
