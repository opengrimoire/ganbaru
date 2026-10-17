import { blockUpdateFromBlock } from "$lib/notes/blocks/factory";
import { parentIdForBlock } from "$lib/notes/blocks/tree";
import { parentIdsByDepth, type NotesUndoSnapshot } from "./undo-history";
import type { NotesBlock } from "$lib/notes/types";
import type { NotesEditOperation } from "$lib/api/notes/compound-edits";

function entryIdsByPresence(
  target: NotesUndoSnapshot,
  source: NotesUndoSnapshot,
): { targetOnlyRoots: NotesBlock[]; sourceOnlyRoots: NotesBlock[] } {
  const targetIds = new Set(target.blocks.map((block) => block.id));
  const sourceIds = new Set(source.blocks.map((block) => block.id));
  const targetOnlyIds = new Set([...targetIds].filter((blockId) => !sourceIds.has(blockId)));
  const sourceOnlyIds = new Set([...sourceIds].filter((blockId) => !targetIds.has(blockId)));
  return {
    targetOnlyRoots: target.blocks.filter((block) => {
      if (!targetOnlyIds.has(block.id)) return false;
      return !targetOnlyIds.has(parentIdForBlock(block));
    }),
    sourceOnlyRoots: source.blocks.filter((block) => {
      if (!sourceOnlyIds.has(block.id)) return false;
      return !sourceOnlyIds.has(parentIdForBlock(block));
    }),
  };
}

function snapshotBlocksById(snapshot: NotesUndoSnapshot): Map<string, NotesBlock> {
  return new Map(snapshot.blocks.map((block) => [block.id, block]));
}

/** Plan only the historical payloads and placements changed by an undo boundary. */
export function notesUndoSnapshotOperations(
  target: NotesUndoSnapshot,
  source: NotesUndoSnapshot,
): NotesEditOperation[] {
  const operations: NotesEditOperation[] = [];
  const conversionsAfterRemoval: NotesEditOperation[] = [];
  const { targetOnlyRoots, sourceOnlyRoots } = entryIdsByPresence(target, source);
  const sourceById = snapshotBlocksById(source);
  for (const block of targetOnlyRoots) {
    operations.push({ type: "trash", block_id: block.id, in_trash: false });
  }

  for (const block of target.blocks) {
    const update = blockUpdateFromBlock(block);
    const previous = sourceById.get(block.id);
    if (!previous || JSON.stringify(update) !== JSON.stringify(blockUpdateFromBlock(previous))) {
      const destination = previous && previous.type !== block.type
        && (previous.type === "table" || previous.type === "column_list" || previous.type === "tab")
        ? conversionsAfterRemoval : operations;
      destination.push({ type: "update", block_id: block.id, update });
    }
  }

  const targetById = snapshotBlocksById(target);
  const removedIds = new Set(source.blocks.filter((block) => !targetById.has(block.id)).map((block) => block.id));
  const placementState = Object.fromEntries(Object.entries(source.childIdsByParentId).map(([id, children]) => [id, children.filter((child) => !removedIds.has(child))]));
  for (const parentId of parentIdsByDepth(target)) {
    if (JSON.stringify(target.childIdsByParentId[parentId]) === JSON.stringify(source.childIdsByParentId[parentId])) continue;
    let before: string | null = null;
    const childIds = [...(target.childIdsByParentId[parentId] ?? [])]
      .reverse();
    for (const childId of childIds) {
      const block = targetById.get(childId);
      if (!block) { before = childId; continue; }
      const siblings = placementState[parentId] ?? [];
      const index = siblings.indexOf(childId);
      if (index < 0 || (siblings[index + 1] ?? null) !== before) {
        const previous = sourceById.get(childId);
        const sourcePageId = previous?.parent.type === "page_id" ? previous.parent.page_id : source.pageId;
        const destinationPageId = block.parent.type === "page_id" ? block.parent.page_id : target.pageId;
        const request = { parent: block.parent, after: null, before };
        operations.push(sourcePageId === destinationPageId
          ? { type: "move", block_id: childId, request }
          : { type: "move_between_pages", block_id: childId, source_page_id: sourcePageId, destination_page_id: destinationPageId, request });
        for (const id of Object.keys(placementState)) placementState[id] = placementState[id].filter((candidate) => candidate !== childId);
        const next = placementState[parentId] ?? [];
        const beforeIndex = before ? next.indexOf(before) : -1;
        next.splice(beforeIndex < 0 ? next.length : beforeIndex, 0, childId);
        placementState[parentId] = next;
      }
      before = childId;
    }
  }
  // Move surviving descendants before trashing a removed parent.
  for (const block of sourceOnlyRoots) {
    operations.push({ type: "trash", block_id: block.id, in_trash: true });
  }
  operations.push(...conversionsAfterRemoval);
  return operations;
}
