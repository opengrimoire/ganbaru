import type { NotesConflictTarget, NotesFieldConflict } from "$lib/api/notes/conflicts";
import { NOTES_BLOCK_TEXT_KEYS } from "$lib/notes/editor/block-rebase";
import type { NotesBlock, NotesBlockUpdate } from "$lib/notes/types";
import type { NotesBlockConflictChoice, NotesBlockEditConflict } from "$lib/stores/notes/persistence";

/** Characters of a version shown before it is cut, so a long block does not fill the card. */
export const NOTES_CONFLICT_PREVIEW_CHARS = 240;

/** Text of a version cut to {@link NOTES_CONFLICT_PREVIEW_CHARS} characters with an ellipsis. */
export function previewNotesConflictText(text: string, maxChars = NOTES_CONFLICT_PREVIEW_CHARS): string {
  const characters = Array.from(text);
  return characters.length <= maxChars ? text : `${characters.slice(0, maxChars).join("").trimEnd()}…`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Plain text of a rich text array, or of nested arrays such as table cells, with cells separated by tabs. */
function richTextValueText(value: unknown): string {
  if (!Array.isArray(value)) return "";
  if (value.every(Array.isArray)) return value.map(richTextValueText).join("\t");
  return value
    .map((item: unknown) => (isRecord(item) && typeof item.plain_text === "string" ? item.plain_text : ""))
    .join("");
}

/**
 * Plain text of a block's text group (rich text, table cells, and captions), the part a block edit conflict is
 * about. Captions follow the main text on their own line.
 */
export function notesBlockUpdateText(update: Pick<NotesBlockUpdate, "type"> | NotesBlock): string {
  const payload: unknown = (update as Record<string, unknown>)[update.type];
  if (!isRecord(payload)) return "";
  const parts: string[] = [];
  for (const key of NOTES_BLOCK_TEXT_KEYS) {
    const text = richTextValueText(payload[key]);
    if (text) parts.push(text);
  }
  return parts.join("\n");
}

/** What a block edit conflict card shows: the two texts and the choices it offers, in order. */
export interface NotesBlockEditConflictView {
  blockId: string;
  /** Whether the block was removed elsewhere rather than edited. */
  removed: boolean;
  mine: string;
  /** The other device's text, or null when the block was removed. */
  theirs: string | null;
  choices: NotesBlockConflictChoice[];
}

/**
 * Presents a held block edit. An edited block offers keeping mine, taking theirs, or keeping both as two
 * blocks; a removed block offers restoring it with my text or discarding my text.
 */
export function notesBlockEditConflictView(conflict: NotesBlockEditConflict): NotesBlockEditConflictView {
  const mine = notesBlockUpdateText(conflict.mine);
  if (conflict.kind === "removed" || !conflict.theirs) {
    return { blockId: conflict.blockId, removed: true, mine, theirs: null, choices: ["mine", "theirs"] };
  }
  return {
    blockId: conflict.blockId,
    removed: false,
    mine,
    theirs: notesBlockUpdateText(conflict.theirs),
    choices: ["mine", "theirs", "both"],
  };
}

/** Catalog keys for the title of a surfaced conflict card, one per field kind without a name to show. */
export type NotesFieldConflictTitleKey =
  | "notes.conflict.titleTitle"
  | "notes.conflict.blockTitle"
  | "notes.conflict.threadTitle"
  | "notes.conflict.commentTitle";

/** Title key of a surfaced conflict card, or null for a property value, whose title names the property. */
export function notesFieldConflictTitleKey(target: NotesConflictTarget): NotesFieldConflictTitleKey | null {
  switch (target) {
    case "page_title":
      return "notes.conflict.titleTitle";
    case "block_text":
      return "notes.conflict.blockTitle";
    case "thread_status":
      return "notes.conflict.threadTitle";
    case "comment_body":
      return "notes.conflict.commentTitle";
    case "property_value":
      return null;
  }
}

/** Order of surfaced conflict cards on a page: the title first, then values, blocks, and the comment panel. */
const TARGET_ORDER: readonly NotesConflictTarget[] = [
  "page_title",
  "property_value",
  "block_text",
  "thread_status",
  "comment_body",
];

/**
 * Surfaced conflicts of a page in the order the page shows its fields, each with the displayed version first so
 * the version every device shows now leads the card. Blocks follow the page's block order when it is given.
 */
export function orderNotesFieldConflicts(
  conflicts: readonly NotesFieldConflict[],
  blockOrder: readonly string[] = [],
): NotesFieldConflict[] {
  const blockRank = new Map(blockOrder.map((id, index) => [id, index]));
  const blockPosition = (conflict: NotesFieldConflict): number =>
    conflict.target === "block_text" ? (blockRank.get(conflict.id) ?? blockOrder.length) : 0;
  return conflicts
    .map((conflict) => ({
      ...conflict,
      versions: [...conflict.versions].sort((a, b) => Number(b.displayed) - Number(a.displayed)),
    }))
    .sort((a, b) =>
      TARGET_ORDER.indexOf(a.target) - TARGET_ORDER.indexOf(b.target) || blockPosition(a) - blockPosition(b));
}

/** Name of a page property by its id, read from the page's composed properties, or null when it is absent. */
export function notesPropertyNameById(properties: Readonly<Record<string, unknown>>, propertyId: string): string | null {
  for (const [name, property] of Object.entries(properties)) {
    if (isRecord(property) && property.id === propertyId) return name;
  }
  return null;
}

/** Whether a stored discussion status reads as resolved; any other status reads as open. */
export function notesThreadStatusIsResolved(status: string): boolean {
  return status === "resolved";
}
