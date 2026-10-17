export interface NotesTextSelection {
  start: number;
  end: number;
}

export type NotesSelectionFallback = "start" | "end";

export interface NotesFocusSelectionInput {
  requestedSelection: NotesTextSelection | null;
  currentSelection: NotesTextSelection | null;
  textLength: number;
  fallback: NotesSelectionFallback;
}

export interface NotesSelectionViewportRect {
  top: number;
  right: number;
  bottom: number;
  left: number;
  width: number;
  height: number;
}

export interface NotesSelectionReconciliationInput {
  focusRequestIsNew: boolean;
  focusRequestedForEditor: boolean;
  requestedSelection: NotesTextSelection | null;
  currentSelection: NotesTextSelection | null;
  textLength: number;
  editorActive: boolean;
}

export interface NotesSelectionReconciliationPlan {
  focusEditor: boolean;
  selection: NotesTextSelection;
}

const TEXT_NODE = 3;
const ELEMENT_NODE = 1;
const DOCUMENT_FRAGMENT_NODE = 11;
const BROWSER_FILLER_TEXT_PATTERN = /^[\u00a0\u200b\ufeff]*$/u;

interface SelectionControl {
  selectionStart: number | null;
  selectionEnd: number | null;
}

/**
 * Read a normalized text selection from a textarea or input-like control.
 */
export function notesTextSelectionFromControl(control: SelectionControl): NotesTextSelection {
  const start = control.selectionStart ?? 0;
  const end = control.selectionEnd ?? start;
  return {
    start: Math.min(start, end),
    end: Math.max(start, end),
  };
}

/**
 * Keep a text selection inside the current plain text bounds.
 */
export function clampNotesTextSelection(
  selection: NotesTextSelection,
  textLength: number,
): NotesTextSelection {
  const max = Math.max(0, textLength);
  const start = Math.min(Math.max(0, selection.start), max);
  const end = Math.min(Math.max(0, selection.end), max);
  return {
    start: Math.min(start, end),
    end: Math.max(start, end),
  };
}

/**
 * Choose the selection to restore when a rich text editor regains focus.
 */
export function notesSelectionForFocus(
  input: NotesFocusSelectionInput,
): NotesTextSelection {
  const fallbackOffset = input.fallback === "start" ? 0 : input.textLength;
  return clampNotesTextSelection(
    input.requestedSelection
      ?? input.currentSelection
      ?? { start: fallbackOffset, end: fallbackOffset },
    input.textLength,
  );
}

/**
 * Choose one authoritative selection restoration after editor content changes.
 */
export function planNotesSelectionReconciliation(
  input: NotesSelectionReconciliationInput,
): NotesSelectionReconciliationPlan | null {
  if (input.focusRequestIsNew) {
    if (!input.focusRequestedForEditor) return null;
    return {
      focusEditor: true,
      selection: notesSelectionForFocus({
        requestedSelection: input.requestedSelection,
        currentSelection: input.currentSelection,
        textLength: input.textLength,
        fallback: "end",
      }),
    };
  }
  if (!input.editorActive || !input.currentSelection) return null;
  return {
    focusEditor: false,
    selection: clampNotesTextSelection(input.currentSelection, input.textLength),
  };
}

function isElementNode(node: Node): node is Element {
  return node.nodeType === ELEMENT_NODE;
}

function isNotesEditorSentinelElement(node: Node): boolean {
  return isElementNode(node)
    && (
      node.hasAttribute("data-notes-editor-sentinel")
      || node.getAttribute("data-notes-trailing-line-sentinel") === "true"
    );
}

function isNotesTrailingLineSentinelElement(node: Node): boolean {
  return isElementNode(node)
    && (
      node.getAttribute("data-notes-editor-sentinel") === "trailing-line"
      || node.getAttribute("data-notes-trailing-line-sentinel") === "true"
    );
}

function isNotesEmptyLineSentinelElement(node: Node): boolean {
  return isElementNode(node) && node.getAttribute("data-notes-editor-sentinel") === "empty-line";
}

function isNotesEditorLineElement(node: Node): boolean {
  return isElementNode(node) && node.getAttribute("data-notes-editor-line") === "true";
}

function nodeHasEditablePlainText(node: Node): boolean {
  if (node.nodeType === TEXT_NODE) {
    const text = node.textContent ?? "";
    return text.length > 0 && !BROWSER_FILLER_TEXT_PATTERN.test(text);
  }
  if (!isElementNode(node) && node.nodeType !== DOCUMENT_FRAGMENT_NODE) return false;
  if (isNotesEditorSentinelElement(node)) return false;
  if (isElementNode(node) && node.tagName === "BR") return false;
  return Array.from(node.childNodes).some(nodeHasEditablePlainText);
}

function nodeHasTrailingLineSentinel(node: Node): boolean {
  if (isNotesTrailingLineSentinelElement(node)) return true;
  if (!isElementNode(node) && node.nodeType !== DOCUMENT_FRAGMENT_NODE) return false;
  return Array.from(node.childNodes).some(nodeHasTrailingLineSentinel);
}

function nodeHasLineBreak(node: Node): boolean {
  if (isElementNode(node) && node.tagName === "BR") return true;
  if (!isElementNode(node) && node.nodeType !== DOCUMENT_FRAGMENT_NODE) return false;
  return Array.from(node.childNodes).some(nodeHasLineBreak);
}

function nodeHasMultipleEditorLines(node: Node): boolean {
  if (!isElementNode(node) && node.nodeType !== DOCUMENT_FRAGMENT_NODE) return false;
  let lineCount = 0;
  for (const child of Array.from(node.childNodes)) {
    if (isNotesEditorLineElement(child)) lineCount += 1;
    if (lineCount > 1 || nodeHasMultipleEditorLines(child)) return true;
  }
  return false;
}

function nodeHasSentinelBackedSoftLineBreak(node: Node): boolean {
  return nodeHasMultipleEditorLines(node) || (nodeHasTrailingLineSentinel(node) && nodeHasLineBreak(node));
}

function previousSiblingIsEditorLine(children: readonly Node[], index: number): boolean {
  for (let candidateIndex = index - 1; candidateIndex >= 0; candidateIndex -= 1) {
    const candidate = children[candidateIndex];
    if (!candidate) continue;
    if (isNotesEditorLineElement(candidate)) return true;
  }
  return false;
}

function appendEditablePlainText(node: Node, output: string[], root: Node): void {
  if (node.nodeType === TEXT_NODE) {
    output.push(node.textContent ?? "");
    return;
  }

  if (!isElementNode(node) && node.nodeType !== DOCUMENT_FRAGMENT_NODE) return;

  if (isElementNode(node)) {
    if (isNotesEditorSentinelElement(node)) return;
    if (node.tagName === "BR") {
      output.push("\n");
      return;
    }
    if (node !== root && ["DIV", "P"].includes(node.tagName)) {
      const previous = output.at(-1);
      if (output.length > 0 && previous !== "\n") output.push("\n");
    }
  }

  const children = Array.from(node.childNodes);
  children.forEach((child, index) => {
    if (isNotesEditorLineElement(child) && previousSiblingIsEditorLine(children, index)) {
      output.push("\n");
    }
    appendEditablePlainText(child, output, root);
  });
}

/** Marks the root element of an inline mention, which carets treat as one token. */
export const NOTES_MENTION_CHIP_ATTRIBUTE = "data-notes-mention-chip";

function isNotesMentionChipElement(node: Node): node is Element {
  return isElementNode(node) && node.hasAttribute(NOTES_MENTION_CHIP_ATTRIBUTE);
}

function editablePlainTextLength(node: Node): number {
  const output: string[] = [];
  appendEditablePlainText(node, output, node);
  return output.join("").length;
}

/**
 * Read only the plain text represented by the rich editable Notes surface.
 */
export function notesPlainTextFromEditableRoot(root: HTMLElement | DocumentFragment): string {
  if (!nodeHasEditablePlainText(root) && !nodeHasSentinelBackedSoftLineBreak(root)) return "";
  const output: string[] = [];
  appendEditablePlainText(root, output, root);
  return output.join("").replace(/\u00a0/gu, " ");
}

export function notesEditableOffsetFromDomPoint(
  root: HTMLElement,
  node: Node,
  offset: number,
): number | null {
  const range = root.ownerDocument.createRange();
  range.selectNodeContents(root);
  try {
    range.setEnd(node, offset);
  } catch {
    return null;
  }
  return notesPlainTextFromEditableRoot(range.cloneContents()).length;
}

/**
 * Read a normalized selection from a rich editable Notes surface.
 */
export function notesTextSelectionFromEditableRoot(root: HTMLElement): NotesTextSelection | null {
  const selection = root.ownerDocument.getSelection();
  const anchorNode = selection?.anchorNode ?? null;
  const focusNode = selection?.focusNode ?? null;
  if (!selection || selection.rangeCount === 0 || !anchorNode || !focusNode) return null;
  if (!root.contains(anchorNode) || !root.contains(focusNode)) return null;

  const anchor = notesEditableOffsetFromDomPoint(root, anchorNode, selection.anchorOffset);
  const focus = notesEditableOffsetFromDomPoint(root, focusNode, selection.focusOffset);
  if (anchor === null || focus === null) return null;

  return {
    start: Math.min(anchor, focus),
    end: Math.max(anchor, focus),
  };
}

/** A text selection that keeps which end the user is moving. */
export interface NotesDirectionalTextSelection {
  anchor: number;
  focus: number;
}

/** Read the selection's anchor and focus offsets from a rich editable Notes surface. */
export function notesDirectionalSelectionFromEditableRoot(root: HTMLElement): NotesDirectionalTextSelection | null {
  const selection = root.ownerDocument.getSelection();
  const anchorNode = selection?.anchorNode ?? null;
  const focusNode = selection?.focusNode ?? null;
  if (!selection || selection.rangeCount === 0 || !anchorNode || !focusNode) return null;
  if (!root.contains(anchorNode) || !root.contains(focusNode)) return null;
  const anchor = notesEditableOffsetFromDomPoint(root, anchorNode, selection.anchorOffset);
  const focus = notesEditableOffsetFromDomPoint(root, focusNode, selection.focusOffset);
  return anchor === null || focus === null ? null : { anchor, focus };
}

/** Place a directional selection in a rich editable Notes surface, keeping the anchor fixed. */
export function setNotesEditableDirectionalSelection(
  root: HTMLElement,
  selection: NotesDirectionalTextSelection,
): boolean {
  const windowSelection = root.ownerDocument.getSelection();
  if (!windowSelection) return false;
  const textLength = notesPlainTextFromEditableRoot(root).length;
  const anchor = findEditableDomPoint(root, Math.min(Math.max(0, selection.anchor), textLength));
  const focus = findEditableDomPoint(root, Math.min(Math.max(0, selection.focus), textLength));
  windowSelection.setBaseAndExtent(anchor.node, anchor.offset, focus.node, focus.offset);
  return true;
}

/** Set on a mention chip that the native selection covers, so the chip can paint its icon as selected. */
export const NOTES_SELECTED_MENTION_CHIP_ATTRIBUTE = "data-notes-selected";

/** Mark exactly the mention chips that intersect a non-collapsed selection inside the editable root. */
export function markNotesSelectedMentionChips(root: HTMLElement): void {
  const selection = notesTextSelectionFromEditableRoot(root);
  const ranged = selection !== null && selection.start !== selection.end;
  for (const chip of root.querySelectorAll(`[${NOTES_MENTION_CHIP_ATTRIBUTE}]`)) {
    const parent = chip.parentNode;
    const start = ranged && parent
      ? notesEditableOffsetFromDomPoint(root, parent, Array.prototype.indexOf.call(parent.childNodes, chip))
      : null;
    const selected = start !== null && selection !== null
      && start < selection.end && start + editablePlainTextLength(chip) > selection.start;
    if (selected) chip.setAttribute(NOTES_SELECTED_MENTION_CHIP_ATTRIBUTE, "true");
    else chip.removeAttribute(NOTES_SELECTED_MENTION_CHIP_ATTRIBUTE);
  }
}

/**
 * Keep mention chip selection marks in step with the document selection. Pass the rendered rich
 * text as the parameter so chips recreated by a content update are marked again.
 */
export function notesMentionChipSelectionAction(root: HTMLElement, _content?: unknown) {
  const owner = root.ownerDocument;
  let marked = false;
  const refresh = (): void => {
    const anchorNode = owner.getSelection()?.anchorNode ?? null;
    const inside = anchorNode !== null && root.contains(anchorNode);
    if (!inside && !marked) return;
    markNotesSelectedMentionChips(root);
    marked = inside;
  };
  owner.addEventListener("selectionchange", refresh);
  return {
    update(): void { refresh(); },
    destroy(): void { owner.removeEventListener("selectionchange", refresh); },
  };
}

function rectFromDomRect(rect: DOMRect): NotesSelectionViewportRect | null {
  if (rect.width <= 0 && rect.height <= 0) return null;
  return {
    top: rect.top,
    right: rect.right,
    bottom: rect.bottom,
    left: rect.left,
    width: rect.width,
    height: rect.height,
  };
}

export function notesEditableSelectionViewportRect(
  root: HTMLElement,
): NotesSelectionViewportRect | null {
  const selection = root.ownerDocument.getSelection();
  const anchorNode = selection?.anchorNode ?? null;
  const focusNode = selection?.focusNode ?? null;
  if (!selection || selection.rangeCount === 0 || !anchorNode || !focusNode) return null;
  if (selection.isCollapsed || !root.contains(anchorNode) || !root.contains(focusNode)) {
    return null;
  }
  const range = selection.getRangeAt(0);
  const boundingRect = rectFromDomRect(range.getBoundingClientRect());
  if (boundingRect) return boundingRect;
  for (const rect of Array.from(range.getClientRects())) {
    const usableRect = rectFromDomRect(rect);
    if (usableRect) return usableRect;
  }
  return null;
}

interface EditableDomPoint {
  node: Node;
  offset: number;
}

/** Resolve a UTF-16 text offset to a DOM point, including soft line breaks. */
export function findEditableDomPoint(root: HTMLElement, textOffset: number): EditableDomPoint {
  let remaining = Math.max(0, textOffset);
  let fallback: EditableDomPoint = { node: root, offset: 0 };

  function visit(parent: Node): EditableDomPoint | null {
    const children = Array.from(parent.childNodes);
    for (let index = 0; index < children.length; index += 1) {
      const child = children[index];
      if (!child) continue;

      if (child.nodeType === TEXT_NODE) {
        const text = child.textContent ?? "";
        if (remaining <= text.length) return { node: child, offset: remaining };
        remaining -= text.length;
        fallback = { node: child, offset: text.length };
        continue;
      }

      if (isNotesEmptyLineSentinelElement(child) && remaining === 0) {
        return { node: child.firstChild ?? child, offset: 0 };
      }

      if (isNotesEditorSentinelElement(child)) {
        continue;
      }

      if (isNotesMentionChipElement(child)) {
        // Mention edges resolve outside the chip so the caret never sits between its icon and label.
        const length = editablePlainTextLength(child);
        if (remaining === 0) return { node: parent, offset: index };
        if (remaining >= length) {
          remaining -= length;
          fallback = { node: parent, offset: index + 1 };
          if (remaining === 0) return fallback;
          continue;
        }
      }

      if (isNotesEditorLineElement(child) && previousSiblingIsEditorLine(children, index)) {
        if (remaining === 0) return { node: child, offset: 0 };
        remaining -= 1;
        fallback = { node: child, offset: 0 };
      }

      if (isElementNode(child) && child.tagName === "BR") {
        if (remaining === 0) return { node: parent, offset: index };
        remaining -= 1;
        fallback = { node: parent, offset: index + 1 };
        continue;
      }

      if (child.nodeType === ELEMENT_NODE || child.nodeType === DOCUMENT_FRAGMENT_NODE) {
        const nested = visit(child);
        if (nested) return nested;
        const childIndex = children.indexOf(child);
        fallback = { node: parent, offset: childIndex + 1 };
      }
    }
    return null;
  }

  return visit(root) ?? fallback;
}

/**
 * Restore a Notes text selection inside a rich editable surface.
 */
export function restoreNotesEditableSelection(
  root: HTMLElement,
  selection: NotesTextSelection,
): boolean {
  const textLength = notesPlainTextFromEditableRoot(root).length;
  const safeSelection = clampNotesTextSelection(selection, textLength);
  const start = findEditableDomPoint(root, safeSelection.start);
  const end = findEditableDomPoint(root, safeSelection.end);
  const range = root.ownerDocument.createRange();
  const windowSelection = root.ownerDocument.getSelection();
  if (!windowSelection) return false;

  range.setStart(start.node, start.offset);
  range.setEnd(end.node, end.offset);
  windowSelection.removeAllRanges();
  windowSelection.addRange(range);
  return true;
}

/** A stable UTF-16 position in the page's ordered block document. */
export interface NotesDocumentPoint {
  blockId: string;
  offset: number;
}

/** Directional selection, independent of which editors are mounted. */
export interface NotesDocumentSelection {
  anchor: NotesDocumentPoint;
  focus: NotesDocumentPoint;
}

/** Normalize text endpoints and identify rows affected by line-level commands. */
export function notesDocumentRange(ids: readonly string[], selection: NotesDocumentSelection) {
  const anchorIndex = ids.indexOf(selection.anchor.blockId);
  const focusIndex = ids.indexOf(selection.focus.blockId);
  if (anchorIndex < 0 || focusIndex < 0) return null;
  const forward = anchorIndex < focusIndex
    || (anchorIndex === focusIndex && selection.anchor.offset <= selection.focus.offset);
  const start = forward ? selection.anchor : selection.focus;
  const end = forward ? selection.focus : selection.anchor;
  const blockIds = ids.slice(Math.min(anchorIndex, focusIndex), Math.max(anchorIndex, focusIndex) + 1);
  return {
    start,
    end,
    // Text operations retain the endpoint block to copy or replace the selected line break.
    blockIds,
    // Reaching a row's start does not select that row for indentation.
    rowBlockIds: blockIds.length > 1 && end.offset === 0 ? blockIds.slice(0, -1) : blockIds,
  };
}

let documentHighlightSequence = 0;

/** Paint document selection independently of the active contenteditable host. */
export function createNotesDocumentSelectionPainter(list: HTMLElement) {
  const document = list.ownerDocument;
  const name = `ganbaru-notes-selection-${++documentHighlightSequence}`;
  const view = document.defaultView as (Window & {
    Highlight?: new (...ranges: Range[]) => unknown;
    CSS?: { highlights?: { set: (key: string, value: unknown) => void; delete: (key: string) => boolean } };
  }) | null;
  const registry = view?.CSS?.highlights;
  const HighlightConstructor = view?.Highlight;
  const style = document.createElement("style");
  const paintedScope = `[data-notes-painted-selection="${name}"]`;
  style.textContent = `
    ::highlight(${name}) { background-color: var(--selection-background, Highlight); }
    ${paintedScope}::selection, ${paintedScope} *::selection { background-color: transparent; }
  `;
  let overlay: HTMLDivElement | null = null;

  function clear(): void {
    registry?.delete(name);
    style.remove();
    overlay?.remove();
    overlay = null;
    list.removeAttribute("data-notes-painted-selection");
  }

  function paint(ranges: readonly Range[]): void {
    clear();
    if (!ranges.length) return;
    if (registry && typeof HighlightConstructor === "function") {
      document.head.append(style);
      registry.set(name, new HighlightConstructor(...ranges));
      list.setAttribute("data-notes-painted-selection", name);
      return;
    }

    // Older webviews need an inert overlay; never wrap or mutate editable text.
    const bounds = list.getBoundingClientRect();
    const scaleX = list.offsetWidth ? bounds.width / list.offsetWidth : 1;
    const scaleY = list.offsetHeight ? bounds.height / list.offsetHeight : 1;
    const rectangles = ranges.flatMap((range) => Array.from(range.getClientRects?.() ?? []))
      .filter((rect) => rect.width > 0 && rect.height > 0)
      .map((rect) => ({ left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom }))
      .sort((left, right) => left.top - right.top || left.left - right.left);
    const merged: typeof rectangles = [];
    for (const rect of rectangles) {
      const previous = merged.at(-1);
      if (previous && Math.abs(previous.top - rect.top) < 0.5 && Math.abs(previous.bottom - rect.bottom) < 0.5
        && rect.left <= previous.right) previous.right = Math.max(previous.right, rect.right);
      else merged.push({ ...rect });
    }
    // Keep native highlighting when there are no visible rectangles to replace it.
    if (!merged.length) return;
    overlay = document.createElement("div");
    overlay.dataset.notesSelectionOverlay = "";
    overlay.setAttribute("aria-hidden", "true");
    overlay.style.cssText = "position:absolute;inset:0;pointer-events:none;user-select:none;overflow:hidden;z-index:1";
    for (const rect of merged) {
      const highlight = document.createElement("span");
      highlight.style.cssText = "position:absolute;background-color:var(--selection-background, Highlight);pointer-events:none";
      highlight.style.left = `${(rect.left - bounds.left) / scaleX + list.scrollLeft}px`;
      highlight.style.top = `${(rect.top - bounds.top) / scaleY + list.scrollTop}px`;
      highlight.style.width = `${(rect.right - rect.left) / scaleX}px`;
      highlight.style.height = `${(rect.bottom - rect.top) / scaleY}px`;
      overlay.append(highlight);
    }
    list.append(overlay);
    document.head.append(style);
    list.setAttribute("data-notes-painted-selection", name);
  }

  return { paint, clear };
}
