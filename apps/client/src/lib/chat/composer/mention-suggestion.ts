import type { ChatMessageReference, ChatParticipantId } from "$lib/chat/contracts";

/** A channel agent the composer can suggest mentioning. */
export interface MentionSuggestionCandidate {
  id: ChatParticipantId;
  displayName: string;
  /**
   * Whether the name only matches with its exact case. Model and named agents carry everyday words such as "Sol" or
   * "Opus", so a lowercase word in a sentence is not read as their name.
   */
  matchCase?: boolean;
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
 * A name matches as a whole word, ignoring case unless the candidate asks to match case, and never inside an existing reference or right
 * after `@`, which the reference picker owns. An agent already mentioned is never suggested.
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
  const haystack = text.toLocaleLowerCase();
  let best: MentionSuggestion | null = null;
  for (const candidate of roster) {
    const name = candidate.displayName.trim();
    if (!name || mentioned.has(candidate.id)) continue;
    const start = candidate.matchCase
      ? firstWholeWordMatch(text, text, name, references)
      : firstWholeWordMatch(text, haystack, name.toLocaleLowerCase(), references);
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
  haystack: string,
  needle: string,
  references: readonly ChatMessageReference[],
): number | null {
  if (haystack.length !== text.length) return null;
  for (let start = haystack.indexOf(needle); start >= 0; start = haystack.indexOf(needle, start + 1)) {
    const end = start + needle.length;
    if (isWordCharacter(text, start - 1) || isWordCharacter(text, end)) continue;
    if (text[start - 1] === "@") continue;
    if (references.some((reference) => start < reference.metadata.endOffset && end > reference.metadata.startOffset)) continue;
    return start;
  }
  return null;
}

function isWordCharacter(text: string, index: number): boolean {
  const character = text[index];
  return character !== undefined && WORD_CHARACTER.test(character);
}

/** A known agent the mention menu may offer to add to the channel. */
export interface MentionableAgent {
  participant: { id: ChatParticipantId; displayName: string; archivedAt: string | null };
}

/**
 * Returns the known agents the mention menu offers as "Add @X to #channel": agents that are not archived, not already
 * in the channel, and whose name contains the typed query, ignoring case. Choosing one adds it to the channel and keeps
 * the mention; a message alone never adds an agent.
 *
 * @param agents Every known agent, in directory order.
 * @param memberIds The participants that are members of the channel now.
 * @param query The text typed after `@`.
 */
export function agentsToAddForMention<Agent extends MentionableAgent>(
  agents: readonly Agent[],
  memberIds: ReadonlySet<ChatParticipantId>,
  query: string,
): Agent[] {
  const normalized = query.trim().toLocaleLowerCase();
  return agents.filter((agent) => (
    agent.participant.archivedAt === null
      && !memberIds.has(agent.participant.id)
      && (!normalized || agent.participant.displayName.toLocaleLowerCase().includes(normalized))
  ));
}
