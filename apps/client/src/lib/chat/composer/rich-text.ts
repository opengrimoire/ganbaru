import type { JsonValue, VersionedJson } from "$lib/chat/contracts";

export const CHAT_COMPOSER_RICH_TEXT_SCHEMA_VERSION = 1;

export const CHAT_COMPOSER_MARKS = ["bold", "italic"] as const;

export type ChatComposerMark = (typeof CHAT_COMPOSER_MARKS)[number];

export interface ChatComposerTextRun {
  text: string;
  marks: ChatComposerMark[];
}

export interface ChatComposerLine {
  runs: ChatComposerTextRun[];
}

export interface ChatComposerDocument {
  lines: ChatComposerLine[];
}

export interface ChatComposerSelection {
  start: number;
  end: number;
}

interface LinearSegment {
  text: string;
  marks: ChatComposerMark[];
}

/** Creates a normalized rich-text document from unformatted text. */
export function chatComposerDocumentFromText(text: string): ChatComposerDocument {
  return normalizeChatComposerDocument({
    lines: text.replace(/\r\n?/gu, "\n").split("\n").map((line) => ({
      runs: line.length > 0 ? [{ text: line, marks: [] }] : [],
    })),
  });
}

/** Returns the visible plain text represented by a composer document. */
export function chatComposerPlainText(document: ChatComposerDocument): string {
  return normalizeChatComposerDocument(document).lines
    .map((line) => line.runs.map((run) => run.text).join(""))
    .join("\n");
}

/** Serializes supported visual marks to provider-facing Markdown. */
export function chatComposerMarkdown(document: ChatComposerDocument): string {
  return normalizeChatComposerDocument(document).lines.map((line) => line.runs.map((run) => {
    const layout = markdownRunLayout(run);
    return layout.leading
      + layout.prefix
      + markdownCoreText(run, layout.core)
      + layout.suffix
      + layout.trailing;
  }).join("")).join("\n");
}

/** Maps a visible JavaScript offset to the matching provider-facing Markdown offset. */
export function chatComposerMarkdownOffset(
  document: ChatComposerDocument,
  visibleOffset: number,
  affinity: "forward" | "backward" = "forward",
): number {
  const normalized = normalizeChatComposerDocument(document);
  const plainText = chatComposerPlainText(normalized);
  const target = clampOffset(visibleOffset, plainText.length);
  let visibleCursor = 0;
  let markdownCursor = 0;
  for (let lineIndex = 0; lineIndex < normalized.lines.length; lineIndex += 1) {
    const line = normalized.lines[lineIndex]!;
    for (const run of line.runs) {
      const layout = markdownRunLayout(run);
      const coreStart = visibleCursor + layout.leading.length;
      const coreEnd = coreStart + layout.core.length;
      const runEnd = visibleCursor + run.text.length;
      const opened = markdownCursor + layout.leading.length + layout.prefix.length;
      const coreClosed = opened + markdownCoreText(run, layout.core).length;
      const closed = coreClosed + layout.suffix.length;
      if (target < runEnd) {
        if (target < coreStart) return markdownCursor + (target - visibleCursor);
        if (target < coreEnd) {
          return opened + markdownCoreText(run, layout.core.slice(0, target - coreStart)).length;
        }
        return closed + (target - coreEnd);
      }
      if (target === runEnd && affinity === "backward") {
        return layout.trailing.length > 0 || layout.core.length === 0
          ? closed + layout.trailing.length
          : coreClosed;
      }
      visibleCursor = runEnd;
      markdownCursor = closed + layout.trailing.length;
    }
    if (lineIndex < normalized.lines.length - 1) {
      if (target === visibleCursor) return markdownCursor;
      visibleCursor += 1;
      markdownCursor += 1;
    }
  }
  return markdownCursor;
}

/** Maps an exact Markdown boundary back to its visible JavaScript offset. */
export function chatComposerVisibleOffset(
  document: ChatComposerDocument,
  markdownOffset: number,
): number {
  const normalized = normalizeChatComposerDocument(document);
  const plainText = chatComposerPlainText(normalized);
  const markdownLength = chatComposerMarkdown(normalized).length;
  const target = clampOffset(markdownOffset, markdownLength);
  let visibleOffset = 0;
  while (visibleOffset <= plainText.length) {
    const mapped = chatComposerMarkdownOffset(normalized, visibleOffset);
    if (mapped >= target) return visibleOffset;
    const character = [...plainText.slice(visibleOffset)][0];
    if (!character) break;
    visibleOffset += character.length;
  }
  return plainText.length;
}

/** Converts a composer document to its bounded, versioned persistence value. */
export function chatComposerDocumentVersioned(document: ChatComposerDocument): VersionedJson {
  const normalized = normalizeChatComposerDocument(document);
  const value: JsonValue = {
    lines: normalized.lines.map((line) => ({
      runs: line.runs.map((run) => ({ text: run.text, marks: [...run.marks] })),
    })),
  };
  return {
    schemaVersion: CHAT_COMPOSER_RICH_TEXT_SCHEMA_VERSION,
    value,
  };
}

/** Parses a persisted composer document, falling back safely when it is invalid or unsupported. */
export function parseChatComposerDocument(
  value: VersionedJson | null,
  fallbackText: string,
): ChatComposerDocument {
  if (value?.schemaVersion !== CHAT_COMPOSER_RICH_TEXT_SCHEMA_VERSION) {
    return chatComposerDocumentFromText(fallbackText);
  }
  const candidate = value.value;
  if (!isRecord(candidate) || !Array.isArray(candidate.lines) || candidate.lines.length === 0) {
    return chatComposerDocumentFromText(fallbackText);
  }
  const lines: ChatComposerLine[] = [];
  for (const candidateLine of candidate.lines) {
    if (!isRecord(candidateLine) || !Array.isArray(candidateLine.runs)) {
      return chatComposerDocumentFromText(fallbackText);
    }
    const runs: ChatComposerTextRun[] = [];
    for (const candidateRun of candidateLine.runs) {
      if (!isRecord(candidateRun) || typeof candidateRun.text !== "string" || !Array.isArray(candidateRun.marks)) {
        return chatComposerDocumentFromText(fallbackText);
      }
      if (/[\r\n]/u.test(candidateRun.text)) return chatComposerDocumentFromText(fallbackText);
      const marks = candidateRun.marks;
      if (!marks.every(isChatComposerMark)) return chatComposerDocumentFromText(fallbackText);
      runs.push({ text: candidateRun.text, marks: [...new Set(marks)].sort(compareMarks) });
    }
    lines.push({ runs });
  }
  const document = normalizeChatComposerDocument({ lines });
  return chatComposerMarkdown(document) === fallbackText
    ? document
    : chatComposerDocumentFromText(fallbackText);
}

/** Replaces a visible text range while preserving marks outside that range. */
export function replaceChatComposerText(
  document: ChatComposerDocument,
  selection: ChatComposerSelection,
  insertedText: string,
  marks: readonly ChatComposerMark[] = [],
): { document: ChatComposerDocument; selection: ChatComposerSelection } {
  const segments = documentSegments(document);
  const length = segments.reduce((total, segment) => total + segment.text.length, 0);
  const start = clampOffset(Math.min(selection.start, selection.end), length);
  const end = clampOffset(Math.max(selection.start, selection.end), length);
  const normalizedText = insertedText.replace(/\r\n?/gu, "\n");
  const next = [
    ...sliceSegments(segments, 0, start),
    ...(normalizedText.length > 0 ? [{ text: normalizedText, marks: normalizeMarks(marks) }] : []),
    ...sliceSegments(segments, end, length),
  ];
  const cursor = start + normalizedText.length;
  return {
    document: documentFromSegments(next),
    selection: { start: cursor, end: cursor },
  };
}

/** Toggles a mark over a non-empty visible text range. */
export function toggleChatComposerMark(
  document: ChatComposerDocument,
  selection: ChatComposerSelection,
  mark: ChatComposerMark,
): ChatComposerDocument {
  const segments = documentSegments(document);
  const length = segments.reduce((total, segment) => total + segment.text.length, 0);
  const start = clampOffset(Math.min(selection.start, selection.end), length);
  const end = clampOffset(Math.max(selection.start, selection.end), length);
  if (start === end) return normalizeChatComposerDocument(document);
  const selected = sliceSegments(segments, start, end);
  const markedText = selected.filter((segment) => segment.text !== "\n");
  const shouldRemove = markedText.length > 0 && markedText.every((segment) => segment.marks.includes(mark));
  const changed = selected.map((segment) => segment.text === "\n" ? segment : {
    text: segment.text,
    marks: shouldRemove
      ? segment.marks.filter((candidate) => candidate !== mark)
      : normalizeMarks([...segment.marks, mark]),
  });
  return documentFromSegments([
    ...sliceSegments(segments, 0, start),
    ...changed,
    ...sliceSegments(segments, end, length),
  ]);
}

/** Returns the marks that should continue when text is inserted at an offset. */
export function chatComposerMarksAt(document: ChatComposerDocument, offset: number): ChatComposerMark[] {
  const segments = documentSegments(document);
  const length = segments.reduce((total, segment) => total + segment.text.length, 0);
  const target = clampOffset(offset, length);
  let cursor = 0;
  let previous: ChatComposerMark[] = [];
  for (const segment of segments) {
    const next = cursor + segment.text.length;
    if (segment.text !== "\n") {
      if (target > cursor && target <= next) return [...segment.marks];
      if (target === cursor) return previous.length > 0 ? [...previous] : [...segment.marks];
      previous = segment.marks;
    } else if (target >= cursor && target <= next) {
      return [];
    }
    cursor = next;
  }
  return [...previous];
}

/** Returns marks that cover the complete selection, or the insertion marks at a caret. */
export function chatComposerActiveMarks(
  document: ChatComposerDocument,
  selection: ChatComposerSelection,
): ChatComposerMark[] {
  if (selection.start === selection.end) return chatComposerMarksAt(document, selection.start);
  const segments = documentSegments(document);
  const length = segments.reduce((total, segment) => total + segment.text.length, 0);
  const start = clampOffset(Math.min(selection.start, selection.end), length);
  const end = clampOffset(Math.max(selection.start, selection.end), length);
  const selected = sliceSegments(segments, start, end).filter((segment) => segment.text !== "\n");
  if (selected.length === 0) return [];
  return CHAT_COMPOSER_MARKS.filter((mark) => selected.every((segment) => segment.marks.includes(mark)));
}

/** Normalizes empty lines, mark order, and adjacent equivalent runs. */
export function normalizeChatComposerDocument(document: ChatComposerDocument): ChatComposerDocument {
  const sourceLines = document.lines.length > 0 ? document.lines : [{ runs: [] }];
  return {
    lines: sourceLines.map((line) => {
      const runs: ChatComposerTextRun[] = [];
      for (const sourceRun of line.runs) {
        if (sourceRun.text.length === 0) continue;
        const marks = normalizeMarks(sourceRun.marks);
        const previous = runs.at(-1);
        if (previous && sameMarks(previous.marks, marks)) previous.text += sourceRun.text;
        else runs.push({ text: sourceRun.text, marks });
      }
      return { runs };
    }),
  };
}

function documentSegments(document: ChatComposerDocument): LinearSegment[] {
  const lines = normalizeChatComposerDocument(document).lines;
  const segments: LinearSegment[] = [];
  lines.forEach((line, index) => {
    segments.push(...line.runs.map((run) => ({ text: run.text, marks: [...run.marks] })));
    if (index < lines.length - 1) segments.push({ text: "\n", marks: [] });
  });
  return segments;
}

function documentFromSegments(segments: LinearSegment[]): ChatComposerDocument {
  const lines: ChatComposerLine[] = [{ runs: [] }];
  for (const segment of segments) {
    const parts = segment.text.split("\n");
    parts.forEach((part, index) => {
      if (part.length > 0) lines.at(-1)?.runs.push({ text: part, marks: [...segment.marks] });
      if (index < parts.length - 1) lines.push({ runs: [] });
    });
  }
  return normalizeChatComposerDocument({ lines });
}

function sliceSegments(segments: LinearSegment[], from: number, to: number): LinearSegment[] {
  if (from >= to) return [];
  const result: LinearSegment[] = [];
  let cursor = 0;
  for (const segment of segments) {
    const next = cursor + segment.text.length;
    const start = Math.max(from, cursor);
    const end = Math.min(to, next);
    if (start < end) {
      result.push({
        text: segment.text.slice(start - cursor, end - cursor),
        marks: [...segment.marks],
      });
    }
    cursor = next;
    if (cursor >= to) break;
  }
  return result;
}

function escapeMarkedText(text: string): string {
  return text.replace(/([\\*_])/gu, "\\$1");
}

interface MarkdownRunLayout {
  leading: string;
  prefix: string;
  core: string;
  suffix: string;
  trailing: string;
}

/**
 * Splits a run into visible text and emphasis delimiters.
 *
 * CommonMark only treats a delimiter as emphasis when it touches non-whitespace text, so
 * edge whitespace of a marked run stays outside the delimiters and a whitespace-only run
 * is emitted without them. Emphasis always uses `*` because CommonMark rejects `_`
 * inside words; combined marks use the `***` form CommonMark reads as emphasis around
 * strong emphasis.
 */
function markdownRunLayout(run: ChatComposerTextRun): MarkdownRunLayout {
  const bold = run.marks.includes("bold");
  const italic = run.marks.includes("italic");
  const delimiters: readonly [string, string] | null = bold && italic
    ? ["***", "***"]
    : bold ? ["**", "**"] : italic ? ["*", "*"] : null;
  if (!delimiters) return { leading: "", prefix: "", core: run.text, suffix: "", trailing: "" };
  const core = run.text.trim();
  if (core.length === 0) return { leading: run.text, prefix: "", core: "", suffix: "", trailing: "" };
  const leadingLength = run.text.length - run.text.trimStart().length;
  return {
    leading: run.text.slice(0, leadingLength),
    prefix: delimiters[0],
    core,
    suffix: delimiters[1],
    trailing: run.text.slice(leadingLength + core.length),
  };
}

function markdownCoreText(run: ChatComposerTextRun, text: string): string {
  return run.marks.length > 0 ? escapeMarkedText(text) : text;
}

function normalizeMarks(marks: readonly ChatComposerMark[]): ChatComposerMark[] {
  return [...new Set(marks)].sort(compareMarks);
}

function compareMarks(left: ChatComposerMark, right: ChatComposerMark): number {
  return CHAT_COMPOSER_MARKS.indexOf(left) - CHAT_COMPOSER_MARKS.indexOf(right);
}

function sameMarks(left: readonly ChatComposerMark[], right: readonly ChatComposerMark[]): boolean {
  return left.length === right.length && left.every((mark, index) => mark === right[index]);
}

function clampOffset(offset: number, length: number): number {
  return Math.min(length, Math.max(0, offset));
}

function isChatComposerMark(value: unknown): value is ChatComposerMark {
  return typeof value === "string" && CHAT_COMPOSER_MARKS.some((mark) => mark === value);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
