import type { ChatTimelinePageRead, ChatTurnId, ChatTurnState } from "$lib/chat/contracts";
import { commitMessageTooLong, normalizeCommitMessage } from "$lib/chat/workspace/commit-message";

/** What an agent drafts: a commit message, or a change request title and body. */
export type DraftKind = "commit" | "change_request";

/** Longest drafted commit message, in characters, that fills the commit field. */
export const COMMIT_DRAFT_MAX_CHARS = 4_000;
/** Longest drafted change request title, in characters, matching the native title bound. */
export const CHANGE_REQUEST_TITLE_MAX_CHARS = 1_024;

/** Why a reply holds no usable draft; a draft is refused visibly instead of being cut. */
export type DraftRejection = "empty" | "too_long" | "title_too_long";

/** The draft a reply holds, or why it holds none. */
export type DraftExtraction =
  | { ok: true; kind: "commit"; message: string }
  | { ok: true; kind: "change_request"; title: string; body: string }
  | { ok: false; reason: DraftRejection };

/** Turn states after which a turn produces no further reply. */
const TERMINAL_TURN_STATES: readonly ChatTurnState[] = ["completed", "interrupted", "failed"];

const FENCE_OPENING = /^ {0,3}(`{3,}|~{3,})/u;

/**
 * Extracts a draft from an agent reply: the first fenced block, or the whole reply when it has
 * no closed fence. Line endings become LF and surrounding whitespace is trimmed.
 *
 * @param text Reply markdown.
 * @param kind Which draft the reply answers.
 * @returns The draft, or the reason it cannot fill the field.
 */
export function extractDraft(text: string, kind: DraftKind): DraftExtraction {
  const draft = (firstFencedBlock(text) ?? normalizeCommitMessage(text)).trim();
  if (!draft) return { ok: false, reason: "empty" };
  if (kind === "commit") {
    if (draft.length > COMMIT_DRAFT_MAX_CHARS || commitMessageTooLong(draft)) return { ok: false, reason: "too_long" };
    return { ok: true, kind, message: draft };
  }
  const [firstLine, ...rest] = draft.split("\n");
  const title = firstLine.trim();
  if (title.length > CHANGE_REQUEST_TITLE_MAX_CHARS) return { ok: false, reason: "title_too_long" };
  return { ok: true, kind, title, body: rest.join("\n").trim() };
}

/**
 * The content of the first closed fenced code block in `text`, with LF line endings.
 *
 * @param text Markdown to scan.
 * @returns The block content, or null when no fence opens and closes.
 */
export function firstFencedBlock(text: string): string | null {
  const lines = text.replace(/\r\n?/gu, "\n").split("\n");
  for (let start = 0; start < lines.length; start += 1) {
    const opening = FENCE_OPENING.exec(lines[start]);
    if (!opening) continue;
    const fence = opening[1];
    const marker = fence[0];
    const info = lines[start].slice(opening[0].length);
    if (marker === "`" && info.includes("`")) continue;
    const closing = new RegExp(`^ {0,3}${marker === "`" ? "`" : "~"}{${fence.length},}\\s*$`, "u");
    for (let end = start + 1; end < lines.length; end += 1) {
      if (closing.test(lines[end])) return lines.slice(start + 1, end).join("\n");
    }
    return null;
  }
  return null;
}

/**
 * Whether a turn has finished, as one exact turn read reports it.
 *
 * @param page Timeline read of the turn.
 * @param turnId Turn the draft was requested in.
 * @returns The terminal state, or null while the turn is still live or absent.
 */
export function draftTurnOutcome(page: ChatTimelinePageRead, turnId: ChatTurnId): ChatTurnState | null {
  const state = page.turns.find((turn) => turn.turnId === turnId)?.state ?? null;
  return state !== null && TERMINAL_TURN_STATES.includes(state) ? state : null;
}

/**
 * The reply text of a turn: its latest final answer message, else its latest assistant message.
 * Only stored message rows count, since their markdown is complete, unlike activity previews.
 *
 * @param page Timeline read of the turn.
 * @param turnId Turn the draft was requested in.
 * @returns The reply markdown, or null when the turn has no assistant message.
 */
export function draftReplyText(page: ChatTimelinePageRead, turnId: ChatTurnId): string | null {
  let latest: { sequence: number; markdown: string } | null = null;
  let latestFinal: { sequence: number; markdown: string } | null = null;
  for (const item of page.items) {
    if (item.kind !== "message" || item.turnId !== turnId) continue;
    const data = item.data.value;
    if (typeof data !== "object" || data === null || Array.isArray(data)) continue;
    if (data.role !== "assistant" || typeof data.markdown !== "string") continue;
    const row = { sequence: item.sequenceAnchor, markdown: data.markdown };
    if (!latest || row.sequence > latest.sequence) latest = row;
    const metadata = data.metadata;
    const final = typeof metadata === "object" && metadata !== null && !Array.isArray(metadata)
      && metadata.phase === "final_answer";
    if (final && (!latestFinal || row.sequence > latestFinal.sequence)) latestFinal = row;
  }
  return (latestFinal ?? latest)?.markdown ?? null;
}
