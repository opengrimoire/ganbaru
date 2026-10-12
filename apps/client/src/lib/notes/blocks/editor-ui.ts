import type { AppLocale } from "$lib/i18n/locales";
import type { NotesBlock, NotesBlockType } from "$lib/notes/types";
import { blockPlainText } from "./queries";

const NOTES_HEADING_CLASSES: Readonly<Record<string, string>> = {
  heading_1: "notes-heading-1",
  heading_2: "notes-heading-2",
  heading_3: "notes-heading-3",
  heading_4: "notes-heading-4",
  heading_5: "notes-heading-5",
  heading_6: "notes-heading-6",
};

/**
 * Return the base classes used by editable Notes text blocks.
 */
export function notesTextareaClass(type: NotesBlockType): string {
  const base =
    "min-h-8 w-full resize-none overflow-hidden bg-transparent pr-1 py-1 outline-none placeholder:text-muted-foreground";
  const headingClass = notesHeadingTextClass(type);
  if (headingClass) return `${base} ${headingClass}`;
  if (type === "code") return `${base} rounded-md bg-muted/60 pl-1 font-mono text-[0.82rem] leading-relaxed`;
  if (type === "callout") return `${base} notes-editor-body-text leading-normal`;
  if (type === "quote") return `${base} border-l-2 border-border pl-3 notes-editor-body-text leading-normal italic`;
  return `${base} notes-editor-body-text leading-normal`;
}

/**
 * Return the classes used by editable rich text Notes blocks.
 */
export function notesRichTextEditorClass(type: NotesBlockType): string {
  return `${notesTextareaClass(type)} notes-rich-text-editor block cursor-text whitespace-pre-wrap break-words`;
}

/**
 * Return the preview classes used when saved rich text is visible outside editing.
 */
export function notesRichTextPreviewClass(type: NotesBlockType): string {
  return `${notesRichTextEditorClass(type)} text-left`;
}

/**
 * Return the visible marker shown before list-like Notes blocks.
 */
export function notesBlockMarker(
  type: NotesBlockType,
  ordinal = 1,
  locale?: AppLocale,
): string {
  if (type === "bulleted_list_item") return "•";
  if (type === "numbered_list_item") {
    const label = locale ? new Intl.NumberFormat(locale, { useGrouping: false }).format(ordinal) : String(ordinal);
    return `${label}.`;
  }
  if (type === "quote") return "";
  return "";
}

/** Number consecutive ordered-list siblings independently for each parent in document order. */
export function notesNumberedListOrdinals(
  blocks: readonly { id: string; type: string; parentId: string | null; indent?: number }[],
): ReadonlyMap<string, number> {
  const counters = new Map<string | null, Map<number, number>>();
  const ordinals = new Map<string, number>();
  for (const block of blocks) {
    const levels = counters.get(block.parentId) ?? new Map<number, number>();
    counters.set(block.parentId, levels);
    const indent = block.indent ?? 0;
    for (const level of levels.keys()) if (level > indent) levels.delete(level);
    if (block.type !== "numbered_list_item") {
      levels.set(indent, 0);
      continue;
    }
    const ordinal = (levels.get(indent) ?? 0) + 1;
    levels.set(indent, ordinal);
    ordinals.set(block.id, ordinal);
  }
  return ordinals;
}

/**
 * Report whether a page body holds nothing but the single empty paragraph every page keeps for typing.
 *
 * @param items The page's flattened body blocks in document order.
 * @returns True when the body should show the empty page hint.
 */
export function isNotesPageBodyEmpty(items: readonly { block: NotesBlock }[]): boolean {
  if (items.length !== 1) return false;
  const { block } = items[0];
  return block.type === "paragraph" && !block.has_children && blockPlainText(block) === "";
}

/** Share heading typography across editing and historical reading surfaces. */
export function notesHeadingTextClass(type: string): string {
  return Object.hasOwn(NOTES_HEADING_CLASSES, type)
    ? `notes-heading-text ${NOTES_HEADING_CLASSES[type]} font-semibold leading-[1.3]`
    : "";
}
