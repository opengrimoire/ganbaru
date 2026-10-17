import type { NotesTextSelection } from "$lib/notes/editor/selection";

export type NotesBlockNavigationDirection = "previous" | "next";
export type NotesBlockNavigationBoundary = "first" | "last";

/**
 * Return the rendered block adjacent to the current rendered block.
 */
export function notesAdjacentRenderedBlockId(
  renderedBlockIds: readonly string[],
  currentBlockId: string,
  direction: NotesBlockNavigationDirection,
): string | null {
  const index = renderedBlockIds.indexOf(currentBlockId);
  if (index < 0) return null;
  const targetIndex = direction === "previous" ? index - 1 : index + 1;
  return renderedBlockIds[targetIndex] ?? null;
}

/**
 * Return the first or last rendered block.
 */
export function notesBoundaryRenderedBlockId(
  renderedBlockIds: readonly string[],
  boundary: NotesBlockNavigationBoundary,
): string | null {
  return boundary === "first"
    ? renderedBlockIds[0] ?? null
    : renderedBlockIds.at(-1) ?? null;
}

/**
 * Return a collapsed selection at a text offset.
 */
export function notesCollapsedNavigationSelection(offset: number): NotesTextSelection {
  return { start: offset, end: offset };
}
