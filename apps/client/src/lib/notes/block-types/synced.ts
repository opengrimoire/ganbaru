import type { NotesSyncedBlockPayload } from "$lib/notes/types";

export type NotesSyncedBlockRole = "original" | "duplicate";

export interface NotesSyncedBlockStatus {
  role: NotesSyncedBlockRole;
  sourceBlockId: string | null;
  canOwnChildren: boolean;
  canEditLoadedChildren: boolean;
  canEditSyncedCopies: false;
}

/** Summarize the local editing capabilities for a Notion-shaped synced block payload. */
export function notesSyncedBlockStatus(
  payload: NotesSyncedBlockPayload,
): NotesSyncedBlockStatus {
  if (payload.synced_from === null) {
    return {
      role: "original",
      sourceBlockId: null,
      canOwnChildren: true,
      canEditLoadedChildren: true,
      canEditSyncedCopies: false,
    };
  }
  return {
    role: "duplicate",
    sourceBlockId: payload.synced_from.block_id,
    canOwnChildren: false,
    canEditLoadedChildren: false,
    canEditSyncedCopies: false,
  };
}
