import {
  chatComposerDocumentVersioned,
  chatComposerActiveMarks,
  chatComposerMarkdown,
  chatComposerMarksAt,
  chatComposerPlainText,
  normalizeChatComposerDocument,
  replaceChatComposerText,
  toggleChatComposerMark,
  type ChatComposerDocument,
  type ChatComposerMark,
  type ChatComposerSelection,
  type ChatComposerTextRun,
} from "./rich-text";
import type { ChatMessageReference } from "$lib/chat/contracts";
import {
  chatReferenceAdjacentToCaret,
  chatJsOffsetFromUtf8,
  copyChatReferenceSlice,
  expandEditRangeToChatReferences,
  insertChatMessageReference,
  normalizeChatMessageReferences,
  pasteChatReferenceSlice,
  rebaseChatMessageReferences,
  rebaseChatReferencesAfterInput,
  type ChatReferenceClipboardSlice,
} from "./message-references";

export interface ChatComposerEditorChange {
  document: ChatComposerDocument;
  markdown: string;
  plainText: string;
  richContent: ReturnType<typeof chatComposerDocumentVersioned>;
  references: ChatMessageReference[];
}

export interface ChatComposerEditorCallbacks {
  onChange(change: ChatComposerEditorChange): void;
  onSelectionChange(selection: ChatComposerSelection, activeMarks: ChatComposerMark[]): void;
}

interface EditorSnapshot {
  document: ChatComposerDocument;
  references: ChatMessageReference[];
  selection: ChatComposerSelection;
  storedMarks: ChatComposerMark[] | null;
}

interface EditorReferenceRange {
  reference: ChatMessageReference;
  start: number;
  end: number;
}

const HISTORY_LIMIT = 100;

/**
 * Owns every descendant of one Chat contenteditable root.
 *
 * The browser may mutate the root during input, especially during IME composition. The controller
 * reads that mutation into a typed document, then normalizes the DOM only when composition is not
 * active. Svelte must never render children inside the root.
 */
export class ChatComposerEditor {
  private document: ChatComposerDocument;
  private references: ChatMessageReference[];
  private composing = false;
  private storedMarks: ChatComposerMark[] | null = null;
  private pendingNativeHistory: EditorSnapshot | null = null;
  private readonly undoStack: EditorSnapshot[] = [];
  private readonly redoStack: EditorSnapshot[] = [];

  public constructor(
    private readonly root: HTMLDivElement,
    document: ChatComposerDocument,
    private readonly callbacks: ChatComposerEditorCallbacks,
    references: readonly ChatMessageReference[] = [],
  ) {
    this.document = normalizeChatComposerDocument(document);
    this.references = normalizeChatMessageReferences(chatComposerPlainText(this.document), references);
    this.render();
  }

  /** Returns whether an IME currently owns the browser DOM. */
  public get isComposing(): boolean {
    return this.composing;
  }

  /** Returns the visible text without Markdown formatting delimiters. */
  public plainText(): string {
    return chatComposerPlainText(this.document);
  }

  /** Returns immutable copies of the structured references currently represented as atoms. */
  public messageReferences(): ChatMessageReference[] {
    return this.references.map((reference) => ({
      ...reference,
      metadata: { ...reference.metadata },
    }));
  }

  /** Returns the current visible-text selection. */
  public selection(): ChatComposerSelection {
    return readEditorSelection(this.root, this.document) ?? {
      start: this.plainText().length,
      end: this.plainText().length,
    };
  }

  /** Focuses the editor and optionally restores a visible-text selection. */
  public focus(selection?: ChatComposerSelection): void {
    this.root.focus();
    if (selection) this.restoreSelection(selection);
  }

  /** Replaces editor state after an external draft or layout change. */
  public setDocument(document: ChatComposerDocument, references: readonly ChatMessageReference[] = []): void {
    const normalized = normalizeChatComposerDocument(document);
    const normalizedReferences = normalizeChatMessageReferences(chatComposerPlainText(normalized), references);
    if (sameDocument(this.document, normalized) && sameReferences(this.references, normalizedReferences)) return;
    const selection = clampSelection(this.selection(), chatComposerPlainText(normalized).length);
    this.document = normalized;
    this.references = normalizedReferences;
    this.storedMarks = null;
    this.pendingNativeHistory = null;
    this.undoStack.length = 0;
    this.redoStack.length = 0;
    this.render();
    if (this.root.ownerDocument.activeElement === this.root) this.restoreSelection(selection);
    this.notifySelection(selection);
  }

  /** Captures native editing intent before the browser changes the DOM. */
  public handleBeforeInput(event: InputEvent): boolean {
    if (event.isComposing || this.composing) return false;
    if (event.inputType === "historyUndo") {
      event.preventDefault();
      this.undo();
      return true;
    }
    if (event.inputType === "historyRedo") {
      event.preventDefault();
      this.redo();
      return true;
    }
    if (event.inputType === "formatBold" || event.inputType === "formatItalic") {
      event.preventDefault();
      this.toggleMark(event.inputType === "formatBold" ? "bold" : "italic");
      return true;
    }
    const selection = this.selection();
    const expanded = expandEditRangeToChatReferences(
      this.plainText(),
      this.references,
      selection.start,
      selection.end,
    );
    if (event.inputType === "deleteContentBackward") {
      const range = selection.start === selection.end
        ? chatReferenceAdjacentToCaret(this.plainText(), this.references, selection.start, "backward") ?? expanded
        : expanded;
      if (range.start !== selection.start || range.end !== selection.end) {
        event.preventDefault();
        this.applyReplacement(range, "", []);
        return true;
      }
    }
    if (event.inputType === "deleteContentForward") {
      const range = selection.start === selection.end
        ? chatReferenceAdjacentToCaret(this.plainText(), this.references, selection.start, "forward") ?? expanded
        : expanded;
      if (range.start !== selection.start || range.end !== selection.end) {
        event.preventDefault();
        this.applyReplacement(range, "", []);
        return true;
      }
    }
    if (
      (expanded.start !== selection.start || expanded.end !== selection.end)
      && event.inputType === "insertText"
      && event.data !== null
    ) {
      event.preventDefault();
      this.applyReplacement(expanded, event.data, this.storedMarks ?? []);
      return true;
    }
    if (event.inputType === "insertText" && event.data !== null && this.storedMarks !== null) {
      event.preventDefault();
      this.replaceSelection(event.data, this.storedMarks);
      return true;
    }
    this.pendingNativeHistory = this.snapshot();
    return false;
  }

  /** Reconciles a completed native DOM input into editor state. */
  public handleInput(): void {
    const previousText = chatComposerPlainText(this.document);
    const nextDocument = readChatComposerDocument(this.root);
    const nextText = chatComposerPlainText(nextDocument);
    const selection = readEditorSelection(this.root, nextDocument) ?? {
      start: chatComposerPlainText(nextDocument).length,
      end: chatComposerPlainText(nextDocument).length,
    };
    const changed = !sameDocument(this.document, nextDocument);
    if (changed) this.references = rebaseChatReferencesAfterInput(previousText, nextText, this.references);
    this.document = nextDocument;
    if (this.composing) {
      if (changed) this.notifyChange();
      this.notifySelection(selection);
      return;
    }
    if (changed && this.pendingNativeHistory) this.pushUndo(this.pendingNativeHistory);
    this.pendingNativeHistory = null;
    this.storedMarks = null;
    this.render();
    this.restoreSelection(selection);
    if (changed) this.notifyChange();
    this.notifySelection(selection);
  }

  /** Starts an IME transaction without normalizing its temporary DOM. */
  public handleCompositionStart(): void {
    if (!this.composing) this.pendingNativeHistory = this.snapshot();
    const selection = this.selection();
    const expanded = expandEditRangeToChatReferences(
      this.plainText(),
      this.references,
      selection.start,
      selection.end,
    );
    if (expanded.start !== selection.start || expanded.end !== selection.end) {
      this.restoreSelection(expanded);
    }
    this.composing = true;
  }

  /** Finishes an IME transaction after the browser commits its text. */
  public handleCompositionEnd(): void {
    this.composing = false;
    queueMicrotask(() => {
      const previousText = chatComposerPlainText(this.document);
      const nextDocument = readChatComposerDocument(this.root);
      const nextText = chatComposerPlainText(nextDocument);
      const selection = readEditorSelection(this.root, nextDocument) ?? {
        start: chatComposerPlainText(nextDocument).length,
        end: chatComposerPlainText(nextDocument).length,
      };
      const changed = !sameDocument(this.document, nextDocument);
      if (changed) this.references = rebaseChatReferencesAfterInput(previousText, nextText, this.references);
      this.document = nextDocument;
      if (this.pendingNativeHistory && !sameDocument(this.pendingNativeHistory.document, nextDocument)) {
        this.pushUndo(this.pendingNativeHistory);
      }
      this.pendingNativeHistory = null;
      this.storedMarks = null;
      this.render();
      this.restoreSelection(selection);
      if (changed) this.notifyChange();
      this.notifySelection(selection);
    });
  }

  /** Handles formatting, history, and soft-break shortcuts before the parent handles sending. */
  public handleKeydown(event: KeyboardEvent): boolean {
    if (this.composing || event.isComposing || event.keyCode === 229) return false;
    const hasCommandModifier = (event.ctrlKey || event.metaKey) && !event.altKey;
    if (hasCommandModifier && event.key.toLowerCase() === "b") {
      event.preventDefault();
      this.toggleMark("bold");
      return true;
    }
    if (hasCommandModifier && event.key.toLowerCase() === "i") {
      event.preventDefault();
      this.toggleMark("italic");
      return true;
    }
    if (hasCommandModifier && event.key.toLowerCase() === "z") {
      event.preventDefault();
      if (event.shiftKey) this.redo();
      else this.undo();
      return true;
    }
    if (hasCommandModifier && event.key.toLowerCase() === "y") {
      event.preventDefault();
      this.redo();
      return true;
    }
    if (event.key === "Enter" && event.shiftKey) {
      event.preventDefault();
      this.replaceSelection("\n");
      return true;
    }
    return false;
  }

  /** Synchronizes the active mark controls after pointer or keyboard selection changes. */
  public handleSelectionChange(): void {
    this.notifySelection(this.selection());
  }

  /** Inserts plain clipboard text and strips untrusted clipboard markup. */
  public insertPlainText(text: string): void {
    this.replaceSelection(text.replace(/\r\n?/gu, "\n"));
  }

  /** Inserts one noneditable structured reference atom at an explicit trigger range. */
  public insertReference(
    start: number,
    end: number,
    reference: ChatMessageReference,
  ): boolean {
    const previous = this.snapshot();
    const expanded = expandEditRangeToChatReferences(
      this.plainText(),
      this.references,
      start,
      end,
    );
    const result = insertChatMessageReference(
      this.plainText(),
      this.references,
      start,
      end,
      reference,
    );
    if (!result.inserted) return false;
    this.pushUndo(previous);
    this.document = replaceChatComposerText(
      this.document,
      expanded,
      result.text.slice(expanded.start, result.selection),
      [],
    ).document;
    this.references = result.references;
    this.storedMarks = null;
    this.render();
    this.restoreSelection({ start: result.selection, end: result.selection });
    this.notifyChange();
    this.notifySelection({ start: result.selection, end: result.selection });
    return true;
  }

  /** Removes one complete reference atom by stable reference identity. */
  public removeReference(referenceId: string): boolean {
    const reference = this.references.find((candidate) => candidate.metadata.referenceId === referenceId);
    if (!reference) return false;
    const text = this.plainText();
    const start = chatJsOffsetFromUtf8(text, reference.metadata.startOffset);
    const end = chatJsOffsetFromUtf8(text, reference.metadata.endOffset);
    this.applyReplacement({ start, end }, "", []);
    return true;
  }

  /** Returns a same-vault clipboard slice for the current selection. */
  public copyReferenceSlice(vaultId: string): ChatReferenceClipboardSlice | null {
    const selection = this.selection();
    if (selection.start === selection.end) return null;
    return copyChatReferenceSlice(
      vaultId,
      this.plainText(),
      this.references,
      selection.start,
      selection.end,
    );
  }

  /** Inserts trusted same-vault reference clipboard data at the current selection. */
  public pasteReferenceSlice(slice: ChatReferenceClipboardSlice): void {
    const selection = this.selection();
    const result = pasteChatReferenceSlice(
      this.plainText(),
      this.references,
      selection.start,
      selection.end,
      slice,
    );
    this.pushUndo(this.snapshot());
    const replacement = replaceChatComposerText(
      this.document,
      expandEditRangeToChatReferences(
        this.plainText(),
        this.references,
        selection.start,
        selection.end,
      ),
      slice.text,
      [],
    );
    this.document = replacement.document;
    this.references = result.references;
    this.render();
    this.restoreSelection({ start: result.selection, end: result.selection });
    this.notifyChange();
    this.notifySelection({ start: result.selection, end: result.selection });
  }

  /** Replaces an explicit visible-text range, used by mentions and commands. */
  public replaceRange(start: number, end: number, text: string): void {
    const selection = { start, end };
    this.applyReplacement(selection, text, chatComposerMarksAt(this.document, start));
  }

  /** Appends unformatted text while retaining the current rich content. */
  public appendPlainText(text: string): void {
    const offset = this.plainText().length;
    this.applyReplacement({ start: offset, end: offset }, text, []);
  }

  /** Toggles bold or italic for the current selection or future typed text. */
  public toggleMark(mark: ChatComposerMark): void {
    const selection = this.selection();
    if (selection.start === selection.end) {
      const current = this.storedMarks ?? chatComposerMarksAt(this.document, selection.start);
      this.storedMarks = current.includes(mark)
        ? current.filter((candidate) => candidate !== mark)
        : [...current, mark];
      this.notifySelection(selection);
      this.root.focus();
      return;
    }
    this.pushUndo(this.snapshot());
    this.document = toggleChatComposerMark(this.document, selection, mark);
    this.storedMarks = null;
    this.render();
    this.restoreSelection(selection);
    this.notifyChange();
    this.notifySelection(selection);
  }

  private replaceSelection(text: string, marks?: readonly ChatComposerMark[]): void {
    const selection = this.selection();
    this.applyReplacement(
      selection,
      text,
      marks ?? this.storedMarks ?? chatComposerMarksAt(this.document, selection.start),
    );
  }

  private applyReplacement(
    selection: ChatComposerSelection,
    text: string,
    marks: readonly ChatComposerMark[],
  ): void {
    const previousText = this.plainText();
    const expanded = expandEditRangeToChatReferences(
      previousText,
      this.references,
      selection.start,
      selection.end,
    );
    this.pushUndo(this.snapshot());
    const replacement = replaceChatComposerText(this.document, expanded, text, marks);
    this.document = replacement.document;
    this.references = rebaseChatMessageReferences(
      previousText,
      chatComposerPlainText(this.document),
      this.references,
      expanded.start,
      expanded.end,
    );
    this.render();
    this.restoreSelection(replacement.selection);
    this.notifyChange();
    this.notifySelection(replacement.selection);
  }

  private undo(): void {
    const previous = this.undoStack.pop();
    if (!previous) return;
    this.redoStack.push(this.snapshot());
    this.restoreSnapshot(previous);
  }

  private redo(): void {
    const next = this.redoStack.pop();
    if (!next) return;
    this.undoStack.push(this.snapshot());
    this.restoreSnapshot(next);
  }

  private restoreSnapshot(snapshot: EditorSnapshot): void {
    this.document = snapshot.document;
    this.references = snapshot.references;
    this.storedMarks = snapshot.storedMarks;
    this.pendingNativeHistory = null;
    this.render();
    this.restoreSelection(snapshot.selection);
    this.notifyChange();
    this.notifySelection(snapshot.selection);
  }

  private snapshot(): EditorSnapshot {
    return {
      document: this.document,
      references: this.messageReferences(),
      selection: this.selection(),
      storedMarks: this.storedMarks ? [...this.storedMarks] : null,
    };
  }

  private pushUndo(snapshot: EditorSnapshot): void {
    const previous = this.undoStack.at(-1);
    if (!previous || !sameDocument(previous.document, snapshot.document)
      || !sameReferences(previous.references, snapshot.references)
      || previous.selection.start !== snapshot.selection.start || previous.selection.end !== snapshot.selection.end) {
      this.undoStack.push(snapshot);
      if (this.undoStack.length > HISTORY_LIMIT) this.undoStack.shift();
    }
    this.redoStack.length = 0;
  }

  private render(): void {
    renderChatComposerDocument(this.root, this.document, this.references);
  }

  private restoreSelection(selection: ChatComposerSelection): void {
    if (this.root.ownerDocument.activeElement !== this.root) this.root.focus({ preventScroll: true });
    restoreEditorSelection(this.root, this.document, selection);
  }

  private notifyChange(): void {
    this.callbacks.onChange({
      document: this.document,
      markdown: chatComposerMarkdown(this.document),
      plainText: chatComposerPlainText(this.document),
      richContent: chatComposerDocumentVersioned(this.document),
      references: this.messageReferences(),
    });
  }

  private notifySelection(selection: ChatComposerSelection): void {
    const marks = this.storedMarks ?? chatComposerActiveMarks(this.document, selection);
    this.callbacks.onSelectionChange(selection, marks);
  }
}

/** Reads the supported rich-text subset from browser-owned editor DOM. */
export function readChatComposerDocument(root: ParentNode): ChatComposerDocument {
  const childNodes = [...root.childNodes];
  const blockChildren = childNodes.filter(isEditorBlock);
  if (blockChildren.length > 0 && blockChildren.length === childNodes.length) {
    return normalizeChatComposerDocument({
      lines: blockChildren.flatMap((block) => readInlineLines(block)),
    });
  }
  return normalizeChatComposerDocument({ lines: readInlineLines(root) });
}

/** Renders a composer document without assigning HTML strings. */
export function renderChatComposerDocument(
  root: HTMLDivElement,
  document: ChatComposerDocument,
  messageReferences: readonly ChatMessageReference[] = [],
): void {
  const owner = root.ownerDocument;
  const normalized = normalizeChatComposerDocument(document);
  const plainText = chatComposerPlainText(normalized);
  let documentOffset = 0;
  const referenceRanges = normalizeChatMessageReferences(
    plainText,
    messageReferences,
  ).map((reference) => ({
    reference,
    start: chatJsOffsetFromUtf8(plainText, reference.metadata.startOffset),
    end: chatJsOffsetFromUtf8(plainText, reference.metadata.endOffset),
  }));
  const lines = normalized.lines.map((line, lineIndex) => {
    const lineElement = owner.createElement("div");
    lineElement.dataset.chatComposerLine = "true";
    if (line.runs.length === 0) {
      const sentinel = owner.createElement("span");
      sentinel.dataset.chatComposerSentinel = "true";
      sentinel.textContent = "\u200b";
      lineElement.append(sentinel);
      if (lineIndex < normalized.lines.length - 1) documentOffset += 1;
      return lineElement;
    }
    for (const run of line.runs) {
      const runStart = documentOffset;
      const runEnd = runStart + run.text.length;
      lineElement.append(renderRunWithReferences(owner, run, runStart, runEnd, referenceRanges));
      documentOffset = runEnd;
    }
    if (lineIndex < normalized.lines.length - 1) documentOffset += 1;
    return lineElement;
  });
  root.replaceChildren(...lines);
  root.dataset.empty = chatComposerPlainText(normalized).length === 0 ? "true" : "false";
}

function readEditorSelection(
  root: HTMLDivElement,
  document: ChatComposerDocument,
): ChatComposerSelection | null {
  const selection = root.ownerDocument.getSelection();
  if (!selection?.anchorNode || !selection.focusNode) return null;
  if (!root.contains(selection.anchorNode) || !root.contains(selection.focusNode)) return null;
  const length = chatComposerPlainText(document).length;
  const anchor = Math.min(length, domPointOffset(root, selection.anchorNode, selection.anchorOffset));
  const focus = Math.min(length, domPointOffset(root, selection.focusNode, selection.focusOffset));
  return { start: Math.min(anchor, focus), end: Math.max(anchor, focus) };
}

function restoreEditorSelection(
  root: HTMLDivElement,
  document: ChatComposerDocument,
  selection: ChatComposerSelection,
): void {
  const browserSelection = root.ownerDocument.getSelection();
  if (!browserSelection) return;
  const length = chatComposerPlainText(document).length;
  const start = domPointAtOffset(root, document, Math.min(length, Math.max(0, selection.start)));
  const end = domPointAtOffset(root, document, Math.min(length, Math.max(0, selection.end)));
  const range = root.ownerDocument.createRange();
  range.setStart(start.node, start.offset);
  range.setEnd(end.node, end.offset);
  browserSelection.removeAllRanges();
  browserSelection.addRange(range);
}

function domPointOffset(root: HTMLDivElement, node: Node, offset: number): number {
  try {
    const range = root.ownerDocument.createRange();
    range.setStart(root, 0);
    range.setEnd(node, offset);
    const fragment = range.cloneContents();
    const holder = root.ownerDocument.createElement("div");
    holder.append(fragment);
    let length = chatComposerPlainText(readChatComposerDocument(holder)).length;
    if (node === root && offset > 0 && offset < root.childNodes.length) length += 1;
    return length;
  } catch {
    return 0;
  }
}

function domPointAtOffset(
  root: HTMLDivElement,
  document: ChatComposerDocument,
  offset: number,
): { node: Node; offset: number } {
  const normalized = normalizeChatComposerDocument(document);
  let remaining = offset;
  for (let index = 0; index < normalized.lines.length; index += 1) {
    const line = normalized.lines[index];
    const lineElement = root.children.item(index);
    if (!(lineElement instanceof HTMLElement)) return { node: root, offset: root.childNodes.length };
    const length = line.runs.reduce((total, run) => total + run.text.length, 0);
    if (remaining <= length) return textPointAtOffset(lineElement, remaining);
    remaining -= length;
    if (index < normalized.lines.length - 1) remaining -= 1;
  }
  return { node: root, offset: root.childNodes.length };
}

function textPointAtOffset(root: HTMLElement, offset: number): { node: Node; offset: number } {
  const walker = root.ownerDocument.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let remaining = offset;
  let last: Text | null = null;
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!(node instanceof Text)) continue;
    last = node;
    const length = node.data.length;
    if (remaining <= length) {
      const atom = node.parentElement?.closest<HTMLElement>("[data-chat-reference-id]");
      if (atom && atom.parentNode) {
        const index = [...atom.parentNode.childNodes].indexOf(atom);
        return {
          node: atom.parentNode,
          offset: index + (remaining === 0 ? 0 : 1),
        };
      }
      return { node, offset: remaining };
    }
    remaining -= length;
  }
  return last ? { node: last, offset: last.data.length } : { node: root, offset: 0 };
}

function readInlineLines(root: ParentNode): import("./rich-text").ChatComposerLine[] {
  const childNodes = [...root.childNodes];
  if (childNodes.length === 0 || (childNodes.length === 1 && childNodes[0] instanceof HTMLBRElement)) {
    return [{ runs: [] }];
  }
  const lines: import("./rich-text").ChatComposerLine[] = [{ runs: [] }];
  for (const node of root.childNodes) appendInlineNode(node, [], lines);
  return lines;
}

function appendInlineNode(
  node: Node,
  inheritedMarks: ChatComposerMark[],
  lines: import("./rich-text").ChatComposerLine[],
): void {
  if (node instanceof Text) {
    const text = node.data.replaceAll("\u200b", "");
    if (text.length > 0) appendRun(lines.at(-1), { text, marks: inheritedMarks });
    return;
  }
  if (!(node instanceof HTMLElement)) return;
  if (node instanceof HTMLBRElement) {
    if (!node.dataset.chatComposerSentinel) lines.push({ runs: [] });
    return;
  }
  const marks = elementMarks(node, inheritedMarks);
  for (const child of node.childNodes) appendInlineNode(child, marks, lines);
}

function appendRun(
  line: import("./rich-text").ChatComposerLine | undefined,
  run: ChatComposerTextRun,
): void {
  if (!line) return;
  const previous = line.runs.at(-1);
  if (previous && sameMarks(previous.marks, run.marks)) previous.text += run.text;
  else line.runs.push({ text: run.text, marks: [...run.marks] });
}

function elementMarks(element: HTMLElement, inherited: ChatComposerMark[]): ChatComposerMark[] {
  const marks = new Set(inherited);
  const tag = element.tagName;
  const weight = element.style.fontWeight;
  if (tag === "B" || tag === "STRONG" || weight === "bold" || Number.parseInt(weight, 10) >= 600) {
    marks.add("bold");
  }
  if (tag === "I" || tag === "EM" || element.style.fontStyle === "italic") marks.add("italic");
  return [...marks].sort((left, right) => left.localeCompare(right));
}

function renderRun(owner: Document, run: ChatComposerTextRun): Node {
  let node: Node = owner.createTextNode(run.text);
  if (run.marks.includes("italic")) {
    const italic = owner.createElement("em");
    italic.append(node);
    node = italic;
  }
  if (run.marks.includes("bold")) {
    const bold = owner.createElement("strong");
    bold.append(node);
    node = bold;
  }
  return node;
}

function renderRunWithReferences(
  owner: Document,
  run: ChatComposerTextRun,
  runStart: number,
  runEnd: number,
  referenceRanges: readonly EditorReferenceRange[],
): DocumentFragment {
  const fragment = owner.createDocumentFragment();
  let cursor = runStart;
  for (const range of referenceRanges) {
    if (range.start < runStart || range.end > runEnd || range.start < cursor) continue;
    if (range.start > cursor) {
      fragment.append(renderRun(owner, {
        text: run.text.slice(cursor - runStart, range.start - runStart),
        marks: run.marks,
      }));
    }
    const atom = owner.createElement("span");
    atom.dataset.chatReferenceId = range.reference.metadata.referenceId;
    atom.dataset.chatReferenceKind = range.reference.kind;
    atom.contentEditable = "false";
    atom.className = "chat-reference-atom";
    atom.setAttribute("aria-label", range.reference.metadata.labelSnapshot);
    atom.textContent = range.reference.metadata.plainTextProjection;
    fragment.append(wrapRunMarks(owner, atom, run.marks));
    cursor = range.end;
  }
  if (cursor < runEnd) {
    fragment.append(renderRun(owner, {
      text: run.text.slice(cursor - runStart),
      marks: run.marks,
    }));
  }
  return fragment;
}

function wrapRunMarks(owner: Document, content: Node, marks: readonly ChatComposerMark[]): Node {
  let node = content;
  if (marks.includes("italic")) {
    const italic = owner.createElement("em");
    italic.append(node);
    node = italic;
  }
  if (marks.includes("bold")) {
    const bold = owner.createElement("strong");
    bold.append(node);
    node = bold;
  }
  return node;
}

function isEditorBlock(node: Node): node is HTMLElement {
  return node instanceof HTMLElement && (node.dataset.chatComposerLine === "true" || node.tagName === "DIV" || node.tagName === "P");
}

function sameDocument(left: ChatComposerDocument, right: ChatComposerDocument): boolean {
  return JSON.stringify(normalizeChatComposerDocument(left)) === JSON.stringify(normalizeChatComposerDocument(right));
}

function sameReferences(left: readonly ChatMessageReference[], right: readonly ChatMessageReference[]): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function sameMarks(left: readonly ChatComposerMark[], right: readonly ChatComposerMark[]): boolean {
  return left.length === right.length && left.every((mark) => right.includes(mark));
}

function clampSelection(selection: ChatComposerSelection, length: number): ChatComposerSelection {
  return {
    start: Math.min(length, Math.max(0, selection.start)),
    end: Math.min(length, Math.max(0, selection.end)),
  };
}
