import type { MediaFolderSelection } from "$lib/api/music";

export type MusicAddSourceKind = "local-root" | "youtube";

export interface MusicLocalSourceSelection {
  selection: MediaFolderSelection;
  name: string;
  relationship: ReturnType<typeof musicFolderRelationship>;
}

export function musicFolderDisplayName(folderPath: string): string {
  const normalized = folderPath.trim().replace(/[\\/]+$/, "");
  const segments = normalized.split(/[\\/]/).filter(Boolean);
  return segments.at(-1)?.trim() || "Music";
}

export function normalizeComparableFolderPath(folderPath: string): string {
  return folderPath.trim().replace(/\\/g, "/").replace(/\/+$/, "").toLocaleLowerCase();
}

export function musicFolderRelationship(
  candidatePath: string,
  existingPaths: readonly string[],
): "duplicate" | "nested" | "contains-existing" | "separate" {
  const candidate = normalizeComparableFolderPath(candidatePath);
  for (const existingPath of existingPaths) {
    const existing = normalizeComparableFolderPath(existingPath);
    if (candidate === existing) return "duplicate";
    if (candidate.startsWith(`${existing}/`)) return "nested";
    if (existing.startsWith(`${candidate}/`)) return "contains-existing";
  }
  return "separate";
}
