import type { NotesIconColor } from "../contracts/assets";
import { NOTES_DATE_MENTION_FORMATS, NOTES_LOCAL_OBJECT_MENTION_TYPES } from "../contracts/core";
import type { NotesColor, NotesDateMentionFormat, NotesLocalObjectMentionType } from "../contracts/core";
import type { NotesDatabaseViewType } from "$lib/notes/contracts/database";
import { isNotesIconColor } from "./assets";
import { isNotesColor } from "./blocks";
import { isNotesDatabaseViewType } from "./database";
import { dateMentionBoundaryLooksIso } from "./rich-text";

export type UnknownRecord = Record<string, unknown>;

export const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/iu;

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function readRecord(value: unknown, label: string): UnknownRecord {
  if (!isRecord(value)) throw new Error(`${label} must be an object`);
  return value;
}

export function readString(value: unknown, label: string): string {
  if (typeof value !== "string") throw new Error(`${label} must be a string`);
  return value;
}

export function readUuidString(value: unknown, label: string): string {
  const id = readString(value, label);
  if (!UUID_PATTERN.test(id)) throw new Error(`${label} must be a UUID`);
  return id;
}

export function readOptionalUuidString(value: unknown, label: string): string | undefined {
  if (value === undefined) return undefined;
  return readUuidString(value, label);
}

export function readNullableString(value: unknown, label: string): string | null {
  if (value === null) return null;
  return readString(value, label);
}

export function readNullableUuidString(value: unknown, label: string): string | null {
  if (value === null) return null;
  return readUuidString(value, label);
}

export function readStringArray(value: unknown, label: string): string[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  return value.map((item, index) => readString(item, `${label}[${index}]`));
}

export function readRecordArray(value: unknown, label: string): Record<string, unknown>[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  return value.map((item, index) => readRecord(item, `${label}[${index}]`));
}

export function readBoolean(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${label} must be a boolean`);
  return value;
}

export function readInteger(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isInteger(value)) {
    throw new Error(`${label} must be an integer`);
  }
  return value;
}

export function readFiniteNumber(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(`${label} must be a finite number`);
  }
  return value;
}

export function containsControlCharacters(value: string): boolean {
  return [...value].some((character) => {
    const codePoint = character.codePointAt(0);
    return codePoint !== undefined && codePoint < 32 && character !== "\n" && character !== "\t";
  });
}

export function readDisplayString(value: unknown, label: string): string {
  const text = readString(value, label);
  if (!text.trim()) throw new Error(`${label} must not be empty`);
  if (containsControlCharacters(text)) throw new Error(`${label} must not contain control characters`);
  return text;
}

export function readMentionObjectId(value: unknown, label: string): string {
  const text = readString(value, label);
  if (!text.trim()) throw new Error(`${label} must not be empty`);
  if (text.length > 2048) throw new Error(`${label} is too long`);
  for (const character of text) {
    const codePoint = character.codePointAt(0);
    if (codePoint !== undefined && codePoint < 32) {
      throw new Error(`${label} must not contain control characters`);
    }
  }
  return text;
}

export function readLocalObjectMentionType(
  value: unknown,
  label: string,
): NotesLocalObjectMentionType {
  const objectType = readString(value, label);
  if (!(NOTES_LOCAL_OBJECT_MENTION_TYPES as readonly string[]).includes(objectType)) {
    throw new Error(`${label} is unsupported`);
  }
  return objectType as NotesLocalObjectMentionType;
}

export function readOptionalDisplayString(value: unknown, label: string): string | undefined {
  if (value === undefined) return undefined;
  return readDisplayString(value, label);
}

export function readNotesColor(value: unknown, label: string): NotesColor {
  const color = readString(value, label);
  if (!isNotesColor(color)) throw new Error(`${label} must be a supported Notion color`);
  return color;
}

export function readNotesIconColor(value: unknown, label: string): NotesIconColor {
  const color = readString(value, label);
  if (!isNotesIconColor(color)) throw new Error(`${label} must be a supported Notion icon color`);
  return color;
}

export function readNullableDisplayString(value: unknown, label: string): string | null {
  if (value === null) return null;
  return readDisplayString(value, label);
}

export function readDateMentionBoundary(value: unknown, label: string): string {
  const text = readDisplayString(value, label);
  if (!dateMentionBoundaryLooksIso(text)) {
    throw new Error(`${label} must be an ISO date or date-time`);
  }
  return text;
}

export function readNullableDateMentionBoundary(value: unknown, label: string): string | null {
  if (value === null) return null;
  return readDateMentionBoundary(value, label);
}

export function readNotesDatabaseViewType(value: unknown, label: string): NotesDatabaseViewType {
  const viewType = readString(value, label);
  if (!isNotesDatabaseViewType(viewType)) {
    throw new Error(`${label} must be a supported database view type`);
  }
  return viewType;
}

export function readNullableRecord(value: unknown, label: string): Record<string, unknown> | null {
  if (value === null) return null;
  return readRecord(value, label);
}

export function readNonNegativeInteger(value: unknown, label: string): number {
  const integer = readInteger(value, label);
  if (integer < 0) throw new Error(`${label} must not be negative`);
  return integer;
}

export function readNullableDateMentionFormat(value: unknown, label: string): NotesDateMentionFormat | null {
  if (value === null) return null;
  const format = readString(value, label);
  if (!(NOTES_DATE_MENTION_FORMATS as readonly string[]).includes(format)) {
    throw new Error(`${label} must be one of ${NOTES_DATE_MENTION_FORMATS.join(", ")}`);
  }
  return format as NotesDateMentionFormat;
}
