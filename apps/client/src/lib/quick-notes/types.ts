import type { EventColor } from "$lib/calendar/types";
import type { SyncDeviceRef } from "$lib/api/sync";

export const QUICK_NOTE_TITLE_MAX_CHARS = 200;
export const QUICK_NOTE_BODY_MAX_CHARS = 65_536;
/** Local create limit; synchronized devices can hold more tags. */
export const QUICK_NOTE_TAG_LIMIT = 9;
export const QUICK_NOTE_TAG_NAME_MAX_CHARS = 40;
export const QUICK_NOTE_ORDER_KEY_MAX_CHARS = 128;
export const QUICK_NOTE_IMAGE_LIMIT = 24;
export const QUICK_NOTE_IMAGE_MAX_BYTES = 25 * 1024 * 1024;
export const QUICK_NOTE_IMAGE_CONTENT_TYPES = Object.freeze([
  "image/png",
  "image/jpeg",
  "image/webp",
  "image/gif",
] as const);

export type QuickNoteImageContentType = (typeof QUICK_NOTE_IMAGE_CONTENT_TYPES)[number];

/** A note's reference to a content-addressed image file in the vault. */
export interface QuickNoteImage {
  /** Lowercase hex SHA-256 of the file bytes. */
  sha256: string;
  contentType: QuickNoteImageContentType;
  byteSize: number;
  /** Displayed width in pixels, after the image orientation is applied. */
  width: number;
  /** Displayed height in pixels, after the image orientation is applied. */
  height: number;
}

/** Images a picker imported, and the picked files that were left out. */
export interface QuickNoteImagePick {
  images: QuickNoteImage[];
  rejected: { fileName: string; code: QuickNoteImageErrorCode }[];
  /** Picked files past the limit that were not imported. */
  skipped: number;
}

export type QuickNoteImageErrorCode = "unsupported" | "too_large" | "unavailable" | "failed";

export interface QuickNoteTextRun {
  content: string;
  bold: boolean;
  italic: boolean;
  underline: boolean;
}

export interface QuickNote {
  id: string;
  title: string;
  bodyPlainText: string;
  runs: QuickNoteTextRun[];
  previewTruncated: boolean;
  images: QuickNoteImage[];
  color: EventColor;
  tagId: string | null;
  pinned: boolean;
  archived: boolean;
  trashedAt: string | null;
  revision: number;
  createdAt: string;
  updatedAt: string;
  /** Whether a linked device wrote a concurrent title, body, or image list that is still unresolved. */
  hasConflict: boolean;
}

export type QuickNotesCollection = "active" | "archive" | "trash";

export interface QuickNotesWindow {
  notes: QuickNote[];
  nextCursor: string | null;
}

export interface QuickNoteCreate {
  id: string;
  title: string;
  runs: QuickNoteTextRun[];
  images: QuickNoteImage[];
  color: EventColor;
  tagId: string | null;
  pinned: boolean;
}

export interface QuickNoteTag {
  id: string;
  name: string;
  /** Palette slot that notes take when they are tagged. */
  color: EventColor;
  orderKey: string;
  createdAt: string;
  updatedAt: string;
}

/** Result of one trash purge pass. */
export interface QuickNotesTrashPurge {
  purged: number;
  /** When the oldest remaining trashed note expires; null when nothing is due. */
  nextPurgeAt: string | null;
}

export interface QuickNoteUpdate extends QuickNoteCreate {
  expectedRevision: number;
}

export interface QuickNoteRevisionRequest {
  id: string;
  expectedRevision: number;
}

export interface QuickNoteReorderRequest {
  id: string;
  previousId: string | null;
  nextId: string | null;
  tagId: string | null;
}

export interface QuickNoteTagReorderRequest {
  id: string;
  previousId: string | null;
  nextId: string | null;
}

export interface QuickNoteFormatting {
  bold: boolean;
  italic: boolean;
  underline: boolean;
}

export type QuickNoteFormattingName = keyof QuickNoteFormatting;

export const EMPTY_QUICK_NOTE_FORMATTING: QuickNoteFormatting = Object.freeze({
  bold: false,
  italic: false,
  underline: false,
});

/** A note field that can hold a sync conflict. */
export type QuickNoteConflictField = "title" | "body" | "images";

/** One concurrent version of a field in conflict. */
export interface QuickNoteConflictVersion {
  /** Opaque version id for resolving. */
  version: string;
  device: SyncDeviceRef;
  editedAtMs: number;
  /** Whether this version is the one every device shows. */
  displayed: boolean;
  title: string | null;
  runs: QuickNoteTextRun[] | null;
  images: QuickNoteImage[] | null;
}

/** The versions of one field in conflict, newest first. */
export interface QuickNoteConflictGroup {
  field: QuickNoteConflictField;
  versions: QuickNoteConflictVersion[];
}

/** The conflicts of a note; `groups` is empty when it has none. */
export interface QuickNoteConflict {
  id: string;
  groups: QuickNoteConflictGroup[];
}

export type QuickNoteConflictChoice =
  | { kind: "displayed" }
  | { kind: "version"; version: string }
  | { kind: "keep_both"; version: string };

export interface QuickNoteConflictResolution {
  id: string;
  field: QuickNoteConflictField;
  choice: QuickNoteConflictChoice;
}

/** The note after a resolution, and the note created to keep both versions. */
export interface QuickNoteConflictResolved {
  note: QuickNote;
  copy: QuickNote | null;
}
