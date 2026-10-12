import type {
  ChatMessageReference,
} from "$lib/chat/contracts";
import {
  chatComposerMarkdown,
  chatComposerMarkdownOffset,
  chatComposerPlainText,
  chatComposerVisibleOffset,
  type ChatComposerDocument,
} from "./rich-text";

export interface ChatReferenceClipboardSlice {
  vaultId: string;
  text: string;
  references: ChatMessageReference[];
}

export interface ChatReferenceInsertionResult {
  text: string;
  references: ChatMessageReference[];
  selection: number;
  inserted: boolean;
}

export interface ChatReferenceTrigger {
  kind: "mention" | "channel";
  query: string;
  start: number;
  end: number;
}

export type ChatReferenceTextSegment =
  | { kind: "text"; text: string }
  | { kind: "reference"; text: string; reference: ChatMessageReference };

/** Returns the stable semantic identity used to prevent duplicate references in one message. */
export function chatReferenceSemanticKey(reference: ChatMessageReference): string {
  switch (reference.kind) {
    case "participant":
      return `participant:${reference.participantId}`;
    case "channel":
      return `channel:${reference.channelId}`;
    case "workingFolder":
      return `working-folder:${reference.workingFolderId}`;
    case "workspacePath":
      return `workspace-path:${reference.workingFolderId}:${reference.pathKind}:${reference.relativePath}`;
    case "executionEnvironment":
      return `execution-environment:${reference.executionEnvironmentId}`;
  }
}

/** Finds the active @ or # reference query immediately before a visible caret. */
export function chatReferenceTriggerAtCaret(text: string, caret: number): ChatReferenceTrigger | null {
  const before = text.slice(0, Math.max(0, Math.min(caret, text.length)));
  const match = /(^|\s)([@#])([^\n@#]*)$/u.exec(before);
  if (!match) return null;
  const query = match[3] ?? "";
  return {
    kind: match[2] === "#" ? "channel" : "mention",
    query,
    start: before.length - query.length - 1,
    end: before.length,
  };
}

/** Converts a JavaScript string offset to a UTF-8 byte offset. */
export function chatUtf8Offset(text: string, offset: number): number {
  return new TextEncoder().encode(text.slice(0, offset)).byteLength;
}

/** Converts a UTF-8 byte offset to a JavaScript string offset at a valid code-point boundary. */
export function chatJsOffsetFromUtf8(text: string, byteOffset: number): number {
  if (byteOffset <= 0) return 0;
  let bytes = 0;
  let offset = 0;
  for (const character of text) {
    const length = new TextEncoder().encode(character).byteLength;
    if (bytes + length > byteOffset) break;
    bytes += length;
    offset += character.length;
  }
  return offset;
}

/** Returns references that are valid, non-overlapping, and semantically unique for the text. */
export function normalizeChatMessageReferences(
  text: string,
  references: readonly ChatMessageReference[],
): ChatMessageReference[] {
  const byteLength = chatUtf8Offset(text, text.length);
  const seenIds = new Set<string>();
  const seenSemanticKeys = new Set<string>();
  let previousEnd = 0;
  return [...references]
    .sort((left, right) => left.metadata.startOffset - right.metadata.startOffset)
    .flatMap((reference) => {
      const metadata = reference.metadata;
      const semanticKey = chatReferenceSemanticKey(reference);
      if (
        metadata.startOffset < previousEnd
        || metadata.startOffset >= metadata.endOffset
        || metadata.endOffset > byteLength
        || seenIds.has(metadata.referenceId)
        || seenSemanticKeys.has(semanticKey)
      ) return [];
      const start = chatJsOffsetFromUtf8(text, metadata.startOffset);
      const end = chatJsOffsetFromUtf8(text, metadata.endOffset);
      if (
        chatUtf8Offset(text, start) !== metadata.startOffset
        || chatUtf8Offset(text, end) !== metadata.endOffset
        || text.slice(start, end) !== metadata.plainTextProjection
        || metadata.plainTextProjection.includes("\n")
        || /[\p{Cc}\u202a-\u202e\u2066-\u2069]/iu.test(metadata.plainTextProjection)
      ) return [];
      previousEnd = metadata.endOffset;
      seenIds.add(metadata.referenceId);
      seenSemanticKeys.add(semanticKey);
      return [{ ...reference, metadata: { ...metadata } }];
    });
}

/** Splits text into ordinary runs and validated atomic reference runs. */
export function chatReferenceTextSegments(
  text: string,
  references: readonly ChatMessageReference[],
): ChatReferenceTextSegment[] {
  const segments: ChatReferenceTextSegment[] = [];
  let cursor = 0;
  for (const reference of normalizeChatMessageReferences(text, references)) {
    const start = chatJsOffsetFromUtf8(text, reference.metadata.startOffset);
    const end = chatJsOffsetFromUtf8(text, reference.metadata.endOffset);
    if (start > cursor) segments.push({ kind: "text", text: text.slice(cursor, start) });
    segments.push({ kind: "reference", text: text.slice(start, end), reference });
    cursor = end;
  }
  if (cursor < text.length) segments.push({ kind: "text", text: text.slice(cursor) });
  if (segments.length === 0) segments.push({ kind: "text", text });
  return segments;
}

/** Projects editor-visible reference ranges onto the serialized Markdown request. */
export function chatReferencesForMarkdown(
  document: ChatComposerDocument,
  references: readonly ChatMessageReference[],
): ChatMessageReference[] {
  const plainText = chatComposerPlainText(document);
  const markdown = chatComposerMarkdown(document);
  return normalizeChatMessageReferences(plainText, references).map((reference) => {
    const visibleStart = chatJsOffsetFromUtf8(plainText, reference.metadata.startOffset);
    const visibleEnd = chatJsOffsetFromUtf8(plainText, reference.metadata.endOffset);
    const markdownStart = chatComposerMarkdownOffset(document, visibleStart);
    const markdownEnd = chatComposerMarkdownOffset(document, visibleEnd, "backward");
    return {
      ...reference,
      metadata: {
        ...reference.metadata,
        startOffset: chatUtf8Offset(markdown, markdownStart),
        endOffset: chatUtf8Offset(markdown, markdownEnd),
      },
    };
  });
}

/** Restores persisted Markdown byte ranges to editor-visible reference ranges. */
export function chatReferencesForEditor(
  document: ChatComposerDocument,
  references: readonly ChatMessageReference[],
): ChatMessageReference[] {
  const markdown = chatComposerMarkdown(document);
  const plainText = chatComposerPlainText(document);
  return normalizeChatMessageReferences(markdown, references).map((reference) => {
    const markdownStart = chatJsOffsetFromUtf8(markdown, reference.metadata.startOffset);
    const markdownEnd = chatJsOffsetFromUtf8(markdown, reference.metadata.endOffset);
    const visibleStart = chatComposerVisibleOffset(document, markdownStart);
    const visibleEnd = chatComposerVisibleOffset(document, markdownEnd);
    return {
      ...reference,
      metadata: {
        ...reference.metadata,
        startOffset: chatUtf8Offset(plainText, visibleStart),
        endOffset: chatUtf8Offset(plainText, visibleEnd),
      },
    };
  });
}

/** Expands an edit range to include every atomic reference it touches. */
export function expandEditRangeToChatReferences(
  text: string,
  references: readonly ChatMessageReference[],
  editStart: number,
  editEnd: number,
): { start: number; end: number } {
  let start = editStart;
  let end = editEnd;
  for (const reference of normalizeChatMessageReferences(text, references)) {
    const referenceStart = chatJsOffsetFromUtf8(text, reference.metadata.startOffset);
    const referenceEnd = chatJsOffsetFromUtf8(text, reference.metadata.endOffset);
    const caretInside = editStart === editEnd && editStart > referenceStart && editStart < referenceEnd;
    const overlaps = editStart < referenceEnd && editEnd > referenceStart;
    if (!caretInside && !overlaps) continue;
    start = Math.min(start, referenceStart);
    end = Math.max(end, referenceEnd);
  }
  return { start, end };
}

/** Returns an atomic reference immediately before or after a caret. */
export function chatReferenceAdjacentToCaret(
  text: string,
  references: readonly ChatMessageReference[],
  caret: number,
  direction: "backward" | "forward",
): { start: number; end: number } | null {
  for (const reference of normalizeChatMessageReferences(text, references)) {
    const start = chatJsOffsetFromUtf8(text, reference.metadata.startOffset);
    const end = chatJsOffsetFromUtf8(text, reference.metadata.endOffset);
    if ((direction === "backward" && end === caret) || (direction === "forward" && start === caret)) {
      return { start, end };
    }
  }
  return null;
}

/** Rebases untouched references around one explicit text replacement. */
export function rebaseChatMessageReferences(
  previousText: string,
  nextText: string,
  references: readonly ChatMessageReference[],
  editStart: number,
  editEnd: number,
): ChatMessageReference[] {
  const insertedLength = nextText.length - (previousText.length - (editEnd - editStart));
  return normalizeChatMessageReferences(previousText, references).flatMap((reference) => {
    const start = chatJsOffsetFromUtf8(previousText, reference.metadata.startOffset);
    const end = chatJsOffsetFromUtf8(previousText, reference.metadata.endOffset);
    if (start < editEnd && end > editStart) return [];
    const follows = start >= editEnd;
    const nextStart = follows ? start + insertedLength : start;
    const nextEnd = follows ? end + insertedLength : end;
    if (nextText.slice(nextStart, nextEnd) !== reference.metadata.plainTextProjection) return [];
    return [{
      ...reference,
      metadata: {
        ...reference.metadata,
        startOffset: chatUtf8Offset(nextText, nextStart),
        endOffset: chatUtf8Offset(nextText, nextEnd),
      },
    }];
  });
}

/** Rebases references around the single replacement represented by native editor input. */
export function rebaseChatReferencesAfterInput(
  previousText: string,
  nextText: string,
  references: readonly ChatMessageReference[],
): ChatMessageReference[] {
  let prefix = 0;
  while (prefix < previousText.length && prefix < nextText.length && previousText[prefix] === nextText[prefix]) {
    prefix += 1;
  }
  let suffix = 0;
  while (
    suffix < previousText.length - prefix
    && suffix < nextText.length - prefix
    && previousText[previousText.length - 1 - suffix] === nextText[nextText.length - 1 - suffix]
  ) suffix += 1;
  return rebaseChatMessageReferences(
    previousText,
    nextText,
    references,
    prefix,
    previousText.length - suffix,
  );
}

/** Inserts one atomic reference and prevents a duplicate semantic target. */
export function insertChatMessageReference(
  text: string,
  references: readonly ChatMessageReference[],
  replaceStart: number,
  replaceEnd: number,
  reference: ChatMessageReference,
): ChatReferenceInsertionResult {
  const current = normalizeChatMessageReferences(text, references);
  if (current.some((candidate) => chatReferenceSemanticKey(candidate) === chatReferenceSemanticKey(reference))) {
    return { text, references: current, selection: replaceEnd, inserted: false };
  }
  const expanded = expandEditRangeToChatReferences(text, current, replaceStart, replaceEnd);
  const projection = reference.metadata.plainTextProjection;
  const trailingSpace = expanded.end === text.length || !/^\s/u.test(text.slice(expanded.end)) ? " " : "";
  const insertedText = `${projection}${trailingSpace}`;
  const nextText = `${text.slice(0, expanded.start)}${insertedText}${text.slice(expanded.end)}`;
  const rebased = rebaseChatMessageReferences(text, nextText, current, expanded.start, expanded.end);
  const nextReference: ChatMessageReference = {
    ...reference,
    metadata: {
      ...reference.metadata,
      startOffset: chatUtf8Offset(nextText, expanded.start),
      endOffset: chatUtf8Offset(nextText, expanded.start + projection.length),
    },
  };
  return {
    text: nextText,
    references: [...rebased, nextReference].sort((left, right) => (
      left.metadata.startOffset - right.metadata.startOffset
    )),
    selection: expanded.start + insertedText.length,
    inserted: true,
  };
}

/** Copies complete references inside a selection for same-vault semantic paste. */
export function copyChatReferenceSlice(
  vaultId: string,
  text: string,
  references: readonly ChatMessageReference[],
  start: number,
  end: number,
): ChatReferenceClipboardSlice {
  const byteStart = chatUtf8Offset(text, start);
  const byteEnd = chatUtf8Offset(text, end);
  return {
    vaultId,
    text: text.slice(start, end),
    references: normalizeChatMessageReferences(text, references)
      .filter((reference) => (
        reference.metadata.startOffset >= byteStart && reference.metadata.endOffset <= byteEnd
      ))
      .map((reference) => ({
        ...reference,
        metadata: {
          ...reference.metadata,
          startOffset: reference.metadata.startOffset - byteStart,
          endOffset: reference.metadata.endOffset - byteStart,
        },
      })),
  };
}

/** Pastes trusted same-vault references, while duplicate targets remain ordinary text. */
export function pasteChatReferenceSlice(
  text: string,
  references: readonly ChatMessageReference[],
  start: number,
  end: number,
  slice: ChatReferenceClipboardSlice,
): ChatReferenceInsertionResult {
  const expanded = expandEditRangeToChatReferences(text, references, start, end);
  const nextText = `${text.slice(0, expanded.start)}${slice.text}${text.slice(expanded.end)}`;
  const rebased = rebaseChatMessageReferences(text, nextText, references, expanded.start, expanded.end);
  const seen = new Set(rebased.map(chatReferenceSemanticKey));
  const byteStart = chatUtf8Offset(nextText, expanded.start);
  const inserted = normalizeChatMessageReferences(slice.text, slice.references).flatMap((reference) => {
    const semanticKey = chatReferenceSemanticKey(reference);
    if (seen.has(semanticKey)) return [];
    seen.add(semanticKey);
    return [{
      ...reference,
      metadata: {
        ...reference.metadata,
        referenceId: crypto.randomUUID(),
        startOffset: byteStart + reference.metadata.startOffset,
        endOffset: byteStart + reference.metadata.endOffset,
      },
    }];
  });
  return {
    text: nextText,
    references: [...rebased, ...inserted].sort((left, right) => (
      left.metadata.startOffset - right.metadata.startOffset
    )),
    selection: expanded.start + slice.text.length,
    inserted: inserted.length > 0,
  };
}
