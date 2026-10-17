import { ensureDbUrl } from "$lib/api/db";
import { invalidateAssetUrlKind } from "$lib/api/asset-url-cache";
import { invalidateNotesNotificationSchedule } from "$lib/notes/collaboration/notification-schedule.svelte";
import { mapNotesBlockDto, mapNotesCreatedDatabaseDto } from "$lib/notes/validation/response-mappers";
import type { NotesAppendBlockChildrenRequest, NotesBlock, NotesBlockUpdate, NotesCreatedDatabase, NotesDatabaseCreateRequest, NotesDatabaseDuplicateRequest, NotesDuplicateBlocksRequest, NotesLinkedDatabaseCreateRequest, NotesMoveBlockRequest, NotesParent } from "$lib/notes/types";
import { invokeNotesMutation } from "./mutation";
import type { NotesBlockPlacement } from "$lib/notes/post-mutation";

export type NotesEditKind = "split" | "merge" | "replace_selection" | "format_selection" | "paste" | "convert" | "table_columns" | "column_layout" | "tab_layout" | "undo" | "redo" | "delete_selection" | "indent_selection" | "template";

export type NotesEditOperation =
  | { type: "append"; request: NotesAppendBlockChildrenRequest }
  | { type: "update"; block_id: string; update: NotesBlockUpdate }
  | { type: "move"; block_id: string; request: NotesMoveBlockRequest }
  | { type: "move_between_pages"; block_id: string; source_page_id: string; destination_page_id: string; request: NotesMoveBlockRequest }
  | { type: "trash"; block_id: string; in_trash: boolean }
  | { type: "duplicate"; request: NotesDuplicateBlocksRequest }
  | { type: "duplicate_children"; source_block_id: string; request: NotesDuplicateBlocksRequest }
  | { type: "create_database"; request: NotesDatabaseCreateRequest }
  | { type: "copy_database"; request: NotesDatabaseDuplicateRequest }
  | { type: "link_database"; request: NotesLinkedDatabaseCreateRequest }
  | { type: "move_children"; source_block_id: string; parent: NotesParent; after: string | null }
  | { type: "table_column"; table_id: string; index: number; insert: boolean; empty_row_id: string };

export interface NotesCompoundEdit {
  operation_id: string;
  page_id: string;
  kind: NotesEditKind;
  expected_blocks: Record<string, string>;
  operations: NotesEditOperation[];
}

export interface NotesCompoundEditResult {
  operation_id: string;
  page_id: string;
  blocks: NotesBlock[];
  databases: NotesCreatedDatabase[];
  placements: NotesBlockPlacement[];
  before_blocks?: NotesBlock[];
  before_placements?: NotesBlockPlacement[];
}

/** Commit one immutable Notes editor action and decode its canonical receipt. */
export async function applyNotesCompoundEdit(request: NotesCompoundEdit): Promise<NotesCompoundEditResult> {
  const value = await invokeNotesMutation("notes_apply_compound_edit", { dbUrl: await ensureDbUrl(), request });
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("Invalid Notes edit result");
  const result = value as Record<string, unknown>;
  if (result.operation_id !== request.operation_id || result.page_id !== request.page_id
    || !Array.isArray(result.blocks) || !Array.isArray(result.databases) || !Array.isArray(result.placements)) throw new Error("Invalid Notes edit receipt");
  const blocks = result.blocks.map(mapNotesBlockDto);
  const databases = result.databases.map(mapNotesCreatedDatabaseDto);
  const byId = new Map(blocks.map((block) => [block.id, block]));
  const mapPlacements = (values: unknown[], blockMap: ReadonlyMap<string, NotesBlock>): NotesBlockPlacement[] => values.map((value: unknown): NotesBlockPlacement => {
    if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("Invalid Notes edit placement");
    const placement = value as Record<string, unknown>;
    if (typeof placement.block_id !== "string" || (placement.after !== null && typeof placement.after !== "string") || (placement.before !== null && typeof placement.before !== "string")) throw new Error("Invalid Notes edit placement");
    const block = blockMap.get(placement.block_id);
    if (!block) throw new Error("Notes edit placement has no block");
    return { blockId: block.id, parent: block.parent, after: placement.after, before: placement.before };
  });
  const placements = mapPlacements(result.placements, byId);
  if (!Array.isArray(result.before_blocks) || !Array.isArray(result.before_placements)) throw new Error("Notes edit receipt has no canonical preimage");
  const beforeBlocks = result.before_blocks.map(mapNotesBlockDto);
  const beforePlacements = mapPlacements(result.before_placements, new Map(beforeBlocks.map((block) => [block.id, block])));
  invalidateNotesNotificationSchedule();
  if (request.operations.some((operation) => operation.type === "trash" && operation.in_trash)) invalidateAssetUrlKind("notes-file");
  return { operation_id: request.operation_id, page_id: request.page_id, blocks, databases, placements, before_blocks: beforeBlocks, before_placements: beforePlacements };
}
