import { normalizeRichTextLinkUrl } from "$lib/notes/rich-text/core";
import { parseNotesLinkHash } from "$lib/notes/links/block-link";

/** Clipboard-relative paths have no shared base URL and must never become invented web links. */
export function notesClipboardLinkUrl(value: string | null | undefined): string | null {
  if (value?.startsWith("#notes?") && parseNotesLinkHash(value)) return value;
  if (!value || !/^(?:https?:|mailto:)/iu.test(value.trim())) return null;
  return normalizeRichTextLinkUrl(value);
}
