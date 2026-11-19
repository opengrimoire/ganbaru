import type { ChatMessageReference, ChatParticipantId } from "$lib/chat/contracts";

/** A channel agent the composer can suggest mentioning. */
export interface MentionSuggestionCandidate {
  id: ChatParticipantId;
  displayName: string;
}

/** An agent named in the draft without a mention, and where the name is. */
export interface MentionSuggestion {
  participantId: ChatParticipantId;
  displayName: string;
  /** Start offset of the name in the draft's plain text. */
  start: number;
  /** End offset of the name in the draft's plain text, exclusive. */
  end: number;
}

const WORD_CHARACTER = /[\p{L}\p{N}\p{M}_]/u;

/**
 * Finds a channel agent the draft names in plain words but does not mention.
 *
 * A name matches as a whole word with its exact case, since agent names carry everyday words such as "Sol" or "Opus",
 * and never inside an existing reference or right after `@`, which the reference picker owns. A name that ends in a
 * digit does not match the start of a longer version, so "Opus 5" is not found in "Opus 5.5". An agent already mentioned is never suggested.
 * The earliest name in the text wins; at the same position the longer name wins, and then roster
 * order. The suggestion is local: it never posts and never calls a provider.
 *
 * @param text The draft's plain text.
 * @param references The draft's references, with offsets into `text`.
 * @param roster The channel agents that can be mentioned, in roster order.
 * @returns The suggested agent and the range of the name, or `null`.
 */
export function suggestAgentMention(
  text: string,
  references: readonly ChatMessageReference[],
  roster: readonly MentionSuggestionCandidate[],
): MentionSuggestion | null {
  const mentioned = new Set(references.flatMap((reference) => (
    reference.kind === "participant" ? [reference.participantId] : []
  )));
  let best: MentionSuggestion | null = null;
  for (const candidate of roster) {
    const name = candidate.displayName.trim();
    if (!name || mentioned.has(candidate.id)) continue;
    const start = firstWholeWordMatch(text, name, references);
    if (start === null) continue;
    const end = start + name.length;
    if (!best || start < best.start || (start === best.start && end > best.end)) {
      best = { participantId: candidate.id, displayName: candidate.displayName, start, end };
    }
  }
  return best;
}

function firstWholeWordMatch(
  text: string,
  needle: string,
  references: readonly ChatMessageReference[],
): number | null {
  for (let start = text.indexOf(needle); start >= 0; start = text.indexOf(needle, start + 1)) {
    const end = start + needle.length;
    if (isWordCharacter(text, start - 1) || isWordCharacter(text, end)) continue;
    if (continuesVersion(text, end)) continue;
    if (text[start - 1] === "@") continue;
    if (references.some((reference) => start < reference.metadata.endOffset && end > reference.metadata.startOffset)) continue;
    return start;
  }
  return null;
}

/** Whether a version number goes on at `end`, such as the `.5` after `Opus 5` in `Opus 5.5`. */
function continuesVersion(text: string, end: number): boolean {
  return /\d/u.test(text[end - 1] ?? "") && text[end] === "." && /\d/u.test(text[end + 1] ?? "");
}

function isWordCharacter(text: string, index: number): boolean {
  const character = text[index];
  return character !== undefined && WORD_CHARACTER.test(character);
}
