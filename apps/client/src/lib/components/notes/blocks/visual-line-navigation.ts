import { findEditableDomPoint, notesEditableOffsetFromDomPoint } from "$lib/notes/editor/selection";

export interface NotesEditableVisualLine {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

function hasVisibleHeight(rect: DOMRect): boolean {
  return Number.isFinite(rect.top) && Number.isFinite(rect.bottom) && rect.bottom > rect.top;
}

function addVisualLine(lines: NotesEditableVisualLine[], rect: DOMRect): void {
  if (!hasVisibleHeight(rect)) return;
  const existing = lines.find((line) => {
    const overlap = Math.min(line.bottom, rect.bottom) - Math.max(line.top, rect.top);
    return overlap > 0 && overlap >= Math.min(line.bottom - line.top, rect.height) / 2;
  });
  if (existing) {
    existing.top = Math.min(existing.top, rect.top);
    existing.right = Math.max(existing.right, rect.right);
    existing.bottom = Math.max(existing.bottom, rect.bottom);
    existing.left = Math.min(existing.left, rect.left);
  } else {
    lines.push({ top: rect.top, right: rect.right, bottom: rect.bottom, left: rect.left });
  }
}

/** Find rendered text lines, including wrapped text and empty rich-text lines. */
export function notesEditableVisualLines(editor: HTMLElement): NotesEditableVisualLine[] {
  const lines: NotesEditableVisualLine[] = [];
  const range = editor.ownerDocument.createRange();
  const walker = editor.ownerDocument.createTreeWalker(editor, 4);
  while (walker.nextNode()) {
    const node = walker.currentNode;
    if (!node.textContent) continue;
    range.selectNodeContents(node);
    for (const rect of Array.from(range.getClientRects?.() ?? [])) addVisualLine(lines, rect);
  }
  for (const line of editor.querySelectorAll<HTMLElement>("[data-notes-editor-line='true']")) {
    if (line.querySelector("[data-notes-editor-sentinel='empty-line']")) {
      addVisualLine(lines, line.getBoundingClientRect());
    }
  }
  if (lines.length === 0) addVisualLine(lines, editor.getBoundingClientRect());
  return lines.sort((left, right) => left.top - right.top);
}

/** Read caret geometry from an offset without changing the browser selection. */
export function notesCaretRectAtOffset(editor: HTMLElement, offset: number): DOMRect | null {
  const dom = findEditableDomPoint(editor, offset);
  const range = editor.ownerDocument.createRange();
  range.setStart(dom.node, dom.offset);
  range.collapse(true);
  const rect = range.getBoundingClientRect?.();
  if (rect && hasVisibleHeight(rect)) return rect;
  const clientRect = Array.from(range.getClientRects?.() ?? []).find(hasVisibleHeight);
  if (clientRect) return clientRect;
  if (dom.node.nodeType === 3) {
    const textLength = dom.node.textContent?.length ?? 0;
    if (dom.offset < textLength) {
      range.setEnd(dom.node, dom.offset + 1);
      const next = Array.from(range.getClientRects?.() ?? []).find(hasVisibleHeight);
      if (next) return new DOMRect(next.left, next.top, 0, next.height);
    }
    if (dom.offset > 0) {
      range.setStart(dom.node, dom.offset - 1);
      const previous = Array.from(range.getClientRects?.() ?? []).find(hasVisibleHeight);
      if (previous) return new DOMRect(previous.right, previous.top, 0, previous.height);
    }
  }
  const element = dom.node instanceof Element ? dom.node : dom.node.parentElement;
  const line = element?.closest<HTMLElement>("[data-notes-editor-line='true']");
  const fallback = line?.getBoundingClientRect() ?? editor.getBoundingClientRect();
  return hasVisibleHeight(fallback) ? new DOMRect(fallback.left, fallback.top, 0, fallback.height) : null;
}

/** Match a caret rectangle to its current rendered line. */
export function notesCaretVisualLineIndex(lines: readonly NotesEditableVisualLine[], caret: DOMRect | null): number | null {
  if (!caret || lines.length === 0) return null;
  const center = (caret.top + caret.bottom) / 2;
  let closest = 0;
  for (let index = 1; index < lines.length; index += 1) {
    const current = lines[index];
    const best = lines[closest];
    if (Math.abs((current.top + current.bottom) / 2 - center) < Math.abs((best.top + best.bottom) / 2 - center)) closest = index;
  }
  return closest;
}

/** Hit-test one rendered line at the preferred horizontal caret position. */
export function notesCaretOffsetOnVisualLine(editor: HTMLElement, line: NotesEditableVisualLine, x: number): number | null {
  const rect = editor.getBoundingClientRect();
  const left = rect.left + 1;
  const right = rect.right - 1;
  const safeX = right <= left ? rect.left : Math.min(Math.max(x, left), right);
  const y = (line.top + line.bottom) / 2;
  const doc = editor.ownerDocument;
  const position = doc.caretPositionFromPoint?.(safeX, y);
  if (position && editor.contains(position.offsetNode)) {
    return notesEditableOffsetFromDomPoint(editor, position.offsetNode, position.offset);
  }
  const range = doc.caretRangeFromPoint?.(safeX, y);
  if (range && editor.contains(range.startContainer)) {
    return notesEditableOffsetFromDomPoint(editor, range.startContainer, range.startOffset);
  }
  return null;
}
