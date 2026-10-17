import { notesClipboardPasteHtml, readNotesClipboard } from "$lib/notes/clipboard/paste";
import { notesClipboardContent, notesDocumentClipboardBlockIds, setNotesClipboardData, writeNotesClipboard } from "$lib/notes/clipboard/export";
import { notesRichTextFormattingShortcutAnnotationName } from "$lib/notes/rich-text/shortcuts";
import type { NotesRichTextAnnotationName } from "$lib/notes/rich-text/core";
import { tick } from "svelte";
import { getLocalization } from "$lib/i18n/translator.svelte";
import { notesPastedTextLinkUrl } from "$lib/notes/links/navigation";
import { blockPlainText, isTextEditableBlock } from "$lib/notes/blocks/factory";
import { isNotesTabKey } from "$lib/notes/blocks/keyboard";
import { createNotesDocumentSelectionPainter, findEditableDomPoint, notesEditableOffsetFromDomPoint } from "$lib/notes/editor/selection";
import { notesDocumentRange, type NotesDocumentPoint, type NotesDocumentSelection } from "$lib/notes/editor/selection";
import type { NotesBlock } from "$lib/notes/types";
import { notesCaretOffsetOnVisualLine, notesCaretRectAtOffset, notesCaretVisualLineIndex, notesEditableVisualLines } from "./visual-line-navigation";

const EDITABLE_BLOCK_SELECTOR = "[contenteditable='true'][data-notes-block-id][role='textbox']";
const ATOMIC_BLOCK_SELECTOR = "[data-notes-atomic-block]";
interface DocumentSelectionOptions {
  readIds: () => readonly string[];
  readPageId: () => string;
  readBlock: (id: string) => NotesBlock | undefined;
  isHiddenCalloutLabel: (id: string) => boolean;
  outlineSubtreeIds: (rootBlockIds: readonly string[]) => readonly string[];
  hydrate: (ids: readonly string[]) => Promise<void>;
  replace: (ids: readonly string[], start: number, end: number, text: string, html?: string, documentSelection?: NotesDocumentSelection) => Promise<void | boolean>;
  indent?: (ids: readonly string[], direction: "nest" | "outdent", selection?: NotesDocumentSelection) => Promise<void>;
  format: (ids: readonly string[], start: number, end: number, annotation: NotesRichTextAnnotationName, documentSelection?: NotesDocumentSelection) => Promise<void>;
  link?: (ids: readonly string[], start: number, end: number, url: string, documentSelection?: NotesDocumentSelection) => Promise<void>;
  focus: (point: NotesDocumentPoint, preventScroll?: boolean) => void;
  restoreFocusAfterEdit?: () => void;
  clearBlockSelection: () => void;
  undo: () => Promise<boolean>;
  redo: () => Promise<boolean>;
}

/** Own text ranges across block editing hosts before individual editors handle input. */
export function createNotesDocumentSelectionController(options: DocumentSelectionOptions) {
  let selection = $state<NotesDocumentSelection | null>(null);
  let pinnedIds = $state<string[]>([]);
  let error = $state<string | null>(null);
  let menu = $state<{ x: number; y: number } | null>(null);
  let list: HTMLDivElement | null = null;
  let pointerAnchor: NotesDocumentPoint | null = null;
  let pointerFromRow = false;
  let shouldSuppressAtomicClick = false;
  let selectionRequestId = 0;
  let replacementText: string | null = null;
  let alive = true;
  let composing = false;
  let shouldPasteAsPlainText = false;
  let verticalGoalX: number | null = null;
  let verticalFocusLine: { blockId: string; offset: number; index: number } | null = null;
  let extensionInProgress = false;
  let extensionEpoch = 0;
  let painter: ReturnType<typeof createNotesDocumentSelectionPainter> | null = null;
  let paintFrame: number | null = null;

  function resetVerticalNavigation(): void {
    verticalGoalX = null;
    verticalFocusLine = null;
    extensionEpoch += 1;
    extensionInProgress = false;
  }

  function editor(id: string): HTMLElement | undefined {
    return Array.from(list?.querySelectorAll<HTMLElement>(EDITABLE_BLOCK_SELECTOR) ?? []).find((node) => node.dataset.notesBlockId === id);
  }

  function point(node: Node | null, offset: number): NotesDocumentPoint | null {
    const element = node instanceof Element ? node : node?.parentElement;
    const atomic = element?.closest<HTMLElement>(ATOMIC_BLOCK_SELECTOR);
    if (atomic && list?.contains(atomic)) return { blockId: atomic.dataset.notesAtomicBlock!, offset: offset === 0 ? 0 : 1 };
    const root = element?.closest<HTMLElement>(EDITABLE_BLOCK_SELECTOR);
    if (!root || !node || !list?.contains(root)) return null;
    const textOffset = notesEditableOffsetFromDomPoint(root, node, offset);
    return textOffset === null ? null : { blockId: root.dataset.notesBlockId!, offset: textOffset };
  }

  function nativeSelection(): NotesDocumentSelection | null {
    const native = list?.ownerDocument.getSelection();
    const anchor = point(native?.anchorNode ?? null, native?.anchorOffset ?? 0);
    const focus = point(native?.focusNode ?? null, native?.focusOffset ?? 0);
    return anchor && focus ? { anchor, focus } : null;
  }

  function blockTextLength(id: string): number {
    const block = options.readBlock(id);
    if (block?.type === "child_page" || block?.type === "child_database") return 1;
    return block ? blockPlainText(block).length : 0;
  }

  function adjacentTextId(id: string, forward: boolean): string | undefined {
    const ids = options.readIds();
    const index = ids.indexOf(id);
    for (let next = index + (forward ? 1 : -1); next >= 0 && next < ids.length; next += forward ? 1 : -1) {
      const candidate = ids[next];
      if (options.isHiddenCalloutLabel(candidate)) continue;
      const block = options.readBlock(candidate);
      if (!block || isTextEditableBlock(block.type) || block.type === "child_page") return candidate;
    }
    return undefined;
  }

  function cancelScheduledPaint(): void {
    if (paintFrame === null) return;
    list?.ownerDocument.defaultView?.cancelAnimationFrame(paintFrame);
    paintFrame = null;
  }

  function schedulePaint(): void {
    if (paintFrame !== null) return;
    const view = list?.ownerDocument.defaultView;
    if (!view?.requestAnimationFrame) { paint(); return; }
    paintFrame = view.requestAnimationFrame(() => { paintFrame = null; paint(); });
  }

  function clear(preserveVerticalGoal = false): void {
    selectionRequestId += 1;
    cancelScheduledPaint();
    if (!preserveVerticalGoal) resetVerticalNavigation();
    selection = null;
    replacementText = null;
    pinnedIds = [];
    menu = null;
    list?.removeAttribute("data-notes-document-selection");
    list?.removeAttribute("data-notes-document-selection-composing");
    painter?.clear();
  }

  function paint(): void {
    cancelScheduledPaint();
    if (!selection || !list || composing) { painter?.clear(); return; }
    if (selection.anchor.blockId === selection.focus.blockId) {
      const root = editor(selection.anchor.blockId);
      if (root) {
        const anchor = findEditableDomPoint(root, selection.anchor.offset);
        const focus = findEditableDomPoint(root, selection.focus.offset);
        list.ownerDocument.getSelection()?.setBaseAndExtent(anchor.node, anchor.offset, focus.node, focus.offset);
        list.removeAttribute("data-notes-document-selection");
        if (list.hasAttribute("data-notes-painted-selection")) painter?.clear();
        return;
      }
    }
    list.setAttribute("data-notes-document-selection", "");
    const ids = options.readIds();
    const selected = notesDocumentRange(ids, selection);
    if (!selected) { painter?.clear(); return; }
    const selectedIds = new Set(selected.blockIds);
    const editors = new Map(Array.from(list.querySelectorAll<HTMLElement>(EDITABLE_BLOCK_SELECTOR))
      .map((root) => [root.dataset.notesBlockId!, root] as const));
    const roots = Array.from(list.querySelectorAll<HTMLElement>("[data-notes-selectable-block-id]"))
      .filter((node) => selectedIds.has(node.dataset.notesSelectableBlockId ?? ""));
    const ranges = roots.flatMap((row) => {
      const id = row.dataset.notesSelectableBlockId!;
      const root = editors.get(id);
      const range = list!.ownerDocument.createRange();
      if (root) {
        const start = findEditableDomPoint(root, id === selected.start.blockId ? selected.start.offset : 0);
        const end = findEditableDomPoint(root, id === selected.end.blockId ? selected.end.offset : blockTextLength(id));
        range.setStart(start.node, start.offset);
        range.setEnd(end.node, end.offset);
      } else {
        if ((id === selected.end.blockId && selected.end.offset === 0)
          || (id === selected.start.blockId && selected.start.offset >= blockTextLength(id))) return [];
        range.selectNodeContents(row.querySelector(ATOMIC_BLOCK_SELECTOR) ?? row.querySelector(".notes-block-surface") ?? row);
      }
      return range.collapsed ? [] : [range];
    });
    painter?.paint(ranges);
    const startRow = roots[0];
    const endRow = roots.at(-1);
    if (!startRow || !endRow) return;
    const startId = startRow.dataset.notesSelectableBlockId!;
    const endId = endRow.dataset.notesSelectableBlockId!;
    const startEditor = editors.get(startId);
    const endEditor = editors.get(endId);
    const startOffset = startId === selected.start.blockId ? selected.start.offset : 0;
    const endOffset = endId === selected.end.blockId ? selected.end.offset : blockTextLength(endId);
    const start = startEditor ? findEditableDomPoint(startEditor, startOffset) : { node: startRow, offset: 0 };
    const end = endEditor ? findEditableDomPoint(endEditor, endOffset) : { node: endRow, offset: endRow.childNodes.length };
    const forward = selected.start === selection.anchor;
    const anchor = forward ? start : end;
    const focus = forward ? end : start;
    list.ownerDocument.getSelection()?.setBaseAndExtent(anchor.node, anchor.offset, focus.node, focus.offset);
  }

  async function select(next: NotesDocumentSelection, keepKeyboardAnchor = false): Promise<void> {
    const ids = options.readIds();
    if (!ids.includes(next.anchor.blockId) || !ids.includes(next.focus.blockId)) return;
    selectionRequestId += 1;
    if (!keepKeyboardAnchor) resetVerticalNavigation();
    options.clearBlockSelection();
    if (next.anchor.blockId === next.focus.blockId && next.anchor.offset === next.focus.offset) {
      const root = editor(next.focus.blockId);
      if (root) {
        const dom = findEditableDomPoint(root, next.focus.offset);
        root.ownerDocument.getSelection()?.collapse(dom.node, dom.offset);
      }
      clear(keepKeyboardAnchor);
      return;
    }
    selection = next;
    if (pinnedIds.length) pinnedIds = [];
    if (keepKeyboardAnchor && next.anchor.blockId !== next.focus.blockId) {
      list?.setAttribute("data-notes-document-selection", "");
      schedulePaint();
    } else paint();
    if (!keepKeyboardAnchor && next.anchor.blockId === next.focus.blockId && editor(next.anchor.blockId) && !selectsEntireDocument()) {
      clear(); return;
    }
  }

  async function run(action: () => Promise<void>): Promise<void> {
    try { error = null; await action(); }
    catch (reason) { error = reason instanceof Error ? reason.message : String(reason); }
  }

  function selectsEntireDocument(): boolean {
    const selected = range();
    return !!selected && selected.start.offset === 0 && selected.end.offset === Number.MAX_SAFE_INTEGER
      && selected.blockIds.length === options.readIds().length;
  }

  async function hydrateSelection(ids: readonly string[]): Promise<void> {
    if (ids.every((id) => options.readBlock(id))) return;
    const token = selectionRequestId;
    const page = options.readPageId();
    pinnedIds = [...ids];
    await tick();
    if (!alive || token !== selectionRequestId || page !== options.readPageId()) return;
    await options.hydrate(ids);
  }

  function range() { return selection ? notesDocumentRange(options.readIds(), selection) : null; }

  function clipboardBlockIds(): string[] {
    const selected = range();
    return selected ? notesDocumentClipboardBlockIds(
      selected.blockIds,
      selected.start.offset,
      selected.end.offset,
      options.readBlock,
      options.outlineSubtreeIds,
    ) : [];
  }

  function clipboardContent(blockIds = clipboardBlockIds()) {
    const selected = range();
    if (!selected) return notesClipboardContent([]);
    return notesClipboardContent(blockIds.map((id) => {
      const block = options.readBlock(id);
      if (!block) throw new Error("Notes selection content is still loading");
      return { block, start: id === selected.blockIds[0] ? selected.start.offset : 0,
        end: id === selected.blockIds.at(-1) ? selected.end.offset : undefined };
    }), { pageId: options.readPageId(), unnamedDatabaseTitle: getLocalization().t("notes.databaseNewTitle") });
  }

  async function replace(value: string, html?: string): Promise<void> {
    if (replacementText !== null) { replacementText += value; return; }
    const selected = range();
    if (!selected) return;
    const page = options.readPageId();
    const token = selectionRequestId;
    replacementText = value;
    try {
      if (selected.blockIds.some((id) => !options.readBlock(id))) await hydrateSelection(selected.blockIds);
      if (!alive || token !== selectionRequestId || page !== options.readPageId()) return;
      const applied = await options.replace(selected.blockIds, selected.start.offset, selected.end.offset, replacementText ?? value, html, selection ?? undefined);
      if (applied === false) return;
      if (!alive || token !== selectionRequestId || page !== options.readPageId()) return;
      clear();
      options.restoreFocusAfterEdit?.();
    } finally { if (token === selectionRequestId) replacementText = null; }
  }

  /** Apply a copied destination to the selected words, hydrating only the selected block content. */
  async function pasteLink(value: string): Promise<boolean> {
    const url = notesPastedTextLinkUrl(value, list?.ownerDocument.defaultView?.location.href);
    const selected = range();
    if (!url || !selected || !options.link) return false;
    const token = selectionRequestId;
    const pageId = options.readPageId();
    await hydrateSelection(selected.blockIds);
    if (!alive || token !== selectionRequestId || pageId !== options.readPageId()) return true;
    const hasText = selected.blockIds.some((id) => {
      const block = options.readBlock(id);
      return block && isTextEditableBlock(block.type) && block.type !== "code"
        && (id === selected.end.blockId ? selected.end.offset : blockTextLength(id))
          > (id === selected.start.blockId ? selected.start.offset : 0);
    });
    if (!hasText) return false;
    await options.link(selected.blockIds, selected.start.offset, selected.end.offset, url, selection ?? undefined);
    await tick(); paint();
    return true;
  }

  async function copy(cut = false): Promise<void> {
    const selected = range();
    if (!selected) return;
    const page = options.readPageId();
    const token = selectionRequestId;
    if (selected.blockIds.some((id) => !options.readBlock(id))) await hydrateSelection(selected.blockIds);
    if (!alive || token !== selectionRequestId || page !== options.readPageId()) return;
    const blockIds = clipboardBlockIds();
    if (blockIds.some((id) => !options.readBlock(id))) await hydrateSelection(blockIds);
    if (!alive || token !== selectionRequestId || page !== options.readPageId()) return;
    await writeNotesClipboard(clipboardContent(blockIds));
    if (token !== selectionRequestId) return;
    if (cut) await replace("");
    menu = null;
  }

  async function format(annotation: NotesRichTextAnnotationName): Promise<void> {
    const selected = range();
    if (!selected) return;
    const token = selectionRequestId;
    await hydrateSelection(selected.blockIds);
    if (!alive || token !== selectionRequestId) return;
    await options.format(selected.blockIds, selected.start.offset, selected.end.offset, annotation, selection ?? undefined);
    await tick();
    paint();
  }

  function pointAt(x: number, y: number): NotesDocumentPoint | null {
    const doc = list?.ownerDocument;
    if (!doc) return null;
    const position = doc.caretPositionFromPoint?.(x, y);
    if (position) return point(position.offsetNode, position.offset);
    const caretDoc = doc as Document & { caretRangeFromPoint?: (x: number, y: number) => Range | null };
    const caret = caretDoc.caretRangeFromPoint?.(x, y);
    return caret ? point(caret.startContainer, caret.startOffset) : null;
  }

  /** Treat noninteractive space in a text row as part of its editing surface. */
  function pointerEditor(target: EventTarget | null): HTMLElement | null {
    if (!(target instanceof Element) || !list?.contains(target)) return null;
    const root = target.closest<HTMLElement>(`${EDITABLE_BLOCK_SELECTOR}, ${ATOMIC_BLOCK_SELECTOR}`);
    if (root) return root;
    if (target.closest("input, textarea, select, button, a, [role='button'], [role='menu'], [role='dialog'], [contenteditable='true']")) return null;
    const row = target.closest<HTMLElement>("[data-notes-selectable-block-id]");
    return row ? editor(row.dataset.notesSelectableBlockId!) ?? null : null;
  }

  /** Clamp padding and marker hits to the nearest caret position in the same editor. */
  function pointerPoint(root: HTMLElement, x: number, y: number): NotesDocumentPoint {
    if (root.matches(ATOMIC_BLOCK_SELECTOR)) {
      const rect = root.getBoundingClientRect();
      return { blockId: root.dataset.notesAtomicBlock!, offset: x < rect.left + rect.width / 2 ? 0 : 1 };
    }
    const id = root.dataset.notesBlockId!;
    const hit = pointAt(x, y);
    if (hit?.blockId === id) return hit;
    const rect = root.getBoundingClientRect();
    const insetX = Math.min(1, rect.width / 2);
    const insetY = Math.min(1, rect.height / 2);
    const clamped = pointAt(
      Math.min(Math.max(x, rect.left + insetX), rect.right - insetX),
      Math.min(Math.max(y, rect.top + insetY), rect.bottom - insetY),
    );
    if (clamped?.blockId === id) return clamped;
    return { blockId: id, offset: y < rect.top || x <= rect.left ? 0 : blockTextLength(id) };
  }

  function pointerDown(event: PointerEvent): void {
    if (event.defaultPrevented || event.button !== 0 || !(event.target instanceof Element) || event.target.closest("[data-notes-selection-menu]")) return;
    pointerAnchor = null;
    pointerFromRow = false;
    shouldSuppressAtomicClick = false;
    resetVerticalNavigation();
    const root = pointerEditor(event.target);
    if (!root) { clear(); return; }
    const hit = pointerPoint(root, event.clientX, event.clientY);
    const previous = selection ?? nativeSelection();
    pointerFromRow = !root.contains(event.target);
    options.clearBlockSelection();
    if (event.shiftKey && previous) {
      event.preventDefault(); event.stopPropagation();
      pointerAnchor = previous.anchor;
      void run(() => select({ anchor: previous.anchor, focus: hit }));
    } else {
      clear();
      pointerAnchor = hit;
      if (pointerFromRow) {
        event.preventDefault(); event.stopPropagation();
        root.focus({ preventScroll: true });
        options.focus(hit, true);
        const dom = findEditableDomPoint(root, hit.offset);
        root.ownerDocument.getSelection()?.collapse(dom.node, dom.offset);
      }
    }
  }

  function pointerMove(event: PointerEvent): void {
    if (!pointerAnchor || !(event.buttons & 1)) return;
    const root = pointerEditor(event.target);
    const hit = pointAt(event.clientX, event.clientY)
      ?? (root ? pointerPoint(root, event.clientX, event.clientY) : null);
    if (!hit || (hit.blockId === pointerAnchor.blockId && !selection && !pointerFromRow
      && options.readBlock(hit.blockId)?.type !== "child_page")) return;
    event.preventDefault();
    shouldSuppressAtomicClick = true;
    void run(() => select({ anchor: pointerAnchor!, focus: hit }));
  }

  async function verticalFocus(current: NotesDocumentPoint, forward: boolean, root: HTMLElement, epoch: number): Promise<NotesDocumentPoint | null> {
    const caret = notesCaretRectAtOffset(root, current.offset);
    const goalX = verticalGoalX ?? caret?.left ?? root.getBoundingClientRect().left + 1;
    verticalGoalX = goalX;
    const lines = notesEditableVisualLines(root);
    const lineIndex = verticalFocusLine?.blockId === current.blockId && verticalFocusLine.offset === current.offset
      && verticalFocusLine.index < lines.length
      ? verticalFocusLine.index : notesCaretVisualLineIndex(lines, caret);
    const nextLineIndex = lineIndex === null ? null : lineIndex + (forward ? 1 : -1);
    const nextLine = nextLineIndex === null ? undefined : lines[nextLineIndex];
    if (nextLine && nextLineIndex !== null) {
      const offset = notesCaretOffsetOnVisualLine(root, nextLine, goalX);
      if (offset !== null) {
        verticalFocusLine = { blockId: current.blockId, offset, index: nextLineIndex };
        return { blockId: current.blockId, offset };
      }
      const native = root.ownerDocument.getSelection();
      if (native?.modify) {
        const dom = findEditableDomPoint(root, current.offset);
        native.collapse(dom.node, dom.offset);
        native.modify("move", forward ? "forward" : "backward", "line");
        const moved = point(native.focusNode, native.focusOffset);
        if (moved?.blockId === current.blockId && moved.offset !== current.offset) {
          verticalFocusLine = { blockId: current.blockId, offset: moved.offset, index: nextLineIndex };
          return moved;
        }
      }
      return current;
    }
    const adjacent = adjacentTextId(current.blockId, forward);
    if (adjacent) {
      if (options.readBlock(adjacent)?.type === "child_page") {
        verticalFocusLine = null;
        return { blockId: adjacent, offset: forward ? 1 : 0 };
      }
      if (!options.readBlock(adjacent) || !editor(adjacent)) {
        extensionInProgress = true;
        const token = selectionRequestId;
        const page = options.readPageId();
        pinnedIds = [adjacent];
        if (!editor(adjacent)) options.focus({ blockId: adjacent, offset: forward ? 0 : blockTextLength(adjacent) });
        await tick();
        if (!alive || epoch !== extensionEpoch || token !== selectionRequestId || page !== options.readPageId()) return null;
        if (!options.readBlock(adjacent)) await options.hydrate([adjacent]);
        await tick();
        if (!alive || epoch !== extensionEpoch || token !== selectionRequestId || page !== options.readPageId()) return null;
      }
      const target = editor(adjacent);
      const targetLines = target ? notesEditableVisualLines(target) : [];
      const targetLine = forward ? targetLines[0] : targetLines.at(-1);
      const offset = target && targetLine ? notesCaretOffsetOnVisualLine(target, targetLine, goalX) : null;
      const nextOffset = offset ?? (forward ? 0 : blockTextLength(adjacent));
      verticalFocusLine = targetLine ? { blockId: adjacent, offset: nextOffset, index: forward ? 0 : targetLines.length - 1 } : null;
      return { blockId: adjacent, offset: nextOffset };
    }
    const boundaryOffset = forward ? blockTextLength(current.blockId) : 0;
    verticalFocusLine = lineIndex === null ? null : { blockId: current.blockId, offset: boundaryOffset, index: lineIndex };
    return { blockId: current.blockId, offset: boundaryOffset };
  }

  async function extend(event: KeyboardEvent, current: NotesDocumentSelection, epoch: number): Promise<void> {
    const ids = options.readIds();
    const forward = ["ArrowRight", "ArrowDown", "End"].includes(event.key);
    let focus = current.focus;
    if ((event.ctrlKey || event.metaKey) && ["Home", "End"].includes(event.key)) {
      focus = { blockId: forward ? ids[ids.length - 1] : ids[0], offset: forward ? Number.MAX_SAFE_INTEGER : 0 };
    } else {
      if (options.readBlock(focus.blockId)?.type === "child_page") {
        if ((forward && focus.offset === 0 && event.key === "ArrowRight")
          || (!forward && focus.offset > 0 && event.key === "ArrowLeft")) {
          focus = { blockId: focus.blockId, offset: forward ? 1 : 0 };
        } else {
          const adjacent = adjacentTextId(focus.blockId, forward);
          focus = adjacent ? { blockId: adjacent, offset: forward ? 0 : blockTextLength(adjacent) }
            : { blockId: focus.blockId, offset: forward ? 1 : 0 };
        }
        await select({ anchor: current.anchor, focus }, true);
        return;
      }
      if (!editor(focus.blockId)) {
        extensionInProgress = true;
        const token = selectionRequestId;
        options.focus(focus);
        await tick();
        if (!alive || epoch !== extensionEpoch || token !== selectionRequestId) return;
        if (!options.readBlock(focus.blockId)) await options.hydrate([focus.blockId]);
        await tick();
        if (!alive || epoch !== extensionEpoch || token !== selectionRequestId) return;
      }
      const root = editor(focus.blockId);
      if (!root) {
        const adjacent = adjacentTextId(focus.blockId, forward);
        if (adjacent) await select({ anchor: current.anchor, focus: { blockId: adjacent, offset: forward ? 0 : Number.MAX_SAFE_INTEGER } }, true);
        return;
      }
      if (event.key === "ArrowUp" || event.key === "ArrowDown") {
        const moved = await verticalFocus(focus, forward, root, epoch);
        if (!moved || epoch !== extensionEpoch) return;
        focus = moved;
      } else {
        const native = list?.ownerDocument.getSelection();
        if (!native) return;
        const dom = findEditableDomPoint(root, focus.offset);
        native.collapse(dom.node, dom.offset);
        const granularity = event.key === "Home" || event.key === "End" ? "lineboundary"
          : event.ctrlKey || event.metaKey ? "word" : "character";
        native.modify?.("move", forward ? "forward" : "backward", granularity);
        const moved = point(native.focusNode, native.focusOffset);
        if (moved && (moved.blockId !== focus.blockId || moved.offset !== focus.offset)) focus = moved;
        else if (event.key.startsWith("Arrow")) {
          const adjacent = adjacentTextId(focus.blockId, forward);
          if (adjacent) {
            if (!options.readBlock(adjacent) || !editor(adjacent)) {
              extensionInProgress = true;
              const token = selectionRequestId;
              const page = options.readPageId();
              pinnedIds = [adjacent];
              if (!editor(adjacent)) options.focus({ blockId: adjacent, offset: forward ? 0 : blockTextLength(adjacent) });
              await tick();
              if (!alive || epoch !== extensionEpoch || token !== selectionRequestId || page !== options.readPageId()) return;
              if (!options.readBlock(adjacent)) await options.hydrate([adjacent]);
              await tick();
              if (!alive || epoch !== extensionEpoch || token !== selectionRequestId || page !== options.readPageId()) return;
            }
            focus = { blockId: adjacent, offset: forward ? 0 : blockTextLength(adjacent) };
          }
        }
      }
    }
    await select({ anchor: current.anchor, focus }, true);
    if (focus.blockId !== current.focus.blockId) {
      editor(focus.blockId)?.scrollIntoView?.({ block: "nearest" });
    }
  }

  function keydown(event: KeyboardEvent): void {
    if (event.defaultPrevented || event.isComposing || !(event.target instanceof Element)) return;
    if (!event.target.closest(`${EDITABLE_BLOCK_SELECTOR}, ${ATOMIC_BLOCK_SELECTOR}`) && !event.target.matches("[data-notes-selectable-block-id]") && !selection) return;
    if (event.target.closest("[data-notes-selection-menu]")) return;
    shouldPasteAsPlainText = (event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === "v";
    if (!["ArrowUp", "ArrowDown", "Shift"].includes(event.key) || !event.shiftKey) resetVerticalNavigation();
    const modifier = event.ctrlKey || event.metaKey;
    if (replacementText !== null && !modifier && !event.altKey) {
      if (event.key.length === 1 || ["Enter", "Backspace", "Delete"].includes(event.key)) {
        event.preventDefault(); event.stopPropagation();
        if (event.key === "Backspace") replacementText = Array.from(replacementText).slice(0, -1).join("");
        else if (event.key !== "Delete") replacementText += event.key === "Enter" ? "\n" : event.key;
        return;
      }
    }
    if (modifier && !event.altKey && event.key.toLowerCase() === "a") {
      event.preventDefault(); event.stopPropagation();
      const ids = options.readIds();
      if (ids.length) void run(() => select({ anchor: { blockId: ids[0], offset: 0 }, focus: { blockId: ids[ids.length - 1], offset: Number.MAX_SAFE_INTEGER } }));
      return;
    }
    const atomic = event.target.closest<HTMLElement>(ATOMIC_BLOCK_SELECTOR);
    const atomicPoint = atomic ? { blockId: atomic.dataset.notesAtomicBlock!, offset: 0 } : null;
    const current = selection ?? (atomicPoint ? { anchor: atomicPoint, focus: atomicPoint } : nativeSelection());
    if (!selection && atomicPoint && !modifier && !event.shiftKey && !event.altKey
      && ["ArrowLeft", "ArrowRight"].includes(event.key)) {
      const forward = event.key === "ArrowRight";
      const adjacent = adjacentTextId(atomicPoint.blockId, forward);
      if (adjacent) {
        event.preventDefault(); event.stopPropagation();
        options.focus({ blockId: adjacent, offset: forward ? 0 : blockTextLength(adjacent) });
      }
      return;
    }
    if (modifier && !event.shiftKey && !event.altKey && ["Home", "End"].includes(event.key)) {
      const ids = options.readIds();
      const id = event.key === "Home" ? ids[0] : ids.at(-1);
      if (id) {
        event.preventDefault(); event.stopPropagation(); clear();
        options.focus({ blockId: id, offset: event.key === "Home" ? 0 : Number.MAX_SAFE_INTEGER });
      }
      return;
    }
    if (!selection && current && !modifier && !event.shiftKey && !event.altKey
      && current.anchor.blockId === current.focus.blockId && current.anchor.offset === current.focus.offset
      && ["ArrowLeft", "ArrowRight"].includes(event.key)) {
      const forward = event.key === "ArrowRight";
      if (current.focus.offset === (forward ? blockTextLength(current.focus.blockId) : 0)) {
        const adjacent = adjacentTextId(current.focus.blockId, forward);
        if (adjacent) {
          event.preventDefault(); event.stopPropagation();
          options.focus({ blockId: adjacent, offset: forward ? 0 : Number.MAX_SAFE_INTEGER });
          return;
        }
      }
    }
    if (event.shiftKey && !event.altKey && current && ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
      event.preventDefault(); event.stopPropagation();
      if (extensionInProgress) return;
      const epoch = extensionEpoch;
      void run(() => extend(event, current, epoch)).finally(() => {
        if (epoch === extensionEpoch) extensionInProgress = false;
      });
      return;
    }
    if (!selection) return;
    if (isNotesTabKey(event) && !modifier && !event.altKey && options.indent) {
      event.preventDefault(); event.stopPropagation();
      const selected = range();
      if (selected) void run(async () => {
        const token = selectionRequestId;
        await hydrateSelection(selected.blockIds);
        if (!alive || token !== selectionRequestId) return;
        await options.indent?.(selected.rowBlockIds, event.shiftKey ? "outdent" : "nest", selection ?? undefined);
        await tick(); paint();
      });
      return;
    }
    const key = event.key.toLowerCase();
    const annotation = notesRichTextFormattingShortcutAnnotationName(event);
    if (annotation) {
      event.preventDefault(); event.stopPropagation(); void run(() => format(annotation));
      return;
    }
    if (modifier && ["z", "y"].includes(key)) {
      event.preventDefault(); event.stopPropagation(); clear();
      void run(async () => { await (key === "y" || event.shiftKey ? options.redo() : options.undo()); });
    } else if (!modifier && ["Backspace", "Delete", "Enter"].includes(event.key)) {
      event.preventDefault(); event.stopPropagation();
      void run(() => replace(event.key === "Enter" ? "\n" : ""));
    } else if (!modifier && !event.altKey && event.key.length === 1) {
      event.preventDefault(); event.stopPropagation(); void run(() => replace(event.key));
    } else if (!modifier && (event.key.startsWith("Arrow") || event.key === "Escape")) {
      event.preventDefault(); event.stopPropagation();
      const selected = range();
      const target = selected && (["ArrowLeft", "ArrowUp"].includes(event.key) ? selected.start : selected.end);
      clear();
      if (target) {
        options.focus(target);
        const root = editor(target.blockId);
        if (root) { root.focus({ preventScroll: true }); const dom = findEditableDomPoint(root, target.offset); root.ownerDocument.getSelection()?.collapse(dom.node, dom.offset); }
      }
    }
  }

  function keyup(event: KeyboardEvent): void {
    if (!["Shift", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(event.key)) return;
    resetVerticalNavigation();
    if (event.key !== "Shift") return;
    if (!selection || selection.anchor.blockId !== selection.focus.blockId) return;
    if (editor(selection.anchor.blockId) && !selectsEntireDocument()) clear();
  }

  function clipboard(event: ClipboardEvent): void {
    const plainOnly = event.type === "paste" && shouldPasteAsPlainText;
    if (event.type === "paste") shouldPasteAsPlainText = false;
    if (!selection && event.target instanceof Element) {
      const atomic = event.target.closest<HTMLElement>(ATOMIC_BLOCK_SELECTOR);
      const id = atomic?.dataset.notesAtomicBlock;
      if (id) {
        if (event.type === "paste") {
          const plainText = event.clipboardData?.getData("text/plain") ?? "";
          const html = plainOnly ? undefined : notesClipboardPasteHtml(plainText, event.clipboardData?.getData("text/html") ?? "") || undefined;
          event.preventDefault(); event.stopPropagation();
          if (plainText || html) void run(async () => { await options.replace([id], 1, 1, plainText, html); });
          return;
        }
        selection = { anchor: { blockId: id, offset: 0 }, focus: { blockId: id, offset: 1 } };
      }
    }
    if (!selection) return;
    event.preventDefault(); event.stopPropagation();
    if (event.type === "paste") {
      const plainText = event.clipboardData?.getData("text/plain") ?? "";
      const html = plainOnly ? undefined : notesClipboardPasteHtml(plainText, event.clipboardData?.getData("text/html") ?? "") || undefined;
      if (plainText || html) void run(async () => { if (plainOnly || !await pasteLink(plainText)) await replace(plainText, html); });
      return;
    }
    const selected = range();
    if (selected?.blockIds.some((id) => !options.readBlock(id))) {
      void run(() => copy(event.type === "cut"));
      return;
    }
    try {
      const blockIds = clipboardBlockIds();
      if (blockIds.some((id) => !options.readBlock(id))) {
        void run(() => copy(event.type === "cut"));
        return;
      }
      if (!event.clipboardData) throw new Error("Notes clipboard is unavailable");
      setNotesClipboardData(event.clipboardData, clipboardContent(blockIds));
      if (event.type === "cut") void run(() => replace(""));
    } catch (reason) { error = reason instanceof Error ? reason.message : String(reason); }
  }

  function beforeInput(event: InputEvent): void {
    if (!selection) return;
    event.stopPropagation();
    if (composing) return;
    event.preventDefault();
    if (event.inputType === "insertFromComposition") return;
    if (event.inputType.startsWith("insert") && event.data !== null) void run(() => replace(event.data!));
    else if (event.inputType.startsWith("delete")) void run(() => replace(""));
  }

  function compositionStart(event: CompositionEvent): void {
    if (!selection || !(event.target instanceof Element)) return;
    const root = event.target.closest<HTMLElement>(EDITABLE_BLOCK_SELECTOR);
    if (!root) return;
    composing = true;
    list?.setAttribute("data-notes-document-selection-composing", "");
    painter?.clear();
    const offset = root.dataset.notesBlockId === selection.anchor.blockId ? selection.anchor.offset : 0;
    const dom = findEditableDomPoint(root, offset);
    root.ownerDocument.getSelection()?.collapse(dom.node, dom.offset);
  }

  function compositionEnd(event: CompositionEvent): void {
    if (!composing) return;
    composing = false;
    list?.removeAttribute("data-notes-document-selection-composing");
    if (event.data) void run(() => replace(event.data));
    else paint();
  }

  function input(event: Event): void {
    if (selection) event.stopPropagation();
  }

  function contextMenu(event: MouseEvent): void {
    if (!selection) return;
    event.preventDefault(); event.stopPropagation();
    menu = { x: event.clientX, y: event.clientY };
  }

  return {
    get pinnedIds() { return pinnedIds; },
    repaint: paint,
    get selection() { return selection; },
    get error() { return error; },
    get menu() { return menu; },
    clear, select, copy, replace, format, run,
    closeMenu() {
      menu = null;
      const root = selection ? editor(selection.anchor.blockId) ?? list?.querySelector<HTMLElement>(EDITABLE_BLOCK_SELECTOR) : null;
      root?.focus({ preventScroll: true });
      paint();
    },
    async paste() {
      const token = selectionRequestId;
      const { plainText, html } = await readNotesClipboard();
      if (token === selectionRequestId && (plainText || html) && !await pasteLink(plainText)) await replace(plainText, notesClipboardPasteHtml(plainText, html) || undefined);
    },
    delegation(node: HTMLDivElement) {
      list = node; alive = true;
      painter = createNotesDocumentSelectionPainter(node);
      const view = node.ownerDocument.defaultView;
      const repaint = () => { if (selection) schedulePaint(); };
      const resizeObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(repaint);
      resizeObserver?.observe(node);
      node.ownerDocument.addEventListener("scroll", repaint, true);
      view?.addEventListener("resize", repaint);
      const endPointerGesture = () => { pointerAnchor = null; pointerFromRow = false; };
      const click = (event: MouseEvent) => {
        if (shouldSuppressAtomicClick && event.target instanceof Element && event.target.closest(ATOMIC_BLOCK_SELECTOR)) {
          event.preventDefault(); event.stopPropagation();
        }
        shouldSuppressAtomicClick = false;
      };
      node.addEventListener("click", click, true);
      node.addEventListener("keydown", keydown, true);
      node.ownerDocument.addEventListener("keyup", keyup, true);
      node.addEventListener("pointerdown", pointerDown, true);
      node.addEventListener("contextmenu", contextMenu, true);
      node.addEventListener("beforeinput", beforeInput, true);
      node.addEventListener("compositionstart", compositionStart, true);
      node.addEventListener("compositionend", compositionEnd, true);
      node.addEventListener("input", input, true);
      for (const type of ["copy", "cut", "paste"] as const) node.addEventListener(type, clipboard, true);
      node.ownerDocument.addEventListener("pointermove", pointerMove);
      node.ownerDocument.addEventListener("pointerup", endPointerGesture);
      node.ownerDocument.addEventListener("pointercancel", endPointerGesture);
      return { destroy() {
        alive = false; clear();
        resizeObserver?.disconnect();
        node.ownerDocument.removeEventListener("scroll", repaint, true);
        view?.removeEventListener("resize", repaint);
        painter = null;
        node.removeEventListener("click", click, true);
        node.removeEventListener("keydown", keydown, true);
        node.ownerDocument.removeEventListener("keyup", keyup, true);
        node.removeEventListener("pointerdown", pointerDown, true);
        node.removeEventListener("contextmenu", contextMenu, true);
        node.removeEventListener("beforeinput", beforeInput, true);
        node.removeEventListener("compositionstart", compositionStart, true);
        node.removeEventListener("compositionend", compositionEnd, true);
        node.removeEventListener("input", input, true);
        for (const type of ["copy", "cut", "paste"] as const) node.removeEventListener(type, clipboard, true);
        node.ownerDocument.removeEventListener("pointermove", pointerMove);
        node.ownerDocument.removeEventListener("pointerup", endPointerGesture);
        node.ownerDocument.removeEventListener("pointercancel", endPointerGesture);
        list = null;
      } };
    },
  };
}
