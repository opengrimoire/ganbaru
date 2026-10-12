import type { NotesTextSelection } from "./selection";

export interface NotesFocusRequest {
  blockId: string | null;
  requestId: number;
  selection: NotesTextSelection | null;
  preventScroll?: boolean;
}

export interface NotesDeleteFocusInput {
  visibleBlockIds: readonly string[];
  removedBlockIds: readonly string[];
  firstRemovedBlockId: string;
}

export interface NotesDocumentEndPointerInput {
  pointerY: number;
  lastRowBottom: number;
  targetInsideRow: boolean;
}

export interface NotesDocumentEndFocusInput {
  activeBlockId: string | null;
  lastBlockId: string;
  selection: NotesTextSelection | null;
  textLength: number;
}

/**
 * Decide whether a background pointer represents the document position after its final row.
 */
export function notesBackgroundPointerTargetsDocumentEnd(
  input: NotesDocumentEndPointerInput,
): boolean {
  return !input.targetInsideRow && input.pointerY >= input.lastRowBottom;
}

/**
 * Detect a document-end action that would only refocus the current caret position.
 */
export function notesDocumentEndFocusIsCurrent(
  input: NotesDocumentEndFocusInput,
): boolean {
  return input.activeBlockId === input.lastBlockId
    && input.selection?.start === input.textLength
    && input.selection.end === input.textLength;
}

/**
 * Return the next focus request token for a Notes block.
 */
export function nextNotesFocusRequest(
  current: NotesFocusRequest,
  blockId: string | null,
  selection: NotesTextSelection | null = null,
  preventScroll = false,
): NotesFocusRequest {
  return {
    blockId,
    requestId: current.requestId + 1,
    selection,
    ...(preventScroll ? { preventScroll: true } : {}),
  };
}

/**
 * Pick the block to focus after a page load or link navigation.
 */
export function planNotesPageLoadFocus(
  visibleBlockIds: readonly string[],
  requestedBlockId: string | null = null,
): string | null {
  if (requestedBlockId && visibleBlockIds.includes(requestedBlockId)) {
    return requestedBlockId;
  }
  return visibleBlockIds[0] ?? null;
}

/**
 * Pick the first inserted block when available, otherwise keep the trigger focused.
 */
export function planNotesInsertedBlockFocus(
  insertedBlockIds: readonly (string | null | undefined)[],
  fallbackBlockId: string | null = null,
): string | null {
  return (
    insertedBlockIds.find(
      (blockId): blockId is string => typeof blockId === "string" && blockId.length > 0,
    ) ?? fallbackBlockId
  );
}

/**
 * Pick the nearest surviving block after blocks leave the current rendered page.
 */
export function planNotesDeletedBlockFocus(input: NotesDeleteFocusInput): string | null {
  const removedIds = new Set(input.removedBlockIds);
  const firstRemovedIndex = input.visibleBlockIds.findIndex(
    (blockId) => blockId === input.firstRemovedBlockId || removedIds.has(blockId),
  );
  if (firstRemovedIndex < 0) {
    return planNotesPageLoadFocus(input.visibleBlockIds);
  }

  let lastRemovedIndex = firstRemovedIndex;
  for (let index = firstRemovedIndex; index < input.visibleBlockIds.length; index += 1) {
    const blockId = input.visibleBlockIds[index];
    if (blockId && removedIds.has(blockId)) {
      lastRemovedIndex = index;
      continue;
    }
    break;
  }

  for (let index = firstRemovedIndex - 1; index >= 0; index -= 1) {
    const blockId = input.visibleBlockIds[index];
    if (blockId && !removedIds.has(blockId)) return blockId;
  }
  for (let index = lastRemovedIndex + 1; index < input.visibleBlockIds.length; index += 1) {
    const blockId = input.visibleBlockIds[index];
    if (blockId && !removedIds.has(blockId)) return blockId;
  }
  return null;
}
