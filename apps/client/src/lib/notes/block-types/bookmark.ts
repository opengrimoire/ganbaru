import { richTextPlainText } from "$lib/notes/blocks/factory";
import type { NotesBookmarkBlockPayload } from "$lib/notes/types";

export function bookmarkCaptionPlainText(bookmark: NotesBookmarkBlockPayload): string {
  return richTextPlainText(bookmark.caption);
}

export function canOpenBookmarkUrl(url: string): boolean {
  const trimmed = url.trim();
  if (!trimmed) return false;
  try {
    const parsed = new URL(trimmed);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}
