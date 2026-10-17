import type { NotesEmbedBlockPayload } from "$lib/notes/types";

export function embedUrlPlainText(embed: NotesEmbedBlockPayload): string {
  return embed.url;
}

export function canOpenEmbedUrl(url: string): boolean {
  const trimmed = url.trim();
  if (!trimmed) return false;
  try {
    const parsed = new URL(trimmed);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

export function embedDisplayTitle(url: string): string {
  const trimmed = url.trim();
  if (!trimmed) return "";
  try {
    const parsed = new URL(trimmed);
    return parsed.hostname || trimmed;
  } catch {
    return trimmed;
  }
}
