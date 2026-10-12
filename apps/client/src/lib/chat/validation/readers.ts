import type { JsonValue, TurnModeSnapshot, UtcTimestamp, VersionedJson } from "../contracts";
import { INTERACTION_MODES, SAFETY_MODES } from "../contracts";
import { readEnum, readNonNegativeSafeInteger, readRecord, readString } from "$lib/utils/readers";

export * from "$lib/utils/readers";

const MAX_JSON_DEPTH = 32;
const MAX_JSON_NODES = 10_000;

export function readUtcTimestamp(value: unknown, label: string): UtcTimestamp {
  const timestamp = readString(value, label);
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(?:Z|[+-]00:00)$/.exec(timestamp);
  if (!match) {
    throw new Error(`${label} must be an RFC 3339 UTC timestamp`);
  }
  const [, yearText, monthText, dayText, hourText, minuteText, secondText] = match;
  const [year, month, day, hour, minute, second] = [
    yearText,
    monthText,
    dayText,
    hourText,
    minuteText,
    secondText,
  ].map(Number);
  const parsed = new Date(0);
  parsed.setUTCFullYear(year, month - 1, day);
  parsed.setUTCHours(hour, minute, second, 0);
  if (
    parsed.getUTCFullYear() !== year
    || parsed.getUTCMonth() !== month - 1
    || parsed.getUTCDate() !== day
    || parsed.getUTCHours() !== hour
    || parsed.getUTCMinutes() !== minute
    || parsed.getUTCSeconds() !== second
  ) {
    throw new Error(`${label} must be an RFC 3339 UTC timestamp`);
  }
  return timestamp;
}

export function readJsonValue(value: unknown, label: string): JsonValue {
  let nodes = 0;

  const visit = (entry: unknown, path: string, depth: number): JsonValue => {
    nodes += 1;
    if (nodes > MAX_JSON_NODES) throw new Error(`${label} exceeds the JSON node limit`);
    if (depth > MAX_JSON_DEPTH) throw new Error(`${label} exceeds the JSON depth limit`);
    if (entry === null || typeof entry === "string" || typeof entry === "boolean") return entry;
    if (typeof entry === "number" && Number.isFinite(entry)) return entry;
    if (Array.isArray(entry)) return entry.map((item, index) => visit(item, `${path}[${index}]`, depth + 1));
    if (typeof entry === "object") {
      const result: { [key: string]: JsonValue } = {};
      for (const [key, item] of Object.entries(entry)) result[key] = visit(item, `${path}.${key}`, depth + 1);
      return result;
    }
    throw new Error(`${path} is not valid JSON`);
  };

  return visit(value, label, 0);
}

export function readVersionedJson(value: unknown, label: string): VersionedJson {
  const record = readRecord(value, label);
  return {
    schemaVersion: readNonNegativeSafeInteger(record.schemaVersion, `${label}.schemaVersion`),
    value: readJsonValue(record.value, `${label}.value`),
  };
}

export function readTurnModeSnapshot(value: unknown, label: string): TurnModeSnapshot {
  const record = readRecord(value, label);
  return {
    safetyMode: readEnum(record.safetyMode, SAFETY_MODES, `${label}.safetyMode`),
    interactionMode: readEnum(record.interactionMode, INTERACTION_MODES, `${label}.interactionMode`),
  };
}
