import {
  NOTES_BLOCK_DRAG_MIME,
  planNotesBlockDrop,
  setActiveNotesBlockDragId,
  type NotesBlockDropIndicator,
  type NotesBlockDropIntent,
} from "$lib/notes/blocks/drag";
import type { NotesTreeState } from "$lib/notes/blocks/tree";

export interface NotesBlockDragControllerOptions {
  readTreeState: () => NotesTreeState;
  dropBlock: (
    sourceBlockId: string,
    targetBlockId: string,
    intent: NotesBlockDropIndicator,
  ) => Promise<void> | void;
}

/** Coordinate native Notes block drag state and validated drop intents. */
export function createNotesBlockDragController(options: NotesBlockDragControllerOptions) {
  let draggingBlockId = $state<string | null>(null);
  let dropTarget = $state<{ blockId: string; intent: NotesBlockDropIndicator } | null>(null);

  function draggedBlockIdFromEvent(event: DragEvent): string | null {
    const transferred = event.dataTransfer?.getData(NOTES_BLOCK_DRAG_MIME) ?? "";
    return transferred || draggingBlockId;
  }

  function dropIntentFromEvent(event: DragEvent): NotesBlockDropIntent {
    const target = event.currentTarget;
    if (!(target instanceof HTMLElement)) return "after";
    const rect = target.getBoundingClientRect();
    const depth = Number.parseInt(
      getComputedStyle(target).getPropertyValue("--notes-depth").trim(),
      10,
    );
    const safeDepth = Number.isFinite(depth) ? Math.max(depth, 0) : 0;
    const yRatio = rect.height > 0 ? (event.clientY - rect.top) / rect.height : 0.5;
    if (yRatio < 0.25) return "before";
    const outdentBoundary = rect.left + safeDepth * 20 + 28;
    if (safeDepth > 0 && event.clientX < outdentBoundary) return "outdent";
    if (yRatio > 0.75) return "after";
    return "inside";
  }

  function start(blockId: string, event: DragEvent): void {
    draggingBlockId = blockId;
    setActiveNotesBlockDragId(blockId);
    dropTarget = null;
    if (!event.dataTransfer) return;
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData(NOTES_BLOCK_DRAG_MIME, blockId);
    event.dataTransfer.setData("text/plain", blockId);
  }

  function end(): void {
    draggingBlockId = null;
    setActiveNotesBlockDragId(null);
    dropTarget = null;
  }

  function over(targetBlockId: string, event: DragEvent): void {
    const sourceBlockId = draggedBlockIdFromEvent(event);
    if (!sourceBlockId) {
      if (dropTarget?.blockId === targetBlockId) dropTarget = null;
      return;
    }
    const plan = planNotesBlockDrop(
      options.readTreeState(),
      sourceBlockId,
      targetBlockId,
      dropIntentFromEvent(event),
    );
    if (!plan) {
      if (dropTarget?.blockId === targetBlockId) dropTarget = null;
      return;
    }
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    dropTarget = { blockId: targetBlockId, intent: plan.indicator };
  }

  function leave(targetBlockId: string, event: DragEvent): void {
    const target = event.currentTarget;
    const related = event.relatedTarget;
    if (target instanceof HTMLElement && related instanceof Node && target.contains(related)) return;
    if (dropTarget?.blockId === targetBlockId) dropTarget = null;
  }

  function drop(targetBlockId: string, event: DragEvent): void {
    const sourceBlockId = draggedBlockIdFromEvent(event);
    if (!sourceBlockId) {
      end();
      return;
    }
    const intent = dropTarget?.blockId === targetBlockId
      ? dropTarget.intent
      : dropIntentFromEvent(event);
    const plan = planNotesBlockDrop(
      options.readTreeState(),
      sourceBlockId,
      targetBlockId,
      intent,
    );
    if (!plan) {
      end();
      return;
    }
    event.preventDefault();
    end();
    void options.dropBlock(sourceBlockId, targetBlockId, plan.indicator);
  }

  return {
    get draggingBlockId() {
      return draggingBlockId;
    },
    dropPositionForBlock(blockId: string): NotesBlockDropIndicator | null {
      return dropTarget?.blockId === blockId ? dropTarget.intent : null;
    },
    start,
    end,
    over,
    leave,
    drop,
  };
}
