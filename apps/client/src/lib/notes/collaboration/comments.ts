import type {
  NotesComment,
  NotesCommentAnchorCreate,
  NotesCommentParent,
  NotesCommentThread,
  NotesCommentThreadStatus,
  NotesRichText,
} from "$lib/notes/types";
import { richTextPlainText, richTextRangeSlice } from "$lib/notes/rich-text/core";

const COMMENT_ANCHOR_CONTEXT_LENGTH = 48;

/**
 * Trim whitespace at the edges of a comment body while keeping mentions and formatting.
 * A body with no visible text becomes empty, which the backend accepts only alongside attachments.
 */
export function notesCommentBodyRichText(richText: readonly NotesRichText[]): NotesRichText[] {
  const plainText = richTextPlainText(richText);
  const start = plainText.length - plainText.trimStart().length;
  const end = plainText.trimEnd().length;
  if (end <= start) return [];
  return richTextRangeSlice(richText, start, end);
}

/** Whether a comment carries nothing to post: no visible text and no attachments. */
export function notesCommentIsEmpty(
  richText: readonly NotesRichText[],
  attachments: readonly Record<string, unknown>[],
): boolean {
  return richTextPlainText(richText).trim().length === 0 && attachments.length === 0;
}

/** The open discussions attached to the page itself rather than to one of its blocks. */
export function openNotesPageCommentThreads(
  threads: readonly NotesCommentThread[],
): NotesCommentThread[] {
  return threads.filter((thread) => thread.parent.type === "page_id" && thread.status === "open");
}

/** The comment most recently posted to any of the given discussions, with its thread. */
export function latestNotesComment(
  threads: readonly NotesCommentThread[],
): { thread: NotesCommentThread; comment: NotesComment } | null {
  let latest: { thread: NotesCommentThread; comment: NotesComment } | null = null;
  for (const thread of threads) {
    for (const comment of thread.comments) {
      if (!latest || Date.parse(comment.created_time) > Date.parse(latest.comment.created_time)) {
        latest = { thread, comment };
      }
    }
  }
  return latest;
}

export interface NotesResolvedCommentAnchor {
  threadId: string;
  blockId: string;
  status: NotesCommentThreadStatus;
  start: number;
  end: number;
  text: string;
}

export function notesCommentPlainText(comment: NotesComment): string {
  return richTextPlainText(comment.rich_text);
}

export function notesCommentThreadSnippet(thread: NotesCommentThread): string {
  return thread.comments
    .map(notesCommentPlainText)
    .map((text) => text.trim())
    .find((text) => text.length > 0) ?? "";
}

export function openNotesCommentThreadCount(
  threads: readonly NotesCommentThread[],
): number {
  return threads.filter((thread) => thread.status === "open").length;
}

export function unreadNotesCommentThreadCount(
  threads: readonly NotesCommentThread[],
): number {
  return threads.filter((thread) => thread.unread).length;
}

export function notesCommentParentKey(parent: NotesCommentParent): string {
  if (parent.type === "page_id") return `page:${parent.page_id}`;
  return `block:${parent.block_id}`;
}

export function notesCommentParentMatches(
  left: NotesCommentParent,
  right: NotesCommentParent,
): boolean {
  return notesCommentParentKey(left) === notesCommentParentKey(right);
}

export function notesCommentAnchorDraft(
  blockText: string,
  start: number,
  end: number,
): NotesCommentAnchorCreate | null {
  const safeStart = Math.max(0, Math.min(start, blockText.length));
  const safeEnd = Math.max(safeStart, Math.min(end, blockText.length));
  const text = blockText.slice(safeStart, safeEnd);
  if (!text.trim()) return null;
  return {
    start: safeStart,
    end: safeEnd,
    text,
    prefix: blockText.slice(Math.max(0, safeStart - COMMENT_ANCHOR_CONTEXT_LENGTH), safeStart),
    suffix: blockText.slice(safeEnd, safeEnd + COMMENT_ANCHOR_CONTEXT_LENGTH),
  };
}

export function notesResolveCommentAnchor(
  thread: NotesCommentThread,
  blockText: string,
): NotesResolvedCommentAnchor | null {
  const anchor = thread.anchor;
  if (!anchor || !thread.block_id || anchor.block_id !== thread.block_id || !anchor.text) {
    return null;
  }
  if (blockText.slice(anchor.start, anchor.end) === anchor.text) {
    return {
      threadId: thread.id,
      blockId: anchor.block_id,
      status: thread.status,
      start: anchor.start,
      end: anchor.end,
      text: anchor.text,
    };
  }
  const bestStart = bestCommentAnchorStart(blockText, anchor.text, anchor.start, anchor.prefix, anchor.suffix);
  if (bestStart === null) return null;
  return {
    threadId: thread.id,
    blockId: anchor.block_id,
    status: thread.status,
    start: bestStart,
    end: bestStart + anchor.text.length,
    text: anchor.text,
  };
}

export function notesCommentAnchorsForBlock(
  threads: readonly NotesCommentThread[],
  blockId: string,
  blockText: string,
): NotesResolvedCommentAnchor[] {
  return threads
    .filter((thread) => thread.block_id === blockId && thread.anchor !== null)
    .map((thread) => notesResolveCommentAnchor(thread, blockText))
    .filter((anchor): anchor is NotesResolvedCommentAnchor => anchor !== null)
    .sort((left, right) => left.start - right.start || right.end - left.end);
}

function bestCommentAnchorStart(
  blockText: string,
  anchorText: string,
  savedStart: number,
  prefix: string,
  suffix: string,
): number | null {
  let best: { start: number; score: number } | null = null;
  let index = blockText.indexOf(anchorText);
  while (index !== -1) {
    const score = commentAnchorCandidateScore(blockText, index, anchorText, savedStart, prefix, suffix);
    if (!best || score > best.score) best = { start: index, score };
    index = blockText.indexOf(anchorText, index + Math.max(1, anchorText.length));
  }
  return best?.start ?? null;
}

function commentAnchorCandidateScore(
  blockText: string,
  start: number,
  anchorText: string,
  savedStart: number,
  prefix: string,
  suffix: string,
): number {
  const before = blockText.slice(Math.max(0, start - prefix.length), start);
  const after = blockText.slice(start + anchorText.length, start + anchorText.length + suffix.length);
  let score = 0;
  if (prefix && before.endsWith(prefix)) score += 1000;
  if (suffix && after.startsWith(suffix)) score += 1000;
  const distance = Math.abs(start - savedStart);
  return score - Math.min(distance, 1000);
}
