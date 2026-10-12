import type { EventColor } from "$lib/calendar/types";
import type { SyncDeviceRef } from "$lib/api/sync";

export const QUICK_NOTE_TITLE_MAX_CHARS = 200;
export const QUICK_NOTE_BODY_MAX_CHARS = 65_536;
/** Local create limit; synchronized devices can hold more tags. */
export const QUICK_NOTE_TAG_LIMIT = 9;
export const QUICK_NOTE_TAG_NAME_MAX_CHARS = 40;
export const QUICK_NOTE_ORDER_KEY_MAX_CHARS = 128;

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
  color: EventColor;
  tagId: string | null;
  pinned: boolean;
  archived: boolean;
  trashedAt: string | null;
  revision: number;
  createdAt: string;
  updatedAt: string;
  /** Whether a linked device wrote a concurrent title or body that is still unresolved. */
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
  color: EventColor;
  tagId: string | null;
  pinned: boolean;
}

export interface QuickNoteTag {
  id: string;
  name: string;
  orderKey: string;
  createdAt: string;
  updatedAt: string;
}

/** Result of one trash purge pass. */
export interface QuickNotesTrashPurge {
  purged: number;
  /** When the oldest remaining trashed note expires; null when nothing is due or this device cannot purge. */
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
export type QuickNoteConflictField = "title" | "body";

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
