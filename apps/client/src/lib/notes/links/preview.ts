import type { NotesLinkPreviewBlockPayload } from "$lib/notes/types";

export function linkPreviewUrlPlainText(linkPreview: NotesLinkPreviewBlockPayload): string {
  return linkPreview.url;
}

export function canOpenLinkPreviewUrl(url: string): boolean {
  const trimmed = url.trim();
  if (!trimmed) return false;
  try {
    const parsed = new URL(trimmed);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

export function linkPreviewDisplayTitle(url: string): string {
  const trimmed = url.trim();
  if (!trimmed) return "";
  try {
    const parsed = new URL(trimmed);
    return parsed.hostname || trimmed;
  } catch {
    return trimmed;
  }
}

export function linkPreviewDisplaySource(url: string): string {
  const trimmed = url.trim();
  if (!trimmed) return "";
  try {
    const parsed = new URL(trimmed);
    const path = `${parsed.pathname}${parsed.search}`.replace(/\/$/, "");
    return path ? `${parsed.hostname}${path}` : parsed.hostname;
  } catch {
    return trimmed;
  }
}
