import { QuickNoteImageError } from "$lib/api/quick-notes";
import {
  QUICK_NOTE_IMAGE_CONTENT_TYPES,
  QUICK_NOTE_IMAGE_LIMIT,
  QUICK_NOTE_IMAGE_MAX_BYTES,
  type QuickNoteImage,
  type QuickNoteImageErrorCode,
  type QuickNoteImagePick,
} from "$lib/quick-notes/types";

/** File types the image file input offers. */
export const QUICK_NOTE_IMAGE_FILE_ACCEPT = QUICK_NOTE_IMAGE_CONTENT_TYPES.join(",");

/** Images a note can still take while `pending` imports are running. */
export function quickNoteImageSlots(current: number, pending: number): number {
  return Math.max(0, QUICK_NOTE_IMAGE_LIMIT - current - pending);
}

/** Adds imported images after the current ones, leaving out images the note already has and any past the limit. */
export function appendQuickNoteImages(
  current: readonly QuickNoteImage[],
  added: readonly QuickNoteImage[],
): QuickNoteImage[] {
  const next = [...current];
  const seen = new Set(current.map((image) => image.sha256));
  for (const image of added) {
    if (next.length >= QUICK_NOTE_IMAGE_LIMIT) break;
    if (seen.has(image.sha256)) continue;
    seen.add(image.sha256);
    next.push(image);
  }
  return next;
}

/** Puts a removed image back at its old position, unless the note has it again or is full. */
export function restoreQuickNoteImage(
  current: readonly QuickNoteImage[],
  image: QuickNoteImage,
  index: number,
): QuickNoteImage[] {
  if (current.length >= QUICK_NOTE_IMAGE_LIMIT || current.some((existing) => existing.sha256 === image.sha256)) {
    return [...current];
  }
  const next = [...current];
  next.splice(Math.min(Math.max(0, index), next.length), 0, image);
  return next;
}

/** Rejects files the native side would refuse before reading their bytes. */
function precheck(file: File): QuickNoteImageErrorCode | null {
  if (file.size > QUICK_NOTE_IMAGE_MAX_BYTES) return "too_large";
  if (file.type && !(QUICK_NOTE_IMAGE_CONTENT_TYPES as readonly string[]).includes(file.type)) return "unsupported";
  return null;
}

/**
 * Imports pasted, dropped, or chosen files one at a time, in order, up to `slots` of them.
 *
 * @param importer Stores the bytes in the vault and returns the reference a note saves.
 */
export async function importQuickNoteImageFiles(
  files: readonly File[],
  slots: number,
  importer: (bytes: Uint8Array) => Promise<QuickNoteImage>,
): Promise<QuickNoteImagePick> {
  const limit = Math.max(0, Math.min(slots, files.length));
  const pick: QuickNoteImagePick = { images: [], rejected: [], skipped: files.length - limit };
  for (const file of files.slice(0, limit)) {
    const early = precheck(file);
    if (early) {
      pick.rejected.push({ fileName: file.name, code: early });
      continue;
    }
    try {
      pick.images.push(await importer(new Uint8Array(await file.arrayBuffer())));
    } catch (error: unknown) {
      if (!(error instanceof QuickNoteImageError)) console.warn("quick note image could not be imported", error);
      pick.rejected.push({
        fileName: file.name,
        code: error instanceof QuickNoteImageError ? error.code : "failed",
      });
    }
  }
  return pick;
}
