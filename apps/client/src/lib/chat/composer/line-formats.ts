import type { ChatComposerSelection } from "./rich-text";

/**
 * Line formats a Chat composer applies as Markdown line syntax in the visible text.
 *
 * Unmarked composer text is stored as written, so a list marker, quote marker, or code fence the
 * composer inserts is the same Markdown a person could type, and it renders, edits again, and
 * syncs without a separate block model.
 */
export const CHAT_COMPOSER_LINE_FORMATS = ["bulletList", "orderedList", "blockquote", "codeBlock"] as const;

export type ChatComposerLineFormat = (typeof CHAT_COMPOSER_LINE_FORMATS)[number];

/** One replacement of a visible text range, expressed in offsets of the text before any edit. */
export interface ChatComposerTextEdit {
  start: number;
  end: number;
  text: string;
  /** Whether a caret exactly at an insertion point stays before the inserted text. */
  keepBefore?: boolean;
}

/** The edits a line format toggle makes, with the selection and code range after them. */
export interface ChatComposerLineFormatResult {
  /** Ascending, non-overlapping edits in offsets of the original text. */
  edits: ChatComposerTextEdit[];
  /** The selection mapped into the edited text. */
  selection: ChatComposerSelection;
  /** A range of the edited text that became code block content and must lose inline marks. */
  codeRange: ChatComposerSelection | null;
}

interface TextLine {
  start: number;
  end: number;
  text: string;
}

type CodeLineState = { kind: "text" } | { kind: "fence" | "code"; block: number };

interface LinePrefix {
  quote: string;
  list: { kind: "bulletList" | "orderedList"; marker: string; number: number; delimiter: string } | null;
  /** Length of the quote and list markers together. */
  length: number;
}

const QUOTE_PREFIX = /^> ?/u;
const LIST_PREFIX = /^(?:([-*+])|(\d{1,9})([.)])) /u;
const OPENING_FENCE = /^(`{3,})/u;

/** Returns whether each line format covers every line the selection touches. */
export function chatComposerLineFormatState(
  text: string,
  selection: ChatComposerSelection,
): Record<ChatComposerLineFormat, boolean> {
  const lines = textLines(text);
  const states = codeLineStates(lines);
  const touched = touchedLineIndexes(lines, selection);
  return {
    bulletList: listIsActive(lines, states, touched, "bulletList"),
    orderedList: listIsActive(lines, states, touched, "orderedList"),
    blockquote: quoteIsActive(lines, states, touched),
    codeBlock: enclosingCodeBlock(states, touched) !== null,
  };
}

/**
 * Toggles one line format on every line the selection touches, or on the caret line.
 *
 * Lists and quotes skip code block lines. A list toggle replaces another list kind instead of
 * nesting it, and numbers ordered lines from one. A code block toggle wraps touched text lines in
 * a fence, or removes the fences of the block that holds every touched line; a selection that
 * mixes text and code lines changes nothing.
 */
export function toggleChatComposerLineFormat(
  text: string,
  selection: ChatComposerSelection,
  format: ChatComposerLineFormat,
): ChatComposerLineFormatResult {
  const lines = textLines(text);
  const states = codeLineStates(lines);
  const touched = touchedLineIndexes(lines, selection);
  const edits = format === "codeBlock"
    ? codeBlockEdits(lines, states, touched)
    : format === "blockquote"
      ? quoteEdits(lines, states, touched)
      : listEdits(lines, states, touched, format);
  const mapped = {
    start: mapChatComposerOffset(Math.min(selection.start, selection.end), edits),
    end: mapChatComposerOffset(Math.max(selection.start, selection.end), edits),
  };
  let codeRange: ChatComposerSelection | null = null;
  if (format === "codeBlock" && edits.length === 2 && edits.every((edit) => edit.start === edit.end)) {
    const first = lines[touched[0]!]!;
    const last = lines[touched.at(-1)!]!;
    codeRange = {
      start: mapChatComposerOffset(first.start, edits),
      end: mapChatComposerOffset(last.end, edits),
    };
  }
  return { edits, selection: mapped, codeRange };
}

/**
 * Returns the edit a soft line break makes inside a list or quote line at a collapsed caret.
 *
 * A break after list or quote content continues the same markers on the next line, with the next
 * number for an ordered list. A break on a line holding only markers removes them, which ends the
 * list or quote. Code block lines and lines without markers return `null` for a plain break.
 */
export function chatComposerLineBreakEdit(text: string, caret: number): ChatComposerTextEdit | null {
  const lines = textLines(text);
  const states = codeLineStates(lines);
  const index = lineIndexAt(lines, caret);
  const line = lines[index]!;
  if (states[index]!.kind !== "text") return null;
  const prefix = linePrefix(line.text);
  if (prefix.length === 0 || caret < line.start + prefix.length) return null;
  if (line.text.slice(prefix.length).trim().length === 0) {
    return { start: line.start, end: line.end, text: "" };
  }
  const list = prefix.list
    ? prefix.list.kind === "bulletList"
      ? `${prefix.list.marker} `
      : `${prefix.list.number + 1}${prefix.list.delimiter} `
    : "";
  const quote = prefix.quote.length > 0 ? "> " : "";
  return { start: caret, end: caret, text: `\n${quote}${list}` };
}

/** Maps an offset of the original text through ascending, non-overlapping edits. */
export function mapChatComposerOffset(offset: number, edits: readonly ChatComposerTextEdit[]): number {
  let delta = 0;
  for (const edit of edits) {
    if (offset < edit.start) break;
    if (edit.start === edit.end && offset === edit.start) {
      if (!edit.keepBefore) delta += edit.text.length;
      break;
    }
    if (offset < edit.end) return edit.start + delta + (offset === edit.start ? 0 : edit.text.length);
    delta += edit.text.length - (edit.end - edit.start);
  }
  return offset + delta;
}

function listEdits(
  lines: readonly TextLine[],
  states: readonly CodeLineState[],
  touched: readonly number[],
  format: "bulletList" | "orderedList",
): ChatComposerTextEdit[] {
  const candidates = listCandidates(lines, states, touched);
  if (candidates.length === 0) return [];
  const active = candidates.every((index) => linePrefix(lines[index]!.text).list?.kind === format);
  return candidates.flatMap((index, position) => {
    const line = lines[index]!;
    const prefix = linePrefix(line.text);
    const start = line.start + prefix.quote.length;
    const end = line.start + prefix.length;
    const replacement = active ? "" : format === "bulletList" ? "- " : `${position + 1}. `;
    return line.text.slice(prefix.quote.length, prefix.length) === replacement
      ? []
      : [{ start, end, text: replacement }];
  });
}

function quoteEdits(
  lines: readonly TextLine[],
  states: readonly CodeLineState[],
  touched: readonly number[],
): ChatComposerTextEdit[] {
  const candidates = touched.filter((index) => states[index]!.kind === "text");
  if (candidates.length === 0) return [];
  const active = quoteIsActive(lines, states, touched);
  return candidates.flatMap((index) => {
    const line = lines[index]!;
    const quote = linePrefix(line.text).quote;
    if (active) return quote.length > 0 ? [{ start: line.start, end: line.start + quote.length, text: "" }] : [];
    return quote.length > 0 ? [] : [{ start: line.start, end: line.start, text: "> " }];
  });
}

function codeBlockEdits(
  lines: readonly TextLine[],
  states: readonly CodeLineState[],
  touched: readonly number[],
): ChatComposerTextEdit[] {
  const block = enclosingCodeBlock(states, touched);
  if (block !== null) {
    const fences = states.flatMap((state, index) => state.kind === "fence" && state.block === block ? [index] : []);
    return fences.map((index) => {
      const line = lines[index]!;
      return index < lines.length - 1
        ? { start: line.start, end: lines[index + 1]!.start, text: "" }
        : { start: index > 0 ? lines[index - 1]!.end : line.start, end: line.end, text: "" };
    });
  }
  if (touched.some((index) => states[index]!.kind !== "text")) return [];
  const fence = "```";
  const first = lines[touched[0]!]!;
  const last = lines[touched.at(-1)!]!;
  return [
    { start: first.start, end: first.start, text: `${fence}\n` },
    { start: last.end, end: last.end, text: `\n${fence}`, keepBefore: true },
  ];
}

function listIsActive(
  lines: readonly TextLine[],
  states: readonly CodeLineState[],
  touched: readonly number[],
  format: "bulletList" | "orderedList",
): boolean {
  const candidates = listCandidates(lines, states, touched);
  return candidates.length > 0 && candidates.every((index) => linePrefix(lines[index]!.text).list?.kind === format);
}

function quoteIsActive(
  lines: readonly TextLine[],
  states: readonly CodeLineState[],
  touched: readonly number[],
): boolean {
  const candidates = touched.filter((index) => states[index]!.kind === "text");
  const written = candidates.filter((index) => lines[index]!.text.length > 0);
  const checked = written.length > 0 ? written : candidates;
  return checked.length > 0 && checked.every((index) => linePrefix(lines[index]!.text).quote.length > 0);
}

/** Lines a list toggle changes: touched text lines, without blank lines when several are touched. */
function listCandidates(
  lines: readonly TextLine[],
  states: readonly CodeLineState[],
  touched: readonly number[],
): number[] {
  const textIndexes = touched.filter((index) => states[index]!.kind === "text");
  if (textIndexes.length <= 1) return textIndexes;
  return textIndexes.filter((index) => lines[index]!.text.trim().length > 0);
}

/** Returns the code block that holds every touched line, or `null`. */
function enclosingCodeBlock(states: readonly CodeLineState[], touched: readonly number[]): number | null {
  const first = states[touched[0]!];
  if (!first || first.kind === "text") return null;
  return touched.every((index) => {
    const state = states[index]!;
    return state.kind !== "text" && state.block === first.block;
  }) ? first.block : null;
}

function linePrefix(text: string): LinePrefix {
  const quote = QUOTE_PREFIX.exec(text)?.[0] ?? "";
  const match = LIST_PREFIX.exec(text.slice(quote.length));
  if (!match) return { quote, list: null, length: quote.length };
  const ordered = match[2] !== undefined;
  return {
    quote,
    list: {
      kind: ordered ? "orderedList" : "bulletList",
      marker: match[1] ?? "-",
      number: ordered ? Number(match[2]) : 0,
      delimiter: match[3] ?? ".",
    },
    length: quote.length + match[0].length,
  };
}

/** Classifies lines as text, fence, or code content of numbered fenced code blocks. */
function codeLineStates(lines: readonly TextLine[]): CodeLineState[] {
  let open: { fence: number; block: number } | null = null;
  let blocks = 0;
  return lines.map((line) => {
    if (open) {
      const closing = /^(`+)\s*$/u.exec(line.text);
      const state: CodeLineState = { kind: closing && closing[1]!.length >= open.fence ? "fence" : "code", block: open.block };
      if (state.kind === "fence") open = null;
      return state;
    }
    const opening = OPENING_FENCE.exec(line.text);
    if (!opening || line.text.slice(opening[1]!.length).includes("`")) return { kind: "text" };
    open = { fence: opening[1]!.length, block: blocks };
    blocks += 1;
    return { kind: "fence", block: open.block };
  });
}

function textLines(text: string): TextLine[] {
  const lines: TextLine[] = [];
  let start = 0;
  for (const part of text.split("\n")) {
    lines.push({ start, end: start + part.length, text: part });
    start += part.length + 1;
  }
  return lines;
}

function lineIndexAt(lines: readonly TextLine[], offset: number): number {
  const index = lines.findIndex((line) => offset <= line.end);
  return index === -1 ? lines.length - 1 : index;
}

/**
 * Returns the lines a selection touches. A selection that ends at the start of a line, such as
 * one made by selecting whole lines, leaves that line alone.
 */
function touchedLineIndexes(lines: readonly TextLine[], selection: ChatComposerSelection): number[] {
  const start = Math.min(selection.start, selection.end);
  const end = Math.max(selection.start, selection.end);
  const first = lineIndexAt(lines, start);
  let last = lineIndexAt(lines, end);
  if (end > start && last > first && lines[last]!.start === end) last -= 1;
  return Array.from({ length: last - first + 1 }, (_, offset) => first + offset);
}
