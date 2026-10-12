import { blockPlainText, isTextEditableBlock } from "$lib/notes/blocks/factory";
import {
  notesAdjacentRenderedBlockId,
  notesBoundaryRenderedBlockId,
  notesCollapsedNavigationSelection,
  type NotesBlockNavigationBoundary,
  type NotesBlockNavigationDirection,
} from "$lib/notes/blocks/navigation";
import {
  notesPlainTextFromEditableRoot,
  notesTextSelectionFromEditableRoot,
  restoreNotesEditableSelection,
  type NotesTextSelection,
} from "$lib/notes/editor/selection";
import type { NotesBlock } from "$lib/notes/types";
import { notesCaretOffsetOnVisualLine, notesEditableVisualLines } from "./visual-line-navigation";

export interface NotesBlockNavigationOptions {
  readListElement: () => HTMLDivElement | null;
  readRenderedBlockIds: () => readonly string[];
  readBlock: (blockId: string) => NotesBlock | undefined;
  isHiddenCalloutLabel: (blockId: string) => boolean;
  requestFocus: (blockId: string, selection: NotesTextSelection | null) => void;
  insertParagraphAdjacent?: (blockId: string, direction: NotesBlockNavigationDirection) => void;
}

/** Coordinate DOM-aware keyboard navigation and focus across rendered Notes blocks. */
export function createNotesBlockNavigationController(options: NotesBlockNavigationOptions) {
  let verticalGoalX: number | null = null;

  function resetVerticalGoal(): void {
    verticalGoalX = null;
  }

  function rowFromEvent(event: Event): HTMLElement | null {
    const target = event.target;
    const list = options.readListElement();
    if (!(target instanceof Element) || !list) return null;
    const row = target.closest<HTMLElement>("[data-notes-selectable-block-id]");
    return row && list.contains(row) ? row : null;
  }

  function blockIdFromEvent(event: Event): string | null {
    return rowFromEvent(event)?.dataset.notesSelectableBlockId ?? null;
  }

  function rowForBlock(blockId: string): HTMLElement | null {
    const list = options.readListElement();
    if (!list) return null;
    return Array.from(list.querySelectorAll<HTMLElement>("[data-notes-selectable-block-id]"))
      .find((element) => element.dataset.notesSelectableBlockId === blockId) ?? null;
  }

  function textEditorForBlock(blockId: string): HTMLElement | null {
    return Array.from(
      rowForBlock(blockId)?.querySelectorAll<HTMLElement>(
        "[contenteditable='true'][role='textbox'][data-notes-block-id]",
      ) ?? [],
    ).find((editor) => editor.dataset.notesBlockId === blockId) ?? null;
  }

  function focusTextEditorAtEnd(blockId: string): boolean {
    const block = options.readBlock(blockId);
    if (!block || !isTextEditableBlock(block.type)) return false;
    const editor = textEditorForBlock(blockId);
    if (!editor) return false;
    editor.focus({ preventScroll: true });
    const textLength = notesPlainTextFromEditableRoot(editor).length;
    restoreNotesEditableSelection(editor, { start: textLength, end: textLength });
    return true;
  }

  function focusRow(blockId: string, preventScroll = true): void {
    queueMicrotask(() => rowForBlock(blockId)?.focus({ preventScroll }));
  }

  function editorFromEvent(event: Event, blockId: string): HTMLElement | null {
    const target = event.target;
    if (!(target instanceof Element)) return null;
    const editor = target.closest<HTMLElement>(
      "[contenteditable='true'][role='textbox'][data-notes-block-id]",
    );
    return editor?.dataset.notesBlockId === blockId ? editor : null;
  }

  function collapsedSelectionRect(editor: HTMLElement): DOMRect | null {
    const selection = editor.ownerDocument.getSelection();
    if (!selection || selection.rangeCount === 0 || !selection.isCollapsed) return null;
    if (!selection.focusNode || !editor.contains(selection.focusNode)) return null;
    const range = selection.getRangeAt(0).cloneRange();
    const rect = range.getBoundingClientRect?.();
    if (rect && (rect.width > 0 || rect.height > 0)) return rect;
    return Array.from(range.getClientRects?.() ?? [])
      .find((candidate) => candidate.width > 0 || candidate.height > 0) ?? null;
  }

  function caretIsOnBoundaryLine(editor: HTMLElement, direction: NotesBlockNavigationDirection): boolean {
    const selection = notesTextSelectionFromEditableRoot(editor);
    const textLength = notesPlainTextFromEditableRoot(editor).length;
    if (selection && selection.start === selection.end) {
      if (direction === "previous" && selection.start === 0) return true;
      if (direction === "next" && selection.start === textLength) return true;
    }
    const lineTops = notesEditableVisualLines(editor).map((line) => line.top);
    if (lineTops.length <= 1) return true;
    const caretRect = collapsedSelectionRect(editor);
    if (!caretRect) return false;
    const boundaryTop = direction === "previous" ? lineTops[0] : lineTops.at(-1);
    return boundaryTop !== undefined && Math.abs(caretRect.top - boundaryTop) < 2;
  }

  function targetLineOffset(editor: HTMLElement, direction: NotesBlockNavigationDirection, x: number): number | null {
    const lines = notesEditableVisualLines(editor);
    const line = direction === "previous" ? lines.at(-1) : lines[0];
    if (!line) return null;
    return notesCaretOffsetOnVisualLine(editor, line, x);
  }

  function clampedSelection(blockId: string, selection: NotesTextSelection | null): NotesTextSelection | null {
    const block = options.readBlock(blockId);
    if (!block || !isTextEditableBlock(block.type) || !selection) return null;
    const length = blockPlainText(block).length;
    const start = Math.min(Math.max(0, selection.start), length);
    const end = Math.min(Math.max(0, selection.end), length);
    return { start: Math.min(start, end), end: Math.max(start, end) };
  }

  function selectionAt(blockId: string, offset: number): NotesTextSelection | null {
    return clampedSelection(blockId, notesCollapsedNavigationSelection(offset));
  }

  function focusAdjacent(currentId: string, direction: NotesBlockNavigationDirection, x: number | null): boolean {
    const ids = options.readRenderedBlockIds();
    let targetId = notesAdjacentRenderedBlockId(ids, currentId, direction);
    while (targetId && options.isHiddenCalloutLabel(targetId)) {
      targetId = notesAdjacentRenderedBlockId(ids, targetId, direction);
    }
    if (!targetId) return false;
    const block = options.readBlock(targetId);
    const fallback = direction === "next" ? 0 : block && isTextEditableBlock(block.type) ? blockPlainText(block).length : 0;
    const editor = x === null ? null : textEditorForBlock(targetId);
    const offset = editor && x !== null ? targetLineOffset(editor, direction, x) : null;
    options.requestFocus(targetId, selectionAt(targetId, offset ?? fallback));
    return true;
  }

  function focusBoundary(boundary: NotesBlockNavigationBoundary): boolean {
    const ids = options.readRenderedBlockIds().filter((id) => !options.isHiddenCalloutLabel(id));
    const targetId = notesBoundaryRenderedBlockId(ids, boundary);
    if (!targetId) return false;
    const block = options.readBlock(targetId);
    const offset = boundary === "first" ? 0 : block && isTextEditableBlock(block.type) ? blockPlainText(block).length : 0;
    options.requestFocus(targetId, selectionAt(targetId, offset));
    return true;
  }

  function handleKeydown(event: KeyboardEvent, blockId: string): boolean {
    if (event.isComposing) return false;
    const isAtomicTarget = options.readBlock(blockId)?.type === "child_page"
      && event.target instanceof Element
      && (event.target.matches("[data-notes-selectable-block-id]") || !!event.target.closest("[data-notes-atomic-block]"));
    if (isAtomicTarget && !event.ctrlKey && !event.metaKey && !event.altKey && event.key === "Enter") {
      event.preventDefault();
      options.insertParagraphAdjacent?.(blockId, event.shiftKey ? "previous" : "next");
      return true;
    }
    if (event.altKey || event.shiftKey) { resetVerticalGoal(); return false; }
    if (event.ctrlKey || event.metaKey) {
      resetVerticalGoal();
      if (event.key === "Home" || (event.metaKey && event.key === "ArrowUp")) {
        event.preventDefault();
        return focusBoundary("first");
      }
      if (event.key === "End" || (event.metaKey && event.key === "ArrowDown")) {
        event.preventDefault();
        return focusBoundary("last");
      }
      return false;
    }
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") { resetVerticalGoal(); return false; }
    const direction: NotesBlockNavigationDirection = event.key === "ArrowUp" ? "previous" : "next";
    const editor = editorFromEvent(event, blockId);
    if (editor) {
      const selection = notesTextSelectionFromEditableRoot(editor);
      if (selection && selection.start === selection.end) {
        verticalGoalX ??= collapsedSelectionRect(editor)?.left ?? null;
      } else resetVerticalGoal();
      if (!selection || selection.start !== selection.end || !caretIsOnBoundaryLine(editor, direction)) return false;
      if (!focusAdjacent(blockId, direction, verticalGoalX)) return false;
      event.preventDefault();
      return true;
    }
    const isDatabaseTitleTarget = options.readBlock(blockId)?.type === "child_database"
      && event.target instanceof HTMLInputElement && event.target.hasAttribute("data-notes-database-title");
    if ((!isAtomicTarget && !isDatabaseTitleTarget && targetIsEditable(event.target))
      || !focusAdjacent(blockId, direction, verticalGoalX)) return false;
    event.preventDefault();
    return true;
  }

  function targetIsEditable(target: EventTarget | null): boolean {
    return target instanceof Element
      && target.closest("input, textarea, select, button, a, [contenteditable='true'], [role='textbox']") !== null;
  }

  /** Only an explicit zone in the target row can begin whole-block selection. */
  function targetIsSelectionZone(target: EventTarget | null): boolean {
    if (!(target instanceof Element)) return false;
    const zone = target.closest("[data-notes-block-selection-zone]");
    const row = target.closest("[data-notes-selectable-block-id]");
    return !!zone && !!row && zone.closest("[data-notes-selectable-block-id]") === row;
  }

  return {
    rowFromEvent,
    blockIdFromEvent,
    rowForBlock,
    textEditorForBlock,
    focusTextEditorAtEnd,
    focusRow,
    resetVerticalGoal,
    handleKeydown,
    targetIsEditable,
    targetIsSelectionZone,
  };
}
