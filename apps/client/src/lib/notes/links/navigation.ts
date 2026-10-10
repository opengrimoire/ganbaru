import { normalizeRichTextLinkUrl, notesPastedLinkUrl } from "$lib/notes/rich-text/core";
import { parseNotesLinkHash, type NotesPageLinkTarget } from "./block-link";

/** Resolve local note links, including links copied from this app's current origin. */
export function notesLinkTarget(value: string, baseHref?: string): NotesPageLinkTarget | null {
  if (value.startsWith("#")) return parseNotesLinkHash(value);
  if (!baseHref) return null;
  try {
    const link = new URL(value);
    const base = new URL(baseHref);
    return link.origin === base.origin && link.protocol === base.protocol && link.host === base.host
      ? parseNotesLinkHash(link.hash) : null;
  } catch { return null; }
}

/** Keep copied app URLs portable within this vault instead of storing a webview origin. */
export function normalizeNotesTextLinkUrl(value: string, baseHref?: string): string | null {
  const target = notesLinkTarget(value.trim(), baseHref);
  if (!target) return normalizeRichTextLinkUrl(value);
  const params = new URLSearchParams({ page: target.pageId });
  if (target.blockId) params.set("block", target.blockId);
  if (target.viewId) params.set("view", target.viewId);
  return `#notes?${params}`;
}

/** Recognize a pasted URL or a valid link copied from this app's current origin. */
export function notesPastedTextLinkUrl(value: string, baseHref?: string): string | null {
  return notesLinkTarget(value.trim(), baseHref) ? normalizeNotesTextLinkUrl(value, baseHref) : notesPastedLinkUrl(value);
}

/** Open local links through normal Notes navigation and web or email links through the system. */
export async function openNotesTextLink(value: string): Promise<void> {
  const target = notesLinkTarget(value, typeof window === "undefined" ? undefined : window.location.href);
  if (target) {
    const { getNotes } = await import("$lib/stores/notes.svelte");
    if (!await getNotes().openNotesLink(target)) throw new Error("Notes link target is unavailable");
    return;
  }
  const url = normalizeRichTextLinkUrl(value);
  if (!url || url.startsWith("#")) throw new Error("Notes link destination is invalid");
  const { openNotesExternalUrl } = await import("$lib/api/notes/knowledge");
  await openNotesExternalUrl(url);
}
