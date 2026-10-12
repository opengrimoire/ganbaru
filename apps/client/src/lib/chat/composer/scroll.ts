/** Resolves the native scroll position that fully reveals one caret line. */
export function composerScrollTopForCaret(
  currentScrollTop: number,
  caretTop: number,
  lineHeight: number,
  scrollHeight: number,
  clientHeight: number,
): number {
  const maximumScrollTop = Math.max(0, scrollHeight - clientHeight);
  const caretBottom = caretTop + lineHeight;
  if (caretTop < currentScrollTop) {
    return Math.min(maximumScrollTop, Math.max(0, caretTop));
  }
  if (caretBottom > currentScrollTop + clientHeight) {
    return Math.min(maximumScrollTop, Math.max(0, caretBottom - clientHeight));
  }
  return Math.min(maximumScrollTop, Math.max(0, currentScrollTop));
}

/** Reveals the current contenteditable caret on a complete line. */
export function revealContenteditableComposerCaret(root: HTMLDivElement): void {
  const selection = root.ownerDocument.getSelection();
  if (!selection || selection.rangeCount === 0) return;
  const range = selection.getRangeAt(0).cloneRange();
  if (!root.contains(range.endContainer)) return;
  range.collapse(false);
  const rootBounds = root.getBoundingClientRect();
  const fallbackLine = composerLineElement(range.endContainer, root);
  const fallbackBounds = fallbackLine?.getBoundingClientRect();
  const caretBounds = typeof range.getBoundingClientRect === "function"
    ? range.getBoundingClientRect()
    : null;
  const caretTop = (caretBounds && (caretBounds.height > 0 || caretBounds.top !== 0)
    ? caretBounds.top
    : fallbackBounds?.top ?? rootBounds.top) - rootBounds.top + root.scrollTop;
  revealComposerCaret(root, caretTop);
}

function revealComposerCaret(element: HTMLElement, caretTop: number): void {
  const lineHeight = composerLineHeight(element);
  const next = composerScrollTopForCaret(
    element.scrollTop,
    caretTop,
    lineHeight,
    element.scrollHeight,
    element.clientHeight,
  );
  if (Math.abs(element.scrollTop - next) > 0.25) element.scrollTop = next;
}

function composerLineHeight(element: HTMLElement): number {
  const value = element.ownerDocument.defaultView?.getComputedStyle(element).lineHeight ?? "";
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 1;
}

function composerLineElement(node: Node, root: HTMLDivElement): HTMLElement | null {
  const element = node instanceof HTMLElement ? node : node.parentElement;
  const line = element?.closest<HTMLElement>("[data-chat-composer-line]") ?? null;
  return line && root.contains(line) ? line : null;
}
