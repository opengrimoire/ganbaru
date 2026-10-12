export type NotesBlockSelectionDirection = "previous" | "next";

export interface NotesBlockSelectionState {
  anchorBlockId: string;
  focusBlockId: string;
  selectedBlockIds: readonly string[];
}

export interface NotesBlockSelectionClickInput {
  blockIds: readonly string[];
  current: NotesBlockSelectionState | null;
  blockId: string;
  extend: boolean;
}

export interface NotesBlockSelectionKeyboardInput {
  blockIds: readonly string[];
  current: NotesBlockSelectionState | null;
  focusedBlockId: string;
  direction: NotesBlockSelectionDirection;
}

export function notesBlockSelectionForBlock(
  blockIds: readonly string[],
  blockId: string,
): NotesBlockSelectionState | null {
  return notesBlockSelectionRange(blockIds, blockId, blockId);
}

export function notesBlockSelectionAfterClick({
  blockIds,
  current,
  blockId,
  extend,
}: NotesBlockSelectionClickInput): NotesBlockSelectionState | null {
  if (!extend) return notesBlockSelectionForBlock(blockIds, blockId);
  const anchorBlockId = current?.anchorBlockId ?? current?.focusBlockId ?? blockId;
  return notesBlockSelectionRange(blockIds, anchorBlockId, blockId)
    ?? notesBlockSelectionForBlock(blockIds, blockId);
}

export function notesBlockSelectionAfterKeyboard({
  blockIds,
  current,
  focusedBlockId,
  direction,
}: NotesBlockSelectionKeyboardInput): NotesBlockSelectionState | null {
  const normalized = normalizeNotesSelectableBlockIds(blockIds);
  const focusBlockId = current?.focusBlockId ?? focusedBlockId;
  const focusIndex = normalized.indexOf(focusBlockId);
  if (focusIndex === -1) return notesBlockSelectionForBlock(normalized, focusedBlockId);
  const nextIndex = direction === "next"
    ? Math.min(normalized.length - 1, focusIndex + 1)
    : Math.max(0, focusIndex - 1);
  const nextFocusBlockId = normalized[nextIndex];
  if (!nextFocusBlockId) return null;
  const anchorBlockId = current?.anchorBlockId ?? focusedBlockId;
  return notesBlockSelectionRange(normalized, anchorBlockId, nextFocusBlockId);
}

export function notesBlockSelectionRange(
  blockIds: readonly string[],
  anchorBlockId: string,
  focusBlockId: string,
): NotesBlockSelectionState | null {
  const normalized = normalizeNotesSelectableBlockIds(blockIds);
  const anchorIndex = normalized.indexOf(anchorBlockId);
  const focusIndex = normalized.indexOf(focusBlockId);
  if (anchorIndex === -1 || focusIndex === -1) return null;
  const start = Math.min(anchorIndex, focusIndex);
  const end = Math.max(anchorIndex, focusIndex);
  return {
    anchorBlockId,
    focusBlockId,
    selectedBlockIds: normalized.slice(start, end + 1),
  };
}

export function notesBlockSelectionContains(
  selection: NotesBlockSelectionState | null,
  blockId: string,
): boolean {
  return selection?.selectedBlockIds.includes(blockId) ?? false;
}

export function notesBlockSelectionPrunedToVisible(
  blockIds: readonly string[],
  selection: NotesBlockSelectionState | null,
): NotesBlockSelectionState | null {
  if (!selection) return null;
  return notesBlockSelectionRange(blockIds, selection.anchorBlockId, selection.focusBlockId);
}

export function normalizeNotesSelectableBlockIds(blockIds: readonly string[]): string[] {
  const seen = new Set<string>();
  const normalized: string[] = [];
  for (const blockId of blockIds) {
    if (!blockId || seen.has(blockId)) continue;
    seen.add(blockId);
    normalized.push(blockId);
  }
  return normalized;
}
