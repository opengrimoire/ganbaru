import type {
  EventOrganizer,
  GeoCoordinates,
} from "$lib/calendar/types";

/** Parse persisted calendar JSON without asserting a domain shape. */
export function safeJsonParse(json: string | null): unknown | undefined {
  if (!json) return undefined;
  try {
    return JSON.parse(json) as unknown;
  } catch {
    return undefined;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Parse an array whose values must all be strings. */
export function parseJsonStringArray(json: string | null): string[] | undefined {
  const value = safeJsonParse(json);
  if (!Array.isArray(value)) return undefined;
  return value.every((item: unknown): item is string => typeof item === "string")
    ? [...value]
    : undefined;
}

/** Parse safe-integer minute offsets used by calendar notifications. */
export function parseJsonNotificationMinutes(json: string | null): number[] | undefined {
  const value = safeJsonParse(json);
  if (!Array.isArray(value)) return undefined;
  return value.every((item: unknown): item is number => (
    typeof item === "number" && Number.isSafeInteger(item)
  ))
    ? [...value]
    : undefined;
}

/** Parse a JSON object whose keys and values are both strings. */
export function parseJsonStringRecord(
  json: string | null,
): Record<string, string> | undefined {
  const value = safeJsonParse(json);
  if (!isRecord(value)) return undefined;
  const entries = Object.entries(value);
  if (!entries.every((entry): entry is [string, string] => typeof entry[1] === "string")) {
    return undefined;
  }
  return Object.fromEntries(entries);
}

/** Parse RFC 5545 geographic coordinates within their valid ranges. */
export function parseJsonGeoCoordinates(json: string | null): GeoCoordinates | undefined {
  const value = safeJsonParse(json);
  if (!isRecord(value)) return undefined;
  const { lat, lng } = value;
  if (typeof lat !== "number" || !Number.isFinite(lat) || lat < -90 || lat > 90) {
    return undefined;
  }
  if (typeof lng !== "number" || !Number.isFinite(lng) || lng < -180 || lng > 180) {
    return undefined;
  }
  return { lat, lng };
}

/** Parse the persisted organizer projection used by full event hydration. */
export function parseJsonEventOrganizer(json: string | null): EventOrganizer | undefined {
  const value = safeJsonParse(json);
  if (!isRecord(value) || typeof value.email !== "string" || value.email.trim().length === 0) {
    return undefined;
  }
  if (value.name !== undefined && value.name !== null && typeof value.name !== "string") {
    return undefined;
  }
  return {
    email: value.email,
    ...(typeof value.name === "string" ? { name: value.name } : {}),
  };
}
