import type { NotesCompoundEditResult } from "$lib/api/notes/compound-edits";
import type { NotesBlock } from "$lib/notes/types";
import type { NotesBlockPlacement } from "$lib/notes/post-mutation";
import { applyNotesPostMutationToTree } from "$lib/notes/post-mutation";
import type { NotesUndoSnapshot } from "./undo-history";

/** Complete one historical boundary with the canonical rows touched by its native edit. */
export function reconcileNotesUndoSnapshot(
  snapshot: NotesUndoSnapshot | null,
  blocks: readonly NotesBlock[],
  placements: readonly NotesBlockPlacement[],
  readRevision?: (id: string) => string | undefined,
): void {
  if (!snapshot) return;
  const visible = blocks.filter((block) => !block.in_trash);
  const state = applyNotesPostMutationToTree({
    blocksById: Object.fromEntries(snapshot.blocks.map((block) => [block.id, block])),
    childIdsByParentId: snapshot.childIdsByParentId,
  }, {
    blocks: visible.map((block) => ({ ...block, edit_revision: readRevision?.(block.id) ?? block.edit_revision })),
    placements: [...placements],
    removedBlockIds: blocks.filter((block) => block.in_trash).map((block) => block.id),
  });
  snapshot.blocks = Object.values(state.blocksById);
  snapshot.childIdsByParentId = Object.fromEntries(Object.entries(state.childIdsByParentId).map(([id, children]) => [id, [...children]]));
}

/** Enrich the exact before and after boundaries without restoring an entire page. */
export function reconcileNotesCompoundUndo(
  before: NotesUndoSnapshot | null,
  after: NotesUndoSnapshot | null,
  result: NotesCompoundEditResult,
  readRevision?: (id: string) => string | undefined,
): void {
  const revisions = new Map(result.blocks.map((block) => [block.id, block.edit_revision]));
  const revision = (id: string) => readRevision?.(id) ?? revisions.get(id);
  reconcileNotesUndoSnapshot(before, result.before_blocks ?? [], result.before_placements ?? [], revision);
  reconcileNotesUndoSnapshot(after, result.blocks, result.placements, revision);
}
