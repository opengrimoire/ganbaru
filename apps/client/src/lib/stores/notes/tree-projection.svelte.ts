import { blockIndent } from "$lib/notes/blocks/queries";
import { flattenNotesBlockOutlines, notesBlockOutlineFromBlock, type NotesBlockOutlineItem } from "$lib/notes/blocks/outline";
import { blockChildrenAreVisible, buildNotesChildIdsByParent, parentIdForBlock, type NotesTreeState } from "$lib/notes/blocks/tree";
import type { NotesUndoSnapshot } from "$lib/notes/history/undo-history";
import { notesUndoSnapshotOperations } from "$lib/notes/history/undo-operations";
import { applyNotesPostMutationToTree, type NotesPostMutationResult } from "$lib/notes/post-mutation";
import { notesPageTitle } from "$lib/notes/pages/title";
import type { NotesBlock, NotesBlockOutline, NotesLoadedPage, NotesPage } from "$lib/notes/types";
import { notesTreeState, notesTreeStateWithoutLeafBlock, type NotesBlockTreeSnapshot } from "./block-tree";

export interface NotesTreeProjectionOptions {
  readSelectedPageId: () => string | null;
}

/** Owns the reactive projection of the currently loaded Notes block tree. */
export class NotesTreeProjectionController {
  loadedPage = $state<NotesPage | null>(null);
  blocksById = $state<Record<string, NotesBlock>>({});
  childIdsByParentId = $state<Record<string, string[]>>({});
  blockOutlines = $state<NotesBlockOutline[]>([]);
  flatBlockOutlines = $state<NotesBlockOutlineItem[]>([]);
  primaryContentReady = $state(false);

  private markLocallyChanged: (blockId: string) => void = () => {};
  private hiddenChildBlockIds = new Set<string>();

  constructor(private readonly options: NotesTreeProjectionOptions) {}

  setLocalChangeMarker(marker: (blockId: string) => void): void {
    this.markLocallyChanged = marker;
  }

  snapshot(): NotesBlockTreeSnapshot {
    return {
      selectedPageId: this.options.readSelectedPageId(),
      blocksById: this.blocksById,
      childIdsByParentId: this.childIdsByParentId,
    };
  }

  treeState(): NotesTreeState {
    return notesTreeState(this.snapshot());
  }

  replaceBlock(block: NotesBlock): void {
    const previous = this.blocksById[block.id];
    this.blocksById = { ...this.blocksById, [block.id]: block };
    this.rememberChildVisibility(block);
    // Type, indentation, and collapsed state affect outline rendering without changing sibling order.
    if (previous && (previous.type !== block.type || blockIndent(previous) !== blockIndent(block)
      || blockChildrenAreVisible(previous) !== blockChildrenAreVisible(block))) this.syncLocalOutlines();
  }

  applyLocalUndoSnapshot(target: NotesUndoSnapshot, source: NotesUndoSnapshot): void {
    const targetIds = new Set(target.blocks.map((block) => block.id));
    const sourceIds = new Set(source.blocks.map((block) => block.id));
    const operations = notesUndoSnapshotOperations(target, source);
    const payloadIds = new Set(operations.flatMap((operation) => operation.type === "update" ? [operation.block_id] : []));
    const placementIds = new Set([
      ...operations.flatMap((operation) => operation.type === "move" || operation.type === "move_between_pages" ? [operation.block_id] : []),
      ...[...targetIds].filter((id) => !sourceIds.has(id)),
      ...[...sourceIds].filter((id) => !targetIds.has(id)),
    ]);
    const affectedIds = new Set([...payloadIds, ...placementIds]);
    const nextBlocksById = { ...this.blocksById };
    for (const blockId of sourceIds) {
      if (!targetIds.has(blockId)) {
        delete nextBlocksById[blockId];
        this.hiddenChildBlockIds.delete(blockId);
      }
    }
    for (const block of target.blocks) {
      if (!affectedIds.has(block.id)) continue;
      const current = this.blocksById[block.id];
      const restored = { ...(payloadIds.has(block.id) || !current ? block : current),
        parent: placementIds.has(block.id) || !current ? block.parent : current.parent,
        edit_revision: current?.edit_revision ?? block.edit_revision,
      } as NotesBlock;
      nextBlocksById[block.id] = restored;
      this.rememberChildVisibility(restored);
    }
    for (const blockId of affectedIds) this.markLocallyChanged(blockId);
    this.blocksById = nextBlocksById;
    const children = Object.fromEntries(Object.entries(this.childIdsByParentId)
      .map(([parentId, ids]) => [parentId, ids.filter((id) => !placementIds.has(id))]));
    for (const [parentId, ids] of Object.entries(target.childIdsByParentId)) {
      const siblings = children[parentId] ?? [];
      for (let index = ids.length - 1; index >= 0; index -= 1) {
        const id = ids[index];
        if (!targetIds.has(id) || !placementIds.has(id)) continue;
        const nextIndex = siblings.indexOf(ids[index + 1]);
        siblings.splice(nextIndex < 0 ? siblings.length : nextIndex, 0, id);
      }
      children[parentId] = siblings;
    }
    this.childIdsByParentId = children;
    this.blockOutlines = this.blockOutlines.filter((outline) => !sourceIds.has(outline.id) || targetIds.has(outline.id));
    this.syncLocalOutlines();
  }

  insertBlockAfter(block: NotesBlock, afterBlockId: string | null): void {
    const parentId = parentIdForBlock(block);
    const current = (this.childIdsByParentId[parentId] ?? []).filter((id) => id !== block.id);
    const afterIndex = afterBlockId ? current.indexOf(afterBlockId) : -1;
    const insertIndex = afterIndex >= 0 ? afterIndex + 1 : current.length;
    this.insertBlockAt(block, parentId, current, insertIndex);
  }

  insertBlockBefore(block: NotesBlock, beforeBlockId: string): void {
    const parentId = parentIdForBlock(block);
    const current = (this.childIdsByParentId[parentId] ?? []).filter((id) => id !== block.id);
    const beforeIndex = current.indexOf(beforeBlockId);
    if (beforeIndex < 0) throw new Error("Notes insertion target not found");
    const pageId = this.options.readSelectedPageId();
    if (pageId) {
      const siblings = this.blockOutlines
        .filter((outline) => (outline.parent.type === "page_id"
          ? outline.parent.page_id : outline.parent.block_id) === parentId)
        .sort((left, right) => left.sort_order - right.sort_order || left.id.localeCompare(right.id));
      const outlineIndex = siblings.findIndex((outline) => outline.id === beforeBlockId);
      if (outlineIndex >= 0) {
        siblings.splice(outlineIndex, 0, notesBlockOutlineFromBlock(block, pageId, outlineIndex));
        const siblingIds = new Set(siblings.map((outline) => outline.id));
        this.blockOutlines = [
          ...this.blockOutlines.filter((outline) => !siblingIds.has(outline.id)),
          ...siblings.map((outline, index) => ({ ...outline, sort_order: index })),
        ];
      }
    }
    this.insertBlockAt(block, parentId, current, beforeIndex);
  }

  private insertBlockAt(
    block: NotesBlock,
    parentId: string,
    current: string[],
    insertIndex: number,
  ): void {
    this.childIdsByParentId = {
      ...this.childIdsByParentId,
      [parentId]: [...current.slice(0, insertIndex), block.id, ...current.slice(insertIndex)],
    };
    this.replaceBlock(block);
    this.markLocallyChanged(block.id);
    this.syncLocalOutlines();
  }

  removeLeafBlock(blockId: string): boolean {
    const next = notesTreeStateWithoutLeafBlock(this.treeState(), blockId);
    if (!next) return false;
    this.markLocallyChanged(blockId);
    this.hiddenChildBlockIds.delete(blockId);
    this.blocksById = next.blocksById;
    this.childIdsByParentId = next.childIdsByParentId;
    this.blockOutlines = this.blockOutlines.filter((outline) => outline.id !== blockId);
    this.syncLocalOutlines();
    return true;
  }

  private syncLocalOutlines(): void {
    const pageId = this.options.readSelectedPageId();
    if (pageId) this.syncHydratedOutlines(pageId);
  }

  setLoadedPage(loaded: NotesLoadedPage): void {
    this.loadedPage = loaded.page;
    this.hiddenChildBlockIds.clear();
    const blocks = loaded.blocks.results;
    this.blocksById = Object.fromEntries(blocks.map((block) => [block.id, block]));
    this.childIdsByParentId = buildNotesChildIdsByParent(blocks);
    this.primaryContentReady = true;
  }

  replaceHydratedBlocks(
    blocksById: Record<string, NotesBlock>,
    childIdsByParentId: Record<string, string[]>,
  ): void {
    for (const block of Object.values(this.blocksById)) {
      if (!blocksById[block.id] && !blockChildrenAreVisible(block)) {
        this.hiddenChildBlockIds.add(block.id);
      }
    }
    for (const block of Object.values(blocksById)) this.rememberChildVisibility(block);
    this.blocksById = blocksById;
    this.childIdsByParentId = childIdsByParentId;
    const pageId = this.options.readSelectedPageId();
    if (pageId) this.flatBlockOutlines = flattenNotesBlockOutlines(this.blockOutlines, pageId, blocksById, this.hiddenChildBlockIds);
  }

  private rememberChildVisibility(block: NotesBlock): void {
    if (blockChildrenAreVisible(block)) this.hiddenChildBlockIds.delete(block.id);
    else this.hiddenChildBlockIds.add(block.id);
  }

  replaceOutlines(outlines: readonly NotesBlockOutline[], pageId: string): void {
    this.blockOutlines = [...outlines];
    this.flatBlockOutlines = flattenNotesBlockOutlines(this.blockOutlines, pageId, this.blocksById, this.hiddenChildBlockIds);
  }

  mergeOutlines(outlines: readonly NotesBlockOutline[], pageId: string): void {
    const next = new Map(this.blockOutlines.map((outline) => [outline.id, outline]));
    for (const outline of outlines) next.set(outline.id, outline);
    this.replaceOutlines([...next.values()], pageId);
  }

  syncHydratedOutlines(pageId: string): void {
    const next = new Map(this.blockOutlines.map((outline) => [outline.id, outline]));
    for (const [parentId, childIds] of Object.entries(this.childIdsByParentId)) {
      const loadedIds = new Set(childIds);
      const previous = [...next.values()].filter((outline) => (
        (outline.parent.type === "page_id" ? outline.parent.page_id : outline.parent.block_id) === parentId
      )).sort((left, right) => left.sort_order - right.sort_order);
      const ordered = [...childIds];
      // Retained, unloaded siblings stay ahead of their next known sibling.
      for (let index = previous.length - 1; index >= 0; index -= 1) {
        const outline = previous[index];
        if (loadedIds.has(outline.id)) continue;
        const following = ordered.indexOf(previous[index + 1]?.id);
        ordered.splice(following < 0 ? ordered.length : following, 0, outline.id);
      }
      ordered.forEach((blockId, index) => {
        const block = this.blocksById[blockId];
        const existing = next.get(blockId);
        const outline = block ? notesBlockOutlineFromBlock(block, pageId, index) : existing;
        if (!outline) return;
        next.set(blockId, {
          ...outline,
          sort_order: index,
          parent: parentId === pageId
            ? { type: "page_id", page_id: pageId }
            : { type: "block_id", block_id: parentId },
        });
      });
    }
    this.replaceOutlines([...next.values()], pageId);
  }

  applyPostMutation(result: NotesPostMutationResult): void {
    if (result.loadedPage !== undefined) {
      if (result.loadedPage) {
        this.setLoadedPage(result.loadedPage);
        this.blockOutlines = [];
        this.syncHydratedOutlines(result.loadedPage.page.id);
      } else {
        this.clearLoadedTree();
      }
    }
    if (result.blocks || result.placements || result.removedBlockIds) {
      const next = applyNotesPostMutationToTree(this.treeState(), result);
      this.blocksById = { ...next.blocksById };
      for (const block of result.blocks ?? []) this.rememberChildVisibility(block);
      for (const blockId of result.removedBlockIds ?? []) this.hiddenChildBlockIds.delete(blockId);
      this.childIdsByParentId = Object.fromEntries(
        Object.entries(next.childIdsByParentId).map(([parentId, childIds]) => [parentId, [...childIds]]),
      );
      if (!result.canonical) {
        for (const block of result.blocks ?? []) this.markLocallyChanged(block.id);
        for (const blockId of result.removedBlockIds ?? []) this.markLocallyChanged(blockId);
      }
    }
    for (const page of result.pages ?? []) {
      const childPageBlock = this.blocksById[page.id];
      if (childPageBlock?.type !== "child_page") continue;
      const title = notesPageTitle(page, "");
      if (childPageBlock.child_page.title === title) continue;
      this.replaceBlock({
        ...childPageBlock,
        last_edited_time: page.last_edited_time,
        child_page: { ...childPageBlock.child_page, title },
      });
    }
    if (this.loadedPage) {
      const returned = result.pages?.find((page) => page.id === this.loadedPage?.id);
      if (returned) this.loadedPage = returned;
      if (result.blocks || result.placements || result.removedBlockIds) {
        const removed = new Set(result.removedBlockIds ?? []);
        this.blockOutlines = this.blockOutlines.filter((outline) => !removed.has(outline.id));
        this.syncHydratedOutlines(this.loadedPage.id);
      }
    }
  }

  resetOutlines(): void {
    this.blockOutlines = [];
    this.flatBlockOutlines = [];
  }

  clearLoadedTree(): void {
    this.loadedPage = null;
    this.hiddenChildBlockIds.clear();
    this.blocksById = {};
    this.childIdsByParentId = {};
  }

  clearSelection(): void {
    this.clearLoadedTree();
    this.resetOutlines();
    this.primaryContentReady = false;
  }
}
