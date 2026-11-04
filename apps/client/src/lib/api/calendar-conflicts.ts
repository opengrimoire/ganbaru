/**
 * Bridge for Calendar sync conflicts: event titles, descriptions, and timing (start, end, time zone, all-day flag,
 * recurrence, and the dates and overrides that belong to it).
 */

import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { mapSyncDeviceRef, type SyncDeviceRef } from "$lib/api/sync";
import {
  readArray,
  readBoolean,
  readEnum,
  readNonNegativeSafeInteger,
  readNullable,
  readRecord,
  readString,
} from "$lib/utils/readers";

/** Event fields that can hold a conflict. */
export const CALENDAR_CONFLICT_FIELDS = ["title", "description", "timing"] as const;
export type CalendarConflictField = (typeof CALENDAR_CONFLICT_FIELDS)[number];

/** A text version of a title or description. */
export interface CalendarTextValue {
  kind: "text";
  text: string;
}

/** A timing version. Times are UTC ISO strings; the counts are the excluded dates, added dates, and overrides. */
export interface CalendarTimingValue {
  kind: "timing";
  startTime: string;
  endTime: string;
  timezone: string;
  allDay: boolean;
  rrule: string | null;
  repeatUntil: string | null;
  excludedDates: number;
  addedDates: number;
  overrides: number;
}

export type CalendarConflictValue = CalendarTextValue | CalendarTimingValue;

/** One concurrent version of an event field. */
export interface CalendarConflictVersion {
  /** Opaque id for choosing this version. */
  version: string;
  device: SyncDeviceRef;
  editedAtMs: number;
  /** Whether every device shows this version now. */
  displayed: boolean;
  value: CalendarConflictValue;
}

/** The versions of one event field in conflict, newest first. */
export interface CalendarFieldConflict {
  eventId: string;
  field: CalendarConflictField;
  versions: CalendarConflictVersion[];
}

/** Keep the version every device shows, or use another one. */
export type CalendarConflictChoice = { kind: "displayed" } | { kind: "version"; version: string };

export interface CalendarConflictResolution {
  eventId: string;
  field: CalendarConflictField;
  choice: CalendarConflictChoice;
}

/** A conflict read or resolution failure. `resolved` means the conflict or version is gone and views should reload. */
export class CalendarConflictError extends Error {
  constructor(readonly code: "resolved" | "failed", message: string) {
    super(message);
    this.name = "CalendarConflictError";
  }
}

const VALUE_KINDS = ["text", "timing"] as const;

function mapValue(value: unknown, label: string): CalendarConflictValue {
  const row = readRecord(value, label);
  const kind = readEnum(row.kind, VALUE_KINDS, `${label}.kind`);
  if (kind === "text") return { kind, text: readString(row.text, `${label}.text`) };
  return {
    kind,
    startTime: readString(row.startTime, `${label}.startTime`),
    endTime: readString(row.endTime, `${label}.endTime`),
    timezone: readString(row.timezone, `${label}.timezone`),
    allDay: readBoolean(row.allDay, `${label}.allDay`),
    rrule: readNullable(row.rrule, `${label}.rrule`, readString),
    repeatUntil: readNullable(row.repeatUntil, `${label}.repeatUntil`, readString),
    excludedDates: readNonNegativeSafeInteger(row.excludedDates, `${label}.excludedDates`),
    addedDates: readNonNegativeSafeInteger(row.addedDates, `${label}.addedDates`),
    overrides: readNonNegativeSafeInteger(row.overrides, `${label}.overrides`),
  };
}

function mapVersion(value: unknown, label: string): CalendarConflictVersion {
  const row = readRecord(value, label);
  return {
    version: readString(row.version, `${label}.version`),
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    editedAtMs: readNonNegativeSafeInteger(row.editedAtMs, `${label}.editedAtMs`),
    displayed: readBoolean(row.displayed, `${label}.displayed`),
    value: mapValue(row.value, `${label}.value`),
  };
}

function mapFieldConflict(value: unknown, label: string): CalendarFieldConflict {
  const row = readRecord(value, label);
  const field = readEnum(row.field, CALENDAR_CONFLICT_FIELDS, `${label}.field`);
  const versions = readArray(row.versions, `${label}.versions`, mapVersion);
  const expected = field === "timing" ? "timing" : "text";
  if (versions.some((version) => version.value.kind !== expected)) {
    throw new Error(`${label}.versions holds a value that does not fit the ${field} field`);
  }
  return { eventId: readString(row.eventId, `${label}.eventId`), field, versions };
}

/** Validates the conflicts read from the native side. */
export function mapCalendarConflicts(value: unknown): CalendarFieldConflict[] {
  const row = readRecord(value, "calendar conflicts");
  return readArray(row.conflicts, "calendar conflicts.conflicts", mapFieldConflict);
}

const CONFLICT_ERROR_CODES = ["resolved", "failed"] as const;

/** Turns a native `{ code, message }` rejection into a {@link CalendarConflictError}, and passes other errors through. */
export function toCalendarConflictError(error: unknown): unknown {
  if (typeof error !== "object" || error === null || !("code" in error) || !("message" in error)) return error;
  const { code, message } = error;
  if (typeof message !== "string") return error;
  const known = CONFLICT_ERROR_CODES.find((candidate) => candidate === code);
  return known ? new CalendarConflictError(known, message) : error;
}

/** Reads every open Calendar conflict; the list is empty on a vault that does not sync. */
export async function loadCalendarConflicts(): Promise<CalendarFieldConflict[]> {
  const dbUrl = await ensureDbUrl();
  try {
    return mapCalendarConflicts(await invoke<unknown>("calendar_conflicts", { dbUrl }));
  } catch (error: unknown) {
    throw toCalendarConflictError(error);
  }
}

/** Resolves a conflict on every linked device. */
export async function resolveCalendarConflict(resolution: CalendarConflictResolution): Promise<void> {
  const dbUrl = await ensureDbUrl();
  try {
    await invoke<void>("calendar_resolve_conflict", { dbUrl, resolution });
  } catch (error: unknown) {
    throw toCalendarConflictError(error);
  }
}
