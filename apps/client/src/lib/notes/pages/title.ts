import { richTextPlainText } from "$lib/notes/blocks/factory";
import { parseNotesRichTextArray } from "$lib/notes/blocks/validation";
import type { NotesPage } from "$lib/notes/types";

export function notesPageTitle(page: NotesPage, fallback = ""): string {
  const title = page.properties.title;
  if (typeof title !== "object" || title === null || Array.isArray(title)) return fallback;
  if (!Object.hasOwn(title, "title")) return fallback;
  const titleRecord = title as Record<string, unknown>;
  try {
    return richTextPlainText(parseNotesRichTextArray(titleRecord.title, "page.properties.title.title"))
      .trim() || fallback;
  } catch {
    return fallback;
  }
}
