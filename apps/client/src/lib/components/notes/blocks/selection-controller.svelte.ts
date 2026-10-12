import { readNotesClipboard } from "$lib/notes/clipboard/paste";
import { getLocalization } from "$lib/i18n/translator.svelte";
import { notesClipboardContent, writeNotesClipboard } from "$lib/notes/clipboard/export";
import { notesUndoShortcutAction } from "$lib/notes/history/undo-history";
import type { NotesBlockSelectionState } from "$lib/notes/blocks/selection";
import {
  notesBlockSelectionAfterClick,
  notesBlockSelectionAfterKeyboard,
  notesBlockSelectionContains,
  notesBlockSelectionForBlock,
  notesBlockSelectionPrunedToVisible,
  notesBlockSelectionRange,
} from "$lib/notes/blocks/selection";
import {
  notesSelectionRootBlockIds,
  planNotesSelectionMoveWithinSiblings,
} from "$lib/notes/blocks/selection-operations";
import type { NotesTreeState } from "$lib/notes/blocks/tree";

export interface NotesBlockSelectionClipboard {
  mode: "copy" | "cut";
  pageId: string;
  rootBlockIds: string[];
  subtreeBlockIds: string[];
  plainText: string;
}

export interface NotesBlockSelectionDelegates {
  pointerDown: (event: PointerEvent) => void;
  pointerOver: (event: PointerEvent) => void;
  keydown: (event: KeyboardEvent) => void;
}

export interface NotesBlockSelectionControllerOptions {
  undo: () => Promise<boolean>;
  redo: () => Promise<boolean>;
  readPageId: () => string;
  readListElement: () => HTMLDivElement | null;
  readRenderedBlockIds: () => readonly string[];
  readTreeState: () => NotesTreeState;
  hydrateSubtrees: (rootIds: readonly string[]) => Promise<readonly string[]>;
  blockIdFromEvent: (event: Event) => string | null;
  targetIsEditable: (target: EventTarget | null) => boolean;
  targetIsSelectionZone: (target: EventTarget | null) => boolean;
  focusTextEditorAtEnd: (blockId: string) => boolean;
  focusRow: (blockId: string, preventScroll?: boolean) => void;
  handleNavigationKeydown: (event: KeyboardEvent, blockId: string) => boolean;
  pasteBlocks: (rootIds: readonly string[], subtreeIds: readonly string[], targetId: string, includeTrashed: boolean) => Promise<string | null>;
  duplicateBlocks: (ids: readonly string[]) => Promise<string | null>;
  moveBlocks: (ids: readonly string[], direction: "up" | "down") => Promise<void>;
  deleteBlocks: (ids: readonly string[]) => Promise<void | boolean>;
}

/** Attach and symmetrically remove delegated Notes block-list listeners. */
export function attachNotesBlockSelectionDelegates(
  node: HTMLDivElement,
  delegates: NotesBlockSelectionDelegates,
) {
  node.addEventListener("pointerdown", delegates.pointerDown);
  node.addEventListener("pointerover", delegates.pointerOver);
  node.addEventListener("keydown", delegates.keydown);
  return {
    destroy() {
      node.removeEventListener("pointerdown", delegates.pointerDown);
      node.removeEventListener("pointerover", delegates.pointerOver);
      node.removeEventListener("keydown", delegates.keydown);
    },
  };
}

/** Own reactive multi-block selection state and delegated list listeners. */
export function createNotesBlockSelectionController(options: NotesBlockSelectionControllerOptions) {
  let selection = $state<NotesBlockSelectionState | null>(null);
  let dragAnchorBlockId = $state<string | null>(null);
  let dragPointerId = $state<number | null>(null);
  let clipboard = $state<NotesBlockSelectionClipboard | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function run(action: () => Promise<void> | void): Promise<void> {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await action();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      busy = false;
    }
  }

  function clearNativeSelection(): void {
    if (typeof window !== "undefined") window.getSelection()?.removeAllRanges();
  }

  function sameSelection(left: NotesBlockSelectionState | null, right: NotesBlockSelectionState | null): boolean {
    if (left === right) return true;
    if (!left || !right) return false;
    return left.anchorBlockId === right.anchorBlockId
      && left.focusBlockId === right.focusBlockId
      && left.selectedBlockIds.length === right.selectedBlockIds.length
      && left.selectedBlockIds.every((id, index) => right.selectedBlockIds[index] === id);
  }

  function syncAttributes(): void {
    const list = options.readListElement();
    if (!list) return;
    for (const row of list.querySelectorAll<HTMLElement>("[data-notes-selectable-block-id]")) {
      const id = row.dataset.notesSelectableBlockId ?? "";
      row.toggleAttribute("data-notes-block-selected", notesBlockSelectionContains(selection, id));
    }
  }

  function setSelection(value: NotesBlockSelectionState | null): void {
    if (sameSelection(selection, value)) return;
    selection = value;
    if (!value) { dragAnchorBlockId = null; dragPointerId = null; }
    queueMicrotask(syncAttributes);
  }

  function pruneToRendered(): void {
    const pruned = notesBlockSelectionPrunedToVisible(options.readRenderedBlockIds(), selection);
    if (!sameSelection(selection, pruned)) selection = pruned;
    else syncAttributes();
  }

  function pointerDown(event: PointerEvent): void {
    if (event.defaultPrevented || event.button !== 0) return;
    if (event.target instanceof Element && event.target.closest("[data-notes-selection-menu]")) return;
    const id = options.blockIdFromEvent(event);
    if (!id) { if (selection) setSelection(null); return; }
    if (options.targetIsEditable(event.target)) {
      if (selection) setSelection(null);
      return;
    }
    if (!options.targetIsSelectionZone(event.target)) { if (selection) setSelection(null); return; }
    if (event.shiftKey && selection) {
      event.preventDefault(); clearNativeSelection();
      const next = notesBlockSelectionAfterClick({ blockIds: options.readRenderedBlockIds(), current: selection, blockId: id, extend: true });
      setSelection(next); if (next) options.focusRow(next.focusBlockId); return;
    }
    event.preventDefault(); clearNativeSelection();
    dragAnchorBlockId = id; dragPointerId = event.pointerId;
    setSelection(notesBlockSelectionForBlock(options.readRenderedBlockIds(), id));
    options.focusRow(id);
  }

  function pointerOver(event: PointerEvent): void {
    if (dragPointerId === null || event.pointerId !== dragPointerId || !dragAnchorBlockId) return;
    const id = options.blockIdFromEvent(event); if (!id) return;
    if (id === dragAnchorBlockId && !selection) return;
    clearNativeSelection();
    setSelection(notesBlockSelectionRange(options.readRenderedBlockIds(), dragAnchorBlockId, id));
    options.focusRow(id);
  }

  async function copy(mode: "copy" | "cut"): Promise<void> {
    if (!selection) return;
    const selectedIds = [...selection.selectedBlockIds];
    const pageId = options.readPageId();
    const selected = selection;
    const roots = notesSelectionRootBlockIds(options.readTreeState(), selectedIds);
    if (!roots.length) return;
    const subtree = [...await options.hydrateSubtrees(roots)];
    if (!subtree.length || pageId !== options.readPageId() || !sameSelection(selected, selection)) return;
    const tree = options.readTreeState();
    const content = notesClipboardContent(subtree.map((id) => {
      const block = tree.blocksById[id];
      if (!block) throw new Error("Notes selection content is still loading");
      return { block };
    }), { pageId, unnamedDatabaseTitle: getLocalization().t("notes.databaseNewTitle") });
    await writeNotesClipboard(content);
    if (pageId !== options.readPageId() || !sameSelection(selected, selection)) return;
    const plainText = content.plainText;
    clipboard = { mode, pageId, rootBlockIds: roots, subtreeBlockIds: subtree, plainText };
    if (mode === "cut") {
      const written = clipboard;
      if (await options.deleteBlocks(selectedIds) === false) {
        if (clipboard === written) clipboard.mode = "copy";
      } else setSelection(null);
    }
  }

  async function paste(targetId: string | null): Promise<void> {
    if (!clipboard || !targetId) return;
    const copied = clipboard;
    const pageId = options.readPageId();
    const current = await readNotesClipboard();
    if (clipboard !== copied || pageId !== options.readPageId()) return;
    if (current.plainText !== copied.plainText) {
      clipboard = null;
      setSelection(null);
      options.focusTextEditorAtEnd(targetId);
      return;
    }
    const focusId = await options.pasteBlocks(copied.rootBlockIds, copied.subtreeBlockIds, targetId, copied.mode === "cut");
    if (copied.mode === "cut") clipboard = null;
    setSelection(null); if (focusId) options.focusRow(focusId, false);
  }

  async function duplicate(): Promise<void> { if (selection) { await options.duplicateBlocks(selection.selectedBlockIds); setSelection(null); } }
  async function move(direction: "up" | "down"): Promise<void> { if (selection) await options.moveBlocks(selection.selectedBlockIds, direction); }
  async function remove(): Promise<void> { if (selection && await options.deleteBlocks(selection.selectedBlockIds) !== false) setSelection(null); }

  function keydown(event: KeyboardEvent): void {
    if (event.defaultPrevented) return;
    const id = options.blockIdFromEvent(event); if (!id) return;
    const historyAction = notesUndoShortcutAction(event);
    if (historyAction && !options.targetIsEditable(event.target)) {
      event.preventDefault(); setSelection(null);
      void run(historyAction === "undo" ? async () => { await options.undo(); } : async () => { await options.redo(); });
      return;
    }
    if (!selection && options.handleNavigationKeydown(event, id)) return;
    const hasModifier = event.ctrlKey || event.metaKey; const key = event.key.toLowerCase();
    if (hasModifier && !event.shiftKey && !event.altKey && key === "a" && (!options.targetIsEditable(event.target) || selection)) {
      const ids = options.readRenderedBlockIds();
      if (ids.length) {
        event.preventDefault(); clearNativeSelection();
        setSelection(notesBlockSelectionRange(ids, ids[0], ids[ids.length - 1]));
      }
      return;
    }
    if (clipboard && hasModifier && !event.shiftKey && !event.altKey && key === "v" && !options.targetIsEditable(event.target)) { event.preventDefault(); void run(() => paste(selection?.focusBlockId ?? id)); return; }
    if (selection && !event.altKey && (event.key === "Backspace" || event.key === "Delete")) { event.preventDefault(); void run(remove); return; }
    if (selection && hasModifier && event.shiftKey && !event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) { event.preventDefault(); void run(() => move(event.key === "ArrowUp" ? "up" : "down")); return; }
    if (selection && hasModifier && !event.shiftKey && !event.altKey && ["c", "x", "d"].includes(key)) { event.preventDefault(); void run(key === "c" ? () => copy("copy") : key === "x" ? () => copy("cut") : duplicate); return; }
    if (event.key === "Escape" && !hasModifier && !event.altKey && !event.isComposing) {
      // Editable controls own dismissal before Svelte's delegated handlers run.
      // Escape never promotes a text caret or range into whole-block selection.
      if (!selection || options.targetIsEditable(event.target)) return;
      event.preventDefault();
      const focusId = selection.focusBlockId;
      setSelection(null);
      options.focusTextEditorAtEnd(focusId);
      return;
    }
    if (event.shiftKey && !hasModifier && !event.altKey && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
      if (!selection && options.targetIsEditable(event.target)) return;
      event.preventDefault(); clearNativeSelection();
      const next = notesBlockSelectionAfterKeyboard({ blockIds: options.readRenderedBlockIds(), current: selection, focusedBlockId: id, direction: event.key === "ArrowDown" ? "next" : "previous" });
      setSelection(next); if (next) options.focusRow(next.focusBlockId, false);
    }
  }

  function delegation(node: HTMLDivElement, delegates: NotesBlockSelectionDelegates) {
    const attached = attachNotesBlockSelectionDelegates(node, delegates);
    const endPointerGesture = () => { dragAnchorBlockId = null; dragPointerId = null; };
    const view = node.ownerDocument.defaultView;
    view?.addEventListener("pointerup", endPointerGesture);
    view?.addEventListener("pointercancel", endPointerGesture);
    return {
      destroy() {
        attached.destroy();
        view?.removeEventListener("pointerup", endPointerGesture);
        view?.removeEventListener("pointercancel", endPointerGesture);
        endPointerGesture();
      },
    };
  }

  return {
    get selection() { return selection; },
    get dragAnchorBlockId() { return dragAnchorBlockId; },
    get dragPointerId() { return dragPointerId; },
    get clipboard() { return clipboard; },
    get busy() { return busy; },
    get error() { return error; },
    get selectedRootBlockIds() { return selection ? notesSelectionRootBlockIds(options.readTreeState(), selection.selectedBlockIds) : []; },
    get canMoveUp() { return !!planNotesSelectionMoveWithinSiblings(options.readTreeState(), selection ? notesSelectionRootBlockIds(options.readTreeState(), selection.selectedBlockIds) : [], "up"); },
    get canMoveDown() { return !!planNotesSelectionMoveWithinSiblings(options.readTreeState(), selection ? notesSelectionRootBlockIds(options.readTreeState(), selection.selectedBlockIds) : [], "down"); },
    setSelection,
    startDrag(blockId: string, pointerId: number): void {
      dragAnchorBlockId = blockId;
      dragPointerId = pointerId;
    },
    setClipboard(value: NotesBlockSelectionClipboard | null): void { clipboard = value; },
    run,
    pruneToRendered,
    copy,
    paste,
    duplicate,
    move,
    remove,
    delegation(node: HTMLDivElement) { return delegation(node, { pointerDown, pointerOver, keydown }); },
  };
}
