import type { NotesBlockUpdate } from "$lib/notes/types";

/** Payload keys stored as a block's `text` group; mirrors `ganbaru_notes::block_storage::BLOCK_TEXT_KEYS`. */
export const NOTES_BLOCK_TEXT_KEYS: ReadonlySet<string> = new Set(["rich_text", "cells", "caption"]);

/** Device-local view keys, which no other device changes, so the editor's value always stays. */
const NOTES_BLOCK_VIEW_KEYS: ReadonlySet<string> = new Set(["ganbaru_open", "editing_locked"]);

/** A block update split into the groups sync merges separately. */
interface NotesBlockParts {
  type: NotesBlockUpdate["type"];
  /** Non-text payload keys; with `type`, the block's `kind` group. */
  attrs: Record<string, unknown>;
  /** Text payload keys, the block's `text` group. */
  text: Record<string, unknown>;
  view: Record<string, unknown>;
}

/** The outcome of replaying an unsaved block edit on newer canonical content. */
export type NotesBlockRebase =
  /** The update to save on the canonical revision: each group takes the side that changed it. */
  | { kind: "rebased"; update: NotesBlockUpdate }
  /** Both sides changed the text differently, so the user chooses. */
  | { kind: "conflict" }
  /** The block was trashed or deleted elsewhere. */
  | { kind: "removed" };

function payloadOf(update: NotesBlockUpdate): Record<string, unknown> {
  const payload: unknown = (update as Record<string, unknown>)[update.type];
  return typeof payload === "object" && payload !== null && !Array.isArray(payload)
    ? payload as Record<string, unknown>
    : {};
}

function splitBlockUpdate(update: NotesBlockUpdate): NotesBlockParts {
  const parts: NotesBlockParts = { type: update.type, attrs: {}, text: {}, view: {} };
  for (const [key, value] of Object.entries(payloadOf(update))) {
    if (NOTES_BLOCK_VIEW_KEYS.has(key)) parts.view[key] = value;
    else if (NOTES_BLOCK_TEXT_KEYS.has(key)) parts.text[key] = value;
    else parts.attrs[key] = value;
  }
  return parts;
}

/** JSON with object keys sorted, so equal values compare equal whatever order their keys were written in. */
function stableJson(value: unknown): string {
  return JSON.stringify(value, (_key, inner: unknown) => {
    if (typeof inner !== "object" || inner === null || Array.isArray(inner)) return inner;
    const record = inner as Record<string, unknown>;
    return Object.fromEntries(Object.keys(record).sort().map((key) => [key, record[key]]));
  });
}

function kindKey(parts: NotesBlockParts): string {
  return stableJson({ type: parts.type, attrs: parts.attrs });
}

function textKey(parts: NotesBlockParts): string {
  return stableJson(parts.text);
}

/**
 * Builds an update from its groups. The payload key always matches `type`, which is the shape every
 * `NotesBlockUpdate` member has; the parts come from updates of the same union, so the payload fits its type.
 */
function composeBlockUpdate(parts: NotesBlockParts): NotesBlockUpdate {
  const payload = { ...parts.attrs, ...parts.text, ...parts.view };
  return { type: parts.type, [parts.type]: payload } as unknown as NotesBlockUpdate;
}

/**
 * Replays the editor's unsaved block edit on canonical content that changed meanwhile, per sync group.
 *
 * The `kind` group (type and attributes) and the `text` group each keep the editor's value when only the
 * editor changed them and take the canonical value otherwise. Text changed on both sides to different values is
 * a conflict the user resolves. A move changes neither group, so it never conflicts. View state is device-local
 * and stays as the editor has it. Applying the result again on the same canonical content gives the same update.
 *
 * @param base The canonical content the editor started from, or `undefined` when it is not known, in which case
 *   any text difference is a conflict and the editor's kind wins.
 * @param local The editor's unsaved content.
 * @param canonical The current canonical content, or `null` when the block was trashed or deleted.
 */
export function rebaseNotesBlockEdit(
  base: NotesBlockUpdate | undefined,
  local: NotesBlockUpdate,
  canonical: NotesBlockUpdate | null,
): NotesBlockRebase {
  if (!canonical) return { kind: "removed" };
  const mine = splitBlockUpdate(local);
  const theirs = splitBlockUpdate(canonical);
  const original = base ? splitBlockUpdate(base) : null;
  const textChangedHere = !original || textKey(mine) !== textKey(original);
  const textChangedThere = !original || textKey(theirs) !== textKey(original);
  if (textChangedHere && textChangedThere && textKey(mine) !== textKey(theirs)) return { kind: "conflict" };
  const kindChangedHere = !original || kindKey(mine) !== kindKey(original);
  const kindSide = kindChangedHere ? mine : theirs;
  const textSide = textChangedHere ? mine : theirs;
  return {
    kind: "rebased",
    update: composeBlockUpdate({ type: kindSide.type, attrs: kindSide.attrs, text: textSide.text, view: mine.view }),
  };
}

/** Whether two block updates carry the same type, attributes, and text, ignoring device-local view state. */
export function notesBlockContentEqual(left: NotesBlockUpdate, right: NotesBlockUpdate): boolean {
  const a = splitBlockUpdate(left);
  const b = splitBlockUpdate(right);
  return kindKey(a) === kindKey(b) && textKey(a) === textKey(b);
}

/** Where the caret or selected range was, by block and character offsets. `start` equals `end` for a caret. */
export interface NotesCaret {
  blockId: string;
  start: number;
  end: number;
}

/**
 * Where the caret goes after the open page refreshed from canonical blocks.
 *
 * The caret is held by block id and offsets, so blocks inserted above or below it leave it where it is. When its
 * block remains, a selected range stays selected, clamped to the block's new text. When its block is gone, the caret
 * collapses at the end of the nearest block before it that remains, or the start of the nearest one after it, and
 * is `null` when no block remains.
 *
 * @param caret The caret before the refresh, or `null` when the editor had none.
 * @param previousOrder Visible block ids in document order before the refresh.
 * @param nextOrder Visible block ids in document order after the refresh.
 * @param textLength Length of a remaining block's text, to place the caret at its end.
 */
export function notesCaretAfterRefresh(
  caret: NotesCaret | null,
  previousOrder: readonly string[],
  nextOrder: readonly string[],
  textLength: (blockId: string) => number,
): NotesCaret | null {
  if (!caret) return null;
  const remaining = new Set(nextOrder);
  if (remaining.has(caret.blockId)) {
    const length = textLength(caret.blockId);
    return { blockId: caret.blockId, start: Math.min(caret.start, length), end: Math.min(caret.end, length) };
  }
  const index = previousOrder.indexOf(caret.blockId);
  for (let before = index - 1; before >= 0; before -= 1) {
    const id = previousOrder[before];
    if (id !== undefined && remaining.has(id)) return collapsedCaret(id, textLength(id));
  }
  for (let after = index + 1; index >= 0 && after < previousOrder.length; after += 1) {
    const id = previousOrder[after];
    if (id !== undefined && remaining.has(id)) return collapsedCaret(id, 0);
  }
  const first = nextOrder[0];
  return first === undefined ? null : collapsedCaret(first, 0);
}

function collapsedCaret(blockId: string, offset: number): NotesCaret {
  return { blockId, start: offset, end: offset };
}
