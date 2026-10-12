import type { MusicInterchangeImportResult } from "./contracts";

export type MusicTransferFormat = "json" | "m3u8";

/** Bounded selected file input. Native code owns decoding, matching, and persistence. */
export interface MusicTransferSource {
  contents: string;
  playlistName: string;
  relativeRootId: string | null;
  selectedAtMs: number;
}

/** A reviewed import action; preserve the complete request when retrying a lost response. */
export interface MusicTransferCommit {
  actionId: string;
  source: MusicTransferSource;
  expectedRevision: string;
  playlistConflict: "keep-existing" | "import-copy" | "replace-existing";
  replaceItemDescriptions: boolean;
  importContextAssignments: boolean;
}

export interface MusicTransferPreview {
  format: MusicTransferFormat;
  revision: string;
  roots: Array<{ id: string; name: string }>;
  availableRoots: Array<{ id: string; name: string }>;
  boundRootIds: string[];
  contextAssignmentCount: number;
  newPlaylists: number;
  matchedPlaylists: number;
  newItems: number;
  matchedItems: number;
  duplicateItems: number;
  missingLocalBindings: number;
  conflicts: string[];
  unsupported: string[];
  localCount: number;
  youtubeCount: number;
  unsupportedCount: number;
  unresolvedLocalCount: number;
}

function record(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("Music transfer response must be an object");
  return value as Record<string, unknown>;
}

function text(value: unknown): string {
  if (typeof value !== "string" || value.length > 8192) throw new Error("Invalid Music transfer text");
  return value;
}

function count(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0 || value > 100000) throw new Error("Invalid Music transfer count");
  return value;
}

function array<T>(value: unknown, parse: (value: unknown) => T, limit = 10000): T[] {
  if (!Array.isArray(value) || value.length > limit) throw new Error("Invalid Music transfer list");
  return value.map(parse);
}

function roots(value: unknown): Array<{ id: string; name: string }> {
  return array(value, (entry) => {
    const row = record(entry);
    return { id: text(row.id), name: text(row.name) };
  }, 500);
}

/** Validates the compact native preview at the IPC boundary. */
export function parseMusicTransferPreview(value: unknown): MusicTransferPreview {
  const row = record(value);
  if (row.format !== "json" && row.format !== "m3u8") throw new Error("Invalid Music transfer format");
  const revision = text(row.revision);
  if (!/^[a-f0-9]{64}$/.test(revision)) throw new Error("Invalid Music transfer revision");
  return {
    format: row.format, revision, roots: roots(row.roots), availableRoots: roots(row.availableRoots),
    boundRootIds: array(row.boundRootIds, text, 500),
    contextAssignmentCount: count(row.contextAssignmentCount), newPlaylists: count(row.newPlaylists),
    matchedPlaylists: count(row.matchedPlaylists), newItems: count(row.newItems), matchedItems: count(row.matchedItems),
    duplicateItems: count(row.duplicateItems), missingLocalBindings: count(row.missingLocalBindings),
    conflicts: array(row.conflicts, text, 500), unsupported: array(row.unsupported, text, 200),
    localCount: count(row.localCount), youtubeCount: count(row.youtubeCount),
    unsupportedCount: count(row.unsupportedCount), unresolvedLocalCount: count(row.unresolvedLocalCount),
  };
}

/** Validates a committed import receipt without trusting an unchecked response cast. */
export function parseMusicTransferResult(value: unknown): MusicInterchangeImportResult {
  const row = record(value);
  return { playlistCount: count(row.playlistCount), itemCount: count(row.itemCount), membershipCount: count(row.membershipCount), assignmentCount: count(row.assignmentCount) };
}
