import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import type { EventColor } from "$lib/calendar/types";
import { normalizeEventColor } from "$lib/calendar/utils";
import { mapSyncDeviceRef } from "$lib/api/sync";
import type {
  QuickNote,
  QuickNoteConflict,
  QuickNoteConflictField,
  QuickNoteConflictGroup,
  QuickNoteConflictResolution,
  QuickNoteConflictResolved,
  QuickNoteConflictVersion,
  QuickNoteCreate,
  QuickNoteImage,
  QuickNoteImageContentType,
  QuickNoteImageErrorCode,
  QuickNoteImagePick,
  QuickNoteRevisionRequest,
  QuickNoteReorderRequest,
  QuickNotesCollection,
  QuickNotesTrashPurge,
  QuickNotesWindow,
  QuickNoteTextRun,
  QuickNoteTag,
  QuickNoteTagReorderRequest,
  QuickNoteUpdate,
} from "$lib/quick-notes/types";
import {
  QUICK_NOTE_IMAGE_CONTENT_TYPES,
  QUICK_NOTE_IMAGE_LIMIT,
  QUICK_NOTE_ORDER_KEY_MAX_CHARS,
  QUICK_NOTE_TAG_NAME_MAX_CHARS,
} from "$lib/quick-notes/types";

function record(value: unknown, label: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  return value as Record<string, unknown>;
}

function string(value: unknown, label: string): string {
  if (typeof value !== "string") throw new Error(`${label} must be a string`);
  return value;
}

function nullableString(value: unknown, label: string): string | null {
  return value === null ? null : string(value, label);
}

function boolean(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${label} must be a boolean`);
  return value;
}

function integer(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) {
    throw new Error(`${label} must be an integer`);
  }
  return value;
}

function mapRun(value: unknown, label: string): QuickNoteTextRun {
  const row = record(value, label);
  return {
    content: string(row.content, `${label}.content`),
    bold: boolean(row.bold, `${label}.bold`),
    italic: boolean(row.italic, `${label}.italic`),
    underline: boolean(row.underline, `${label}.underline`),
  };
}

const DIGEST_PATTERN = /^[0-9a-f]{64}$/u;

function isImageContentType(value: unknown): value is QuickNoteImageContentType {
  return QUICK_NOTE_IMAGE_CONTENT_TYPES.some((type) => type === value);
}

function positiveInteger(value: unknown, label: string): number {
  const number = integer(value, label);
  if (number <= 0) throw new Error(`${label} must be positive`);
  return number;
}

/** Validates one image reference from the native side. */
export function mapQuickNoteImage(value: unknown, label = "quick note image"): QuickNoteImage {
  const row = record(value, label);
  const sha256 = string(row.sha256, `${label}.sha256`);
  if (!DIGEST_PATTERN.test(sha256)) throw new Error(`${label}.sha256 is invalid`);
  if (!isImageContentType(row.contentType)) throw new Error(`${label}.contentType is invalid`);
  return {
    sha256,
    contentType: row.contentType,
    byteSize: positiveInteger(row.byteSize, `${label}.byteSize`),
    width: positiveInteger(row.width, `${label}.width`),
    height: positiveInteger(row.height, `${label}.height`),
  };
}

function mapImages(value: unknown, label: string): QuickNoteImage[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  return value.map((image, index) => mapQuickNoteImage(image, `${label}[${index}]`));
}

export function mapQuickNote(value: unknown): QuickNote {
  const row = record(value, "quick note");
  if (!Array.isArray(row.runs)) throw new Error("quick note.runs must be an array");
  const color = normalizeEventColor(row.color);
  if (color === undefined) throw new Error("quick note.color is invalid");
  return {
    id: string(row.id, "quick note.id"),
    title: string(row.title, "quick note.title"),
    bodyPlainText: string(row.bodyPlainText, "quick note.bodyPlainText"),
    runs: row.runs.map((run, index) => mapRun(run, `quick note.runs[${index}]`)),
    previewTruncated: boolean(row.previewTruncated, "quick note.previewTruncated"),
    images: mapImages(row.images, "quick note.images"),
    color,
    tagId: nullableString(row.tagId, "quick note.tagId"),
    pinned: boolean(row.pinned, "quick note.pinned"),
    archived: boolean(row.archived, "quick note.archived"),
    trashedAt: nullableString(row.trashedAt, "quick note.trashedAt"),
    revision: integer(row.revision, "quick note.revision"),
    createdAt: string(row.createdAt, "quick note.createdAt"),
    updatedAt: string(row.updatedAt, "quick note.updatedAt"),
    hasConflict: boolean(row.hasConflict, "quick note.hasConflict"),
  };
}

export async function listQuickNotes(
  collection: QuickNotesCollection,
  query: string,
  tagId: string | null = null,
  cursor: string | null = null,
): Promise<QuickNotesWindow> {
  const value = await invoke<unknown>("quick_notes_list", {
    dbUrl: await ensureDbUrl(),
    request: { collection, query: query || null, tagId, cursor, pageSize: 60 },
  });
  const result = record(value, "quick notes window");
  if (!Array.isArray(result.notes)) throw new Error("quick notes window.notes must be an array");
  return {
    notes: result.notes.map(mapQuickNote),
    nextCursor: nullableString(result.nextCursor, "quick notes window.nextCursor"),
  };
}

/** A native note write failure with a stable recovery code. */
export class QuickNoteWriteError extends Error {
  constructor(readonly code: "revision_conflict" | "failed", message: string) {
    super(message);
    this.name = "QuickNoteWriteError";
  }
}

async function invokeNote(command: string, args: Record<string, unknown>): Promise<QuickNote> {
  try {
    return mapQuickNote(await invoke<unknown>(command, { dbUrl: await ensureDbUrl(), ...args }));
  } catch (error: unknown) {
    if (typeof error === "object" && error !== null && "code" in error && "message" in error
      && (error.code === "revision_conflict" || error.code === "failed") && typeof error.message === "string") {
      throw new QuickNoteWriteError(error.code, error.message);
    }
    throw error;
  }
}

export function getQuickNote(id: string): Promise<QuickNote> {
  return invokeNote("quick_notes_load", { id });
}

export function createQuickNote(note: QuickNoteCreate): Promise<QuickNote> {
  return invokeNote("quick_notes_create", { note });
}

export function updateQuickNote(note: QuickNoteUpdate): Promise<QuickNote> {
  return invokeNote("quick_notes_update", { note });
}

export function setQuickNotePinned(request: QuickNoteRevisionRequest, pinned: boolean): Promise<QuickNote> {
  return invokeNote("quick_notes_set_pinned", { request: { ...request, pinned } });
}

export async function reorderQuickNote(request: QuickNoteReorderRequest): Promise<void> {
  await invoke("quick_notes_reorder", { dbUrl: await ensureDbUrl(), request });
}

export function archiveQuickNote(request: QuickNoteRevisionRequest): Promise<QuickNote> {
  return invokeNote("quick_notes_archive", { request });
}

export function unarchiveQuickNote(request: QuickNoteRevisionRequest): Promise<QuickNote> {
  return invokeNote("quick_notes_unarchive", { request });
}

export function trashQuickNote(request: QuickNoteRevisionRequest): Promise<QuickNote> {
  return invokeNote("quick_notes_trash", { request });
}

export function restoreQuickNote(request: QuickNoteRevisionRequest): Promise<QuickNote> {
  return invokeNote("quick_notes_restore", { request });
}

export async function deleteQuickNotePermanently(id: string): Promise<void> {
  await invoke("quick_notes_delete_permanently", { dbUrl: await ensureDbUrl(), id });
}

export async function emptyQuickNotesTrash(): Promise<number> {
  return invoke<number>("quick_notes_empty_trash", { dbUrl: await ensureDbUrl() });
}

const QUICK_NOTE_IMAGE_ERROR_CODES = ["unsupported", "too_large", "unavailable", "failed"] as const;

function isQuickNoteImageErrorCode(value: unknown): value is QuickNoteImageErrorCode {
  return QUICK_NOTE_IMAGE_ERROR_CODES.some((code) => code === value);
}

/** A native image file failure with a stable code for user-facing messages. */
export class QuickNoteImageError extends Error {
  constructor(readonly code: QuickNoteImageErrorCode, message: string) {
    super(message);
    this.name = "QuickNoteImageError";
  }
}

/** Converts a native image error payload, or returns null when it has another shape. */
function imageErrorFrom(value: unknown): QuickNoteImageError | null {
  if (typeof value !== "object" || value === null || !("code" in value)) return null;
  if (!isQuickNoteImageErrorCode(value.code)) return null;
  const message = "message" in value && typeof value.message === "string" ? value.message : value.code;
  return new QuickNoteImageError(value.code, message);
}

async function invokeImage<T>(command: string, args?: Parameters<typeof invoke>[1]): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error: unknown) {
    throw imageErrorFrom(error) ?? error;
  }
}

/** Android IPC cannot carry raw request bodies, so its image imports travel as base64 text. */
function sendsBase64Bodies(): boolean {
  return typeof __GANBARU_BUILD_PLATFORM__ !== "undefined" && __GANBARU_BUILD_PLATFORM__ === "android";
}

const BASE64_CHUNK_BYTES = 0x8000;

function encodeBase64(bytes: Uint8Array): string {
  const parts: string[] = [];
  for (let start = 0; start < bytes.length; start += BASE64_CHUNK_BYTES) {
    parts.push(String.fromCharCode(...bytes.subarray(start, start + BASE64_CHUNK_BYTES)));
  }
  return btoa(parts.join(""));
}

/** Stores image bytes in the vault and returns the reference a note saves. */
export async function importQuickNoteImage(bytes: Uint8Array): Promise<QuickNoteImage> {
  const body = sendsBase64Bodies() ? { base64: encodeBase64(bytes) } : bytes;
  return mapQuickNoteImage(await invokeImage<unknown>("quick_notes_import_image", body));
}

function imageFile(image: QuickNoteImage): { sha256: string; contentType: QuickNoteImageContentType } {
  return { sha256: image.sha256, contentType: image.contentType };
}

/** Reads the verified bytes of an image; `unavailable` means this device does not hold them. */
export async function readQuickNoteImage(image: QuickNoteImage): Promise<ArrayBuffer> {
  const bytes = await invokeImage<unknown>("quick_notes_read_image", { file: imageFile(image) });
  if (!(bytes instanceof ArrayBuffer)) throw new Error("quick note image bytes must be an ArrayBuffer");
  return bytes;
}

/** Validates a picker result from the native side. */
export function mapQuickNoteImagePick(value: unknown): QuickNoteImagePick {
  const row = record(value, "quick note image pick");
  if (!Array.isArray(row.rejected)) throw new Error("quick note image pick.rejected must be an array");
  const skipped = integer(row.skipped, "quick note image pick.skipped");
  if (skipped < 0) throw new Error("quick note image pick.skipped is invalid");
  return {
    images: mapImages(row.images, "quick note image pick.images"),
    rejected: row.rejected.map((entry, index) => {
      const label = `quick note image pick.rejected[${index}]`;
      const rejection = record(entry, label);
      const error = imageErrorFrom(rejection.error);
      if (error === null) throw new Error(`${label}.error is invalid`);
      return { fileName: string(rejection.fileName, `${label}.fileName`), code: error.code };
    }),
    skipped,
  };
}

/** Opens the desktop image picker and imports up to `limit` picked files. */
export async function pickQuickNoteImages(limit: number): Promise<QuickNoteImagePick> {
  const bounded = Math.max(0, Math.min(Math.floor(limit), QUICK_NOTE_IMAGE_LIMIT));
  return mapQuickNoteImagePick(await invokeImage<unknown>("quick_notes_pick_images", { limit: bounded }));
}

/**
 * Imports up to `limit` of the files last dropped on this window. Rust reads the paths the operating system delivered
 * with the drop, so a drop is imported once and only within a minute of it.
 */
export async function importDroppedQuickNoteImages(limit: number): Promise<QuickNoteImagePick> {
  const bounded = Math.max(0, Math.min(Math.floor(limit), QUICK_NOTE_IMAGE_LIMIT));
  return mapQuickNoteImagePick(await invokeImage<unknown>("quick_notes_import_dropped_images", { limit: bounded }));
}

/**
 * Imports the image on the system clipboard, for pastes the webview hands over without it. Resolves an empty pick
 * when the clipboard holds no image.
 */
export async function pasteQuickNoteClipboardImage(): Promise<QuickNoteImagePick> {
  return mapQuickNoteImagePick(await invokeImage<unknown>("quick_notes_paste_clipboard_image"));
}

/** Saves a copy of an image outside the vault. Resolves `false` when the user cancels. */
export async function downloadQuickNoteImage(image: QuickNoteImage): Promise<boolean> {
  const saved = await invokeImage<unknown>("quick_notes_download_image", { file: imageFile(image) });
  return boolean(saved, "quick note image download");
}

const ORDER_KEY_PATTERN = /^[0-9A-Za-z]+$/u;

export function mapQuickNoteTag(value: unknown): QuickNoteTag {
  const row = record(value, "quick note tag");
  const id = string(row.id, "quick note tag.id");
  const name = string(row.name, "quick note tag.name");
  const orderKey = string(row.orderKey, "quick note tag.orderKey");
  if (id.length === 0 || id.length > 128) throw new Error("quick note tag.id is invalid");
  if (name !== name.trim() || name.length === 0 || [...name].length > QUICK_NOTE_TAG_NAME_MAX_CHARS) {
    throw new Error("quick note tag.name is invalid");
  }
  if (!ORDER_KEY_PATTERN.test(orderKey) || orderKey.length > QUICK_NOTE_ORDER_KEY_MAX_CHARS) {
    throw new Error("quick note tag.orderKey is invalid");
  }
  const color = normalizeEventColor(row.color);
  if (color === undefined) throw new Error("quick note tag.color is invalid");
  return {
    id,
    name,
    color,
    orderKey,
    createdAt: string(row.createdAt, "quick note tag.createdAt"),
    updatedAt: string(row.updatedAt, "quick note tag.updatedAt"),
  };
}

const QUICK_NOTE_TAG_ERROR_CODES = ["duplicate_name", "limit_reached", "not_found", "failed"] as const;

export type QuickNoteTagErrorCode = (typeof QUICK_NOTE_TAG_ERROR_CODES)[number];

/** A native tag write failure with a stable code for user-facing messages. */
export class QuickNoteTagError extends Error {
  constructor(readonly code: QuickNoteTagErrorCode, message: string) {
    super(message);
    this.name = "QuickNoteTagError";
  }
}

function isQuickNoteTagErrorCode(value: unknown): value is QuickNoteTagErrorCode {
  return QUICK_NOTE_TAG_ERROR_CODES.some((code) => code === value);
}

async function invokeTagWrite(command: string, args: Record<string, unknown>): Promise<unknown> {
  try {
    return await invoke<unknown>(command, { dbUrl: await ensureDbUrl(), ...args });
  } catch (error: unknown) {
    if (typeof error === "object" && error !== null && "code" in error && "message" in error
      && isQuickNoteTagErrorCode(error.code) && typeof error.message === "string") {
      throw new QuickNoteTagError(error.code, error.message);
    }
    throw error;
  }
}

export async function listQuickNoteTags(): Promise<QuickNoteTag[]> {
  const value = await invoke<unknown>("quick_notes_list_tags", { dbUrl: await ensureDbUrl() });
  if (!Array.isArray(value)) throw new Error("quick note tags must be an array");
  return value.map(mapQuickNoteTag);
}

export async function createQuickNoteTag(id: string, name: string, color: EventColor): Promise<QuickNoteTag> {
  return mapQuickNoteTag(await invokeTagWrite("quick_notes_create_tag", { tag: { id, name, color } }));
}

/** Renames and recolors a tag; notes keep the color they already have. */
export async function updateQuickNoteTag(id: string, name: string, color: EventColor): Promise<QuickNoteTag> {
  return mapQuickNoteTag(await invokeTagWrite("quick_notes_update_tag", { tag: { id, name, color } }));
}

/** Moves a tag between two adjacent neighbors; a null anchor means the start or the end. */
export async function reorderQuickNoteTag(request: QuickNoteTagReorderRequest): Promise<void> {
  await invokeTagWrite("quick_notes_reorder_tag", { request });
}

/** Deletes a tag and untags its notes; deleting a tag that no longer exists succeeds. */
export async function deleteQuickNoteTag(id: string): Promise<void> {
  await invokeTagWrite("quick_notes_delete_tag", { id });
}

export function mapQuickNotesTrashPurge(value: unknown): QuickNotesTrashPurge {
  const row = record(value, "quick notes trash purge");
  const purged = integer(row.purged, "quick notes trash purge.purged");
  if (purged < 0) throw new Error("quick notes trash purge.purged is invalid");
  return {
    purged,
    nextPurgeAt: nullableString(row.nextPurgeAt, "quick notes trash purge.nextPurgeAt"),
  };
}

/** Deletes trash past its retention when this device may write the vault. */
export async function purgeExpiredQuickNotesTrash(): Promise<QuickNotesTrashPurge> {
  return mapQuickNotesTrashPurge(await invoke<unknown>("quick_notes_purge_expired_trash", {
    dbUrl: await ensureDbUrl(),
  }));
}

/** A conflict read or resolution failure; `resolved` means the conflict or version is gone. */
export class QuickNoteConflictError extends Error {
  constructor(readonly code: "resolved" | "failed", message: string) {
    super(message);
    this.name = "QuickNoteConflictError";
  }
}

async function invokeConflict(command: string, args: Record<string, unknown>): Promise<unknown> {
  try {
    return await invoke<unknown>(command, { dbUrl: await ensureDbUrl(), ...args });
  } catch (error: unknown) {
    if (typeof error === "object" && error !== null && "code" in error && "message" in error
      && (error.code === "resolved" || error.code === "failed") && typeof error.message === "string") {
      throw new QuickNoteConflictError(error.code, error.message);
    }
    throw error;
  }
}

function conflictField(value: unknown, label: string): QuickNoteConflictField {
  if (value !== "title" && value !== "body" && value !== "images") throw new Error(`${label} is invalid`);
  return value;
}

function mapConflictVersion(value: unknown, label: string, field: QuickNoteConflictField): QuickNoteConflictVersion {
  const row = record(value, label);
  const editedAtMs = integer(row.editedAtMs, `${label}.editedAtMs`);
  if (editedAtMs < 0) throw new Error(`${label}.editedAtMs is invalid`);
  const title = nullableString(row.title, `${label}.title`);
  let runs: QuickNoteTextRun[] | null = null;
  if (row.runs !== null) {
    if (!Array.isArray(row.runs)) throw new Error(`${label}.runs must be an array`);
    runs = row.runs.map((run, index) => mapRun(run, `${label}.runs[${index}]`));
  }
  const images = row.images === null ? null : mapImages(row.images, `${label}.images`);
  if ((field === "title") !== (title !== null) || (field === "body") !== (runs !== null)
    || (field === "images") !== (images !== null)) {
    throw new Error(`${label} does not match its field`);
  }
  return {
    version: string(row.version, `${label}.version`),
    device: mapSyncDeviceRef(row.device, `${label}.device`),
    editedAtMs,
    displayed: boolean(row.displayed, `${label}.displayed`),
    title,
    runs,
    images,
  };
}

/** Validates the conflicts of a note from the native side. */
export function mapQuickNoteConflict(value: unknown): QuickNoteConflict {
  const row = record(value, "quick note conflict");
  if (!Array.isArray(row.groups)) throw new Error("quick note conflict.groups must be an array");
  const groups: QuickNoteConflictGroup[] = row.groups.map((entry, index) => {
    const label = `quick note conflict.groups[${index}]`;
    const group = record(entry, label);
    const field = conflictField(group.field, `${label}.field`);
    if (!Array.isArray(group.versions)) throw new Error(`${label}.versions must be an array`);
    return {
      field,
      versions: group.versions.map((version, position) =>
        mapConflictVersion(version, `${label}.versions[${position}]`, field)),
    };
  });
  return { id: string(row.id, "quick note conflict.id"), groups };
}

/** Reads the concurrent versions of a note's conflicted fields. */
export async function getQuickNoteConflict(id: string): Promise<QuickNoteConflict> {
  return mapQuickNoteConflict(await invokeConflict("quick_notes_conflict", { id }));
}

/** Resolves one conflicted field on every linked device. */
export async function resolveQuickNoteConflict(
  resolution: QuickNoteConflictResolution,
): Promise<QuickNoteConflictResolved> {
  const row = record(await invokeConflict("quick_notes_resolve_conflict", { resolution }), "quick note resolution");
  return {
    note: mapQuickNote(row.note),
    copy: row.copy === null ? null : mapQuickNote(row.copy),
  };
}
