import {
  createBlockUpdate,
  createBlockWrite,
  createCodePayload,
  createTodoPayload,
} from "$lib/notes/blocks/factory";
import type { NotesBlockType, NotesBlockUpdate, NotesBlockWrite, NotesParent, NotesAppendBlockChildrenRequest } from "$lib/notes/types";

export const NOTES_CLIPBOARD_MAX_TEXT_LENGTH = 64 * 1024;
export const NOTES_CLIPBOARD_MAX_BLOCKS = 101;
export const NOTES_MARKDOWN_LIST_PARAGRAPHS_ATTRIBUTE = "data-notes-markdown-list-paragraphs";
/** Carries the identity of user, date, and app object mentions in copied HTML. */
export const NOTES_CLIPBOARD_MENTION_ATTRIBUTE = "data-notes-mention";
/** Upper bound for one serialized mention identity read back from the clipboard. */
export const NOTES_CLIPBOARD_MENTION_MAX_LENGTH = 2 * 1024;
const MARKDOWN_TAB_WIDTH = 4;

type NotesPastedTextBlockType =
  | "paragraph"
  | "heading_1"
  | "heading_2"
  | "heading_3"
  | "heading_4"
  | "heading_5"
  | "heading_6"
  | "bulleted_list_item"
  | "numbered_list_item"
  | "to_do"
  | "toggle"
  | "quote"
  | "divider"
  | "code";

interface NotesPastedBlockSegment {
  type: NotesPastedTextBlockType;
  content: string;
  depth?: number;
  checked?: boolean;
  language?: string;
}

export interface NotesPlainTextPastePlan {
  currentUpdate: NotesBlockUpdate;
  appendedBlocks: NotesBlockWrite[];
  blockDepths: number[];
  focusBlockId: string;
  focusOffset: number;
}

export interface NotesPlainTextPastePlanInput {
  currentBlockId: string;
  currentBlockType: NotesBlockType;
  currentText: string;
  selectionStart: number;
  selectionEnd: number;
  plainText: string;
  createId: () => string;
}

function containsUnsupportedControlCharacter(text: string): boolean {
  return [...text].some((character) => {
    if (character === "\n" || character === "\t") return false;
    return character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127;
  });
}

export function normalizeNotesClipboardPlainText(plainText: string): string | null {
  if (!plainText || plainText.length > NOTES_CLIPBOARD_MAX_TEXT_LENGTH) return null;
  const normalized = plainText.replace(/\r\n?/gu, "\n");
  if (!normalized || containsUnsupportedControlCharacter(normalized)) return null;
  return normalized;
}

function markdownPrefixSegment(line: string): NotesPastedBlockSegment | null {
  const trimmedRight = line.trimEnd();
  if (/^---+$/u.test(trimmedRight.trim())) {
    return { type: "divider", content: "" };
  }

  const heading = /^(#{1,6})\s+(.*)$/u.exec(trimmedRight);
  if (heading) {
    const level = heading[1].length;
    const content = heading[2] ?? "";
    if (level === 1) return { type: "heading_1", content };
    if (level === 2) return { type: "heading_2", content };
    if (level === 3) return { type: "heading_3", content };
    if (level === 4) return { type: "heading_4", content };
    if (level === 5) return { type: "heading_5", content };
    return { type: "heading_6", content };
  }

  const task = /^[-*+]\s+\[([ xX])\]\s+(.*)$/u.exec(trimmedRight);
  if (task) return { type: "to_do", content: task[2], checked: task[1] !== " " };

  const bullet = /^[-*+]\s+(.*)$/u.exec(trimmedRight);
  if (bullet) return { type: "bulleted_list_item", content: bullet[1] ?? "" };

  const numbered = /^\d+[.)]\s+(.*)$/u.exec(trimmedRight);
  if (numbered) return { type: "numbered_list_item", content: numbered[1] ?? "" };

  const blockquote = /^>\s+(.*)$/u.exec(trimmedRight);
  if (blockquote) return { type: "quote", content: blockquote[1] ?? "" };

  return null;
}

function languageFromFence(line: string): string {
  const language = line.trim().slice(3).trim();
  return language || "plain text";
}

function parsePastedTextSegments(
  text: string,
  allowMarkdownForFirstLine: boolean,
): NotesPastedBlockSegment[] {
  const lines = text.split("\n");
  const segments: NotesPastedBlockSegment[] = [];
  const listIndents: number[] = [];
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index] ?? "";
    if (!line.trim() && listIndents.length) {
      const nextLine = lines.slice(index + 1).find((candidate) => candidate.trim());
      const nextSegment = nextLine ? markdownPrefixSegment(nextLine.trimStart()) : null;
      if (nextSegment && ["bulleted_list_item", "numbered_list_item", "to_do"].includes(nextSegment.type)) continue;
    }
    if (line.trim().startsWith("```")) {
      const language = languageFromFence(line);
      const codeLines: string[] = [];
      index += 1;
      while (index < lines.length && !(lines[index] ?? "").trim().startsWith("```")) {
        codeLines.push(lines[index] ?? "");
        index += 1;
      }
      segments.push({ type: "code", content: codeLines.join("\n"), language });
      continue;
    }

    const indentation = line.match(/^[ \t]*/u)?.[0] ?? "";
    const indent = [...indentation].reduce((column, character) =>
      character === "\t" ? column + MARKDOWN_TAB_WIDTH - column % MARKDOWN_TAB_WIDTH : column + 1, 0);
    const sourceSegment = markdownPrefixSegment(line.slice(indentation.length));
    const markdownSegment: NotesPastedBlockSegment | null = index === 0 && !allowMarkdownForFirstLine
      ? { type: "paragraph", content: line }
      : sourceSegment;
    if (markdownSegment && sourceSegment && ["bulleted_list_item", "numbered_list_item", "to_do"].includes(sourceSegment.type)) {
      while (listIndents.length && listIndents[listIndents.length - 1] > indent) listIndents.pop();
      if (!listIndents.length || listIndents[listIndents.length - 1] < indent) listIndents.push(indent);
      markdownSegment.depth = listIndents.length - 1;
    } else if (!markdownSegment && line.trim() && listIndents.length && indent > listIndents[listIndents.length - 1]) {
      const previous = segments.at(-1);
      if (previous) {
        previous.content += `\n${line.trimStart()}`;
        continue;
      }
    } else if (line.trim()) {
      listIndents.length = 0;
    }
    segments.push(markdownSegment ?? { type: "paragraph", content: line });
  }
  return segments;
}

function segmentAsPlainText(segment: NotesPastedBlockSegment): string {
  if (segment.type === "divider") return "---";
  return segment.content;
}

function capSegments(segments: readonly NotesPastedBlockSegment[]): NotesPastedBlockSegment[] {
  if (segments.length <= NOTES_CLIPBOARD_MAX_BLOCKS) return [...segments];
  const kept = segments.slice(0, NOTES_CLIPBOARD_MAX_BLOCKS - 1);
  const overflow = segments
    .slice(NOTES_CLIPBOARD_MAX_BLOCKS - 1)
    .map(segmentAsPlainText)
    .join("\n");
  return [...kept, { type: "paragraph", content: overflow }];
}

function segmentCanCarrySuffix(segment: NotesPastedBlockSegment): boolean {
  return segment.type !== "divider";
}

function appendSuffixToSegments(
  segments: readonly NotesPastedBlockSegment[],
  suffix: string,
): NotesPastedBlockSegment[] {
  if (!suffix) return [...segments];
  const next = [...segments];
  const last = next.at(-1);
  if (!last) return [{ type: "paragraph", content: suffix }];
  if (!segmentCanCarrySuffix(last)) {
    next.push({ type: "paragraph", content: suffix });
    return next;
  }
  next[next.length - 1] = {
    ...last,
    content: `${last.content}${suffix}`,
  };
  return next;
}

function canConvertCurrentBlockFromSegment(
  currentBlockType: NotesBlockType,
  prefix: string,
  _segment: NotesPastedBlockSegment,
): boolean {
  if (currentBlockType !== "paragraph") return false;
  if (prefix.trim().length > 0) return false;
  return true;
}

function createUpdateForSegment(
  currentBlockType: NotesBlockType,
  prefix: string,
  segment: NotesPastedBlockSegment,
): NotesBlockUpdate {
  if (canConvertCurrentBlockFromSegment(currentBlockType, prefix, segment)) {
    if (segment.type === "to_do") {
      return {
        type: "to_do",
        to_do: createTodoPayload(segment.content, segment.checked ?? false),
      };
    }
    if (segment.type === "code") {
      return {
        type: "code",
        code: createCodePayload(segment.content, segment.language),
      };
    }
    return createBlockUpdate(segment.type, segment.content);
  }
  return createBlockUpdate(currentBlockType, `${prefix}${segmentAsPlainText(segment)}`);
}

function createWriteForSegment(
  id: string,
  segment: NotesPastedBlockSegment,
): NotesBlockWrite {
  if (segment.type === "to_do") {
    return {
      id,
      type: "to_do",
      to_do: createTodoPayload(segment.content, segment.checked ?? false),
    };
  }
  if (segment.type === "code") {
    return {
      id,
      type: "code",
      code: createCodePayload(segment.content, segment.language),
    };
  }
  return createBlockWrite(id, segment.type, segment.content);
}

export function shouldHandleNotesPlainTextPaste(input: {
  currentBlockType: NotesBlockType;
  currentText: string;
  selectionStart: number;
  plainText: string;
}): boolean {
  if (input.currentBlockType === "code") return false;
  const normalizedText = normalizeNotesClipboardPlainText(input.plainText);
  if (!normalizedText) return false;
  return shouldPlanPaste(
    normalizedText,
    input.currentText,
    input.selectionStart,
    input.currentBlockType,
  );
}

function shouldPlanPaste(
  normalizedText: string,
  currentText: string,
  selectionStart: number,
  currentBlockType: NotesBlockType,
): boolean {
  if (normalizedText.includes("\n")) return true;
  if (currentBlockType !== "paragraph") return false;
  if (currentText.slice(0, selectionStart).trim().length > 0) return false;
  return markdownPrefixSegment(normalizedText) !== null;
}

export function planNotesPlainTextPaste(
  input: NotesPlainTextPastePlanInput,
): NotesPlainTextPastePlan | null {
  if (input.currentBlockType === "code") return null;
  const normalizedText = normalizeNotesClipboardPlainText(input.plainText);
  if (!normalizedText) return null;
  const start = Math.max(0, Math.min(input.selectionStart, input.selectionEnd));
  const end = Math.min(
    input.currentText.length,
    Math.max(input.selectionStart, input.selectionEnd),
  );
  if (!shouldPlanPaste(normalizedText, input.currentText, start, input.currentBlockType)) {
    return null;
  }

  const prefix = input.currentText.slice(0, start);
  const suffix = input.currentText.slice(end);
  const allowMarkdownForFirstLine = prefix.trim().length === 0;
  const segments = capSegments(
    appendSuffixToSegments(
      parsePastedTextSegments(normalizedText, allowMarkdownForFirstLine),
      suffix,
    ),
  );
  const [firstSegment, ...remainingSegments] = segments;
  if (!firstSegment) return null;
  const appendedBlocks = remainingSegments.map((segment) => {
    const id = input.createId();
    return createWriteForSegment(id, segment);
  });
  const focusSegment = segments.at(-1) ?? firstSegment;
  const focusSegmentPasteEnd = Math.max(
    0,
    segmentAsPlainText(focusSegment).length - suffix.length,
  );
  const focusOffset = appendedBlocks.length > 0
    ? focusSegmentPasteEnd
    : canConvertCurrentBlockFromSegment(input.currentBlockType, prefix, firstSegment)
      ? focusSegmentPasteEnd
      : prefix.length + focusSegmentPasteEnd;
  return {
    currentUpdate: createUpdateForSegment(input.currentBlockType, prefix, firstSegment),
    appendedBlocks,
    blockDepths: segments.map((segment) => segment.depth ?? 0),
    focusBlockId: appendedBlocks.at(-1)?.id ?? input.currentBlockId,
    focusOffset,
  };
}

/** Group pasted blocks by their structural parent, preserving preorder and sibling order. */
export function notesPasteAppendRequests(
  currentBlockId: string,
  parent: NotesParent,
  writes: readonly NotesBlockWrite[],
  depths: readonly number[] = [],
): NotesAppendBlockChildrenRequest[] {
  const ancestors = [currentBlockId];
  const groups = new Map<string, NotesAppendBlockChildrenRequest>();
  for (const [index, write] of writes.entries()) {
    const depth = Math.min(Math.max(0, depths[index + 1] ?? 0), ancestors.length);
    const parentId = depth > 0 ? ancestors[depth - 1] : null;
    const key = parentId ?? "";
    let group = groups.get(key);
    if (!group) {
      group = {
        parent: parentId ? { type: "block_id", block_id: parentId } : parent,
        after: parentId ? null : currentBlockId,
        children: [],
      };
      groups.set(key, group);
    }
    group.children.push(write);
    ancestors[depth] = write.id;
    ancestors.length = depth + 1;
  }
  return [...groups.values()];
}
