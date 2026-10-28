/**
 * Bridge for Notes sync conflicts: page titles, block text, property values, discussion status, and comment bodies.
 */

import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { mapSyncDeviceRef, type SyncDeviceRef } from "$lib/api/sync";
import { withOwnerRequiredPrompt } from "$lib/vault/owner-required";
import {
  readArray,
  readBoolean,
  readEnum,
  readNonNegativeSafeInteger,
  readRecord,
  readString,
} from "$lib/utils/readers";

/**
 * Fields of a page that can hold a conflict. The id of a conflict is the page id for a title, the block id for block
 * text, the property id for a value, the thread id for a discussion status, and the comment id for a comment body.
 */
export const NOTES_CONFLICT_TARGETS = [
  "page_title",
  "block_text",
  "property_value",
  "thread_status",
  "comment_body",
] as const;
export type NotesConflictTarget = (typeof NOTES_CONFLICT_TARGETS)[number];

/** One concurrent version of a field. */
export interface NotesConflictVersion {
  /** Opaque id for choosing this version. */
  version: string;
  device: SyncDeviceRef;
  editedAtMs: number;
  /** Whether every device shows this version now. */
  displayed: boolean;
  /** Plain text of the version; for a discussion status, `open` or `resolved`. */
  text: string;
}

/** The versions of one field in conflict. */
export interface NotesFieldConflict {
  target: NotesConflictTarget;
  id: string;
  versions: NotesConflictVersion[];
}

/** The conflicts of a page; empty when it has none. */
export interface NotesPageConflicts {
  pageId: string;
  conflicts: NotesFieldConflict[];
}

/** Keep the version every device shows, or use another one. */
export type NotesConflictChoice = { kind: "displayed" } | { kind: "version"; version: string };

export interface NotesConflictResolution {
  pageId: string;
  target: NotesConflictTarget;
  id: string;
  choice: NotesConflictChoice;
}

/** A conflict read or resolution failure. `resolved` means the conflict or version is gone. */
export class NotesConflictError extends Error {
  constructor(readonly code: "resolved" | "failed", message: string) {
    super(message);
    this.name = "NotesConflictError";
  }
}

function mapVersion(value: unknown, label: string): NotesConflictVersion {
  const row = readRecord(value, label);
  return {
    version: readString(row.version, `${label}.version`),
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    editedAtMs: readNonNegativeSafeInteger(row.editedAtMs, `${label}.editedAtMs`),
    displayed: readBoolean(row.displayed, `${label}.displayed`),
    text: readString(row.text, `${label}.text`),
  };
}

function mapFieldConflict(value: unknown, label: string): NotesFieldConflict {
  const row = readRecord(value, label);
  return {
    target: readEnum(row.target, NOTES_CONFLICT_TARGETS, `${label}.target`),
    id: readString(row.id, `${label}.id`),
    versions: readArray(row.versions, `${label}.versions`, mapVersion),
  };
}

/** Validates the conflicts of a page from the native side. */
export function mapNotesPageConflicts(value: unknown): NotesPageConflicts {
  const row = readRecord(value, "notes conflicts");
  return {
    pageId: readString(row.pageId, "notes conflicts.pageId"),
    conflicts: readArray(row.conflicts, "notes conflicts.conflicts", mapFieldConflict),
  };
}

const CONFLICT_ERROR_CODES = ["resolved", "failed"] as const;

/** Turns a native `{ code, message }` rejection into a {@link NotesConflictError}, and passes other errors through. */
export function toNotesConflictError(error: unknown): unknown {
  if (typeof error !== "object" || error === null || !("code" in error) || !("message" in error)) return error;
  const { code, message } = error;
  if (typeof message !== "string") return error;
  const known = CONFLICT_ERROR_CODES.find((candidate) => candidate === code);
  return known ? new NotesConflictError(known, message) : error;
}

/** Reads the conflicts of a page. */
export async function loadNotesPageConflicts(pageId: string): Promise<NotesPageConflicts> {
  const dbUrl = await ensureDbUrl();
  try {
    return mapNotesPageConflicts(await invoke<unknown>("notes_conflicts", { dbUrl, pageId }));
  } catch (error: unknown) {
    throw toNotesConflictError(error);
  }
}

/**
 * Converts a row value stored under another type than its property's current type, or discards it. A value the new
 * type cannot represent converts to that type's empty value.
 */
export async function settleNotesMismatchedValue(pageId: string, propertyId: string, convert: boolean): Promise<void> {
  const dbUrl = await ensureDbUrl();
  await withOwnerRequiredPrompt(() =>
    invoke<null>("notes_settle_mismatched_value", { dbUrl, pageId, propertyId, convert }));
}

/** Resolves a conflict on every linked device. */
export async function resolveNotesConflict(resolution: NotesConflictResolution): Promise<void> {
  const dbUrl = await ensureDbUrl();
  try {
    await withOwnerRequiredPrompt(() => invoke<null>("notes_resolve_conflict", { dbUrl, resolution }));
  } catch (error: unknown) {
    throw toNotesConflictError(error);
  }
}
