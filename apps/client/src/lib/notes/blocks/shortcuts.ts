import type { NotesBlockType } from "$lib/notes/types";

const SHORTCUTS: Readonly<Record<string, NotesBlockType>> = Object.freeze({
  "#": "heading_1",
  "##": "heading_2",
  "###": "heading_3",
  "####": "heading_4",
  "#####": "heading_5",
  "######": "heading_6",
  "-": "bulleted_list_item",
  "1.": "numbered_list_item",
  "[]": "to_do",
  ">": "toggle",
  "\"": "quote",
  "```": "code",
});

/** Resolve a Notion-style text shortcut typed into an empty paragraph. */
export function blockTypeForTextShortcut(text: string): NotesBlockType | null {
  return SHORTCUTS[text.trim()] ?? null;
}

export function isTextShortcutTriggerKey(key: string): boolean {
  return key === " " || key === "Enter";
}
