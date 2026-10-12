import {
  blockEditableRichText,
  blockPlainText,
} from "$lib/notes/blocks/factory";
import {
  replacePlainTextPreservingRichText,
} from "$lib/notes/rich-text/core";
import type {
  NotesBlock,
  NotesRichText,
  NotesSuggestion,
  NotesSuggestionCreate,
  NotesSuggestionStatus,
} from "$lib/notes/types";

const SUGGESTION_ANCHOR_CONTEXT_LENGTH = 48;

export interface NotesResolvedSuggestionAnchor {
  suggestionId: string;
  blockId: string;
  status: NotesSuggestionStatus;
  start: number;
  end: number;
  originalText: string;
  proposedText: string;
}

export interface NotesSuggestionDraft {
  block_id: string;
  range_start: number;
  range_end: number;
  original_text: string;
  proposed_text: string;
  prefix: string;
  suffix: string;
}

export interface NotesSuggestionApplyPlan {
  richText: NotesRichText[];
  selectionStart: number;
  selectionEnd: number;
}

export function notesSuggestionDraft(
  blockId: string,
  blockText: string,
  start: number,
  end: number,
): NotesSuggestionDraft | null {
  const safeStart = Math.max(0, Math.min(start, blockText.length));
  const safeEnd = Math.max(safeStart, Math.min(end, blockText.length));
  const originalText = blockText.slice(safeStart, safeEnd);
  if (!originalText.trim()) return null;
  return {
    block_id: blockId,
    range_start: safeStart,
    range_end: safeEnd,
    original_text: originalText,
    proposed_text: originalText,
    prefix: blockText.slice(Math.max(0, safeStart - SUGGESTION_ANCHOR_CONTEXT_LENGTH), safeStart),
    suffix: blockText.slice(safeEnd, safeEnd + SUGGESTION_ANCHOR_CONTEXT_LENGTH),
  };
}

export function notesSuggestionCreateRequest(
  id: string,
  draft: NotesSuggestionDraft,
  proposedText: string,
): NotesSuggestionCreate {
  return {
    id,
    block_id: draft.block_id,
    range_start: draft.range_start,
    range_end: draft.range_end,
    original_text: draft.original_text,
    proposed_text: proposedText,
    prefix: draft.prefix,
    suffix: draft.suffix,
  };
}

export function notesResolveSuggestionAnchor(
  suggestion: NotesSuggestion,
  blockText: string,
): NotesResolvedSuggestionAnchor | null {
  if (!suggestion.original_text) return null;
  if (blockText.slice(suggestion.range_start, suggestion.range_end) === suggestion.original_text) {
    return {
      suggestionId: suggestion.id,
      blockId: suggestion.block_id,
      status: suggestion.status,
      start: suggestion.range_start,
      end: suggestion.range_end,
      originalText: suggestion.original_text,
      proposedText: suggestion.proposed_text,
    };
  }
  const bestStart = bestSuggestionAnchorStart(
    blockText,
    suggestion.original_text,
    suggestion.range_start,
    suggestion.prefix,
    suggestion.suffix,
  );
  if (bestStart === null) return null;
  return {
    suggestionId: suggestion.id,
    blockId: suggestion.block_id,
    status: suggestion.status,
    start: bestStart,
    end: bestStart + suggestion.original_text.length,
    originalText: suggestion.original_text,
    proposedText: suggestion.proposed_text,
  };
}

export function notesSuggestionAnchorsForBlock(
  suggestions: readonly NotesSuggestion[],
  blockId: string,
  blockText: string,
): NotesResolvedSuggestionAnchor[] {
  return suggestions
    .filter((suggestion) => suggestion.block_id === blockId)
    .map((suggestion) => notesResolveSuggestionAnchor(suggestion, blockText))
    .filter((anchor): anchor is NotesResolvedSuggestionAnchor => anchor !== null)
    .sort((left, right) => left.start - right.start || right.end - left.end);
}

export function notesApplySuggestionToBlock(
  block: NotesBlock,
  suggestion: NotesSuggestion,
): NotesSuggestionApplyPlan | null {
  const text = blockPlainText(block);
  const anchor = notesResolveSuggestionAnchor(suggestion, text);
  if (!anchor) return null;
  const nextText = `${text.slice(0, anchor.start)}${suggestion.proposed_text}${text.slice(anchor.end)}`;
  const selectionStart = anchor.start;
  const selectionEnd = anchor.start + suggestion.proposed_text.length;
  return {
    richText: replacePlainTextPreservingRichText(blockEditableRichText(block), nextText),
    selectionStart,
    selectionEnd,
  };
}

export function openNotesSuggestionCount(suggestions: readonly NotesSuggestion[]): number {
  return suggestions.filter((suggestion) => suggestion.status === "open").length;
}

function bestSuggestionAnchorStart(
  blockText: string,
  anchorText: string,
  savedStart: number,
  prefix: string,
  suffix: string,
): number | null {
  let best: { start: number; score: number } | null = null;
  let index = blockText.indexOf(anchorText);
  while (index !== -1) {
    const score = suggestionAnchorCandidateScore(blockText, index, anchorText, savedStart, prefix, suffix);
    if (!best || score > best.score) best = { start: index, score };
    index = blockText.indexOf(anchorText, index + Math.max(1, anchorText.length));
  }
  return best?.start ?? null;
}

function suggestionAnchorCandidateScore(
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
