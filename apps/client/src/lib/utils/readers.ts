/**
 * Bounded readers that validate unknown backend values into typed ones, labeling each failure
 * with the path of the rejected field.
 */

const MAX_IDENTIFIER_BYTES = 1_024;

export type UnknownRecord = Record<string, unknown>;

export function readRecord(value: unknown, label: string): UnknownRecord {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  return value as UnknownRecord;
}

export function readString(value: unknown, label: string): string {
  if (typeof value !== "string") throw new Error(`${label} must be a string`);
  return value;
}

export function readIdentifier(value: unknown, label: string): string {
  const identifier = readString(value, label);
  if (identifier.length === 0) throw new Error(`${label} is required`);
  if (new TextEncoder().encode(identifier).byteLength > MAX_IDENTIFIER_BYTES) {
    throw new Error(`${label} exceeds the ${MAX_IDENTIFIER_BYTES} byte limit`);
  }
  for (const character of identifier) {
    const codePoint = character.codePointAt(0);
    if (codePoint !== undefined && (codePoint <= 0x1f || (codePoint >= 0x7f && codePoint <= 0x9f))) {
      throw new Error(`${label} contains a control character`);
    }
  }
  return identifier;
}

export function readBoolean(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${label} must be a boolean`);
  return value;
}

export function readSafeInteger(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) {
    throw new Error(`${label} must be a safe integer`);
  }
  return value;
}

export function readFiniteNumber(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(`${label} must be a finite number`);
  }
  return value;
}

export function readNonNegativeSafeInteger(value: unknown, label: string): number {
  const integer = readSafeInteger(value, label);
  if (integer < 0) throw new Error(`${label} must not be negative`);
  return integer;
}

export function readNullable<T>(value: unknown, label: string, read: (value: unknown, label: string) => T): T | null {
  return value === null ? null : read(value, label);
}

export function readArray<T>(value: unknown, label: string, read: (value: unknown, label: string) => T): T[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  return value.map((entry, index) => read(entry, `${label}[${index}]`));
}

export function readStringArray(value: unknown, label: string): string[] {
  return readArray(value, label, readString);
}

export function readStringRecord(value: unknown, label: string): Record<string, string> {
  const record = readRecord(value, label);
  return Object.fromEntries(
    Object.entries(record).map(([key, entry]) => [key, readString(entry, `${label}.${key}`)]),
  );
}

export function readEnum<const T extends readonly string[]>(value: unknown, values: T, label: string): T[number] {
  const candidate = readString(value, label);
  if (!(values as readonly string[]).includes(candidate)) {
    throw new Error(`${label} has an unsupported value`);
  }
  return candidate as T[number];
}
