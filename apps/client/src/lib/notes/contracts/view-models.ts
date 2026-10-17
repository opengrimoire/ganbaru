import type { NotesBlock, NotesColumnBlock, NotesParagraphBlock } from "./core";

export interface NotesBlockTreeItem {
  block: NotesBlock;
  depth: number;
  parentId: string;
  previousSiblingId: string | null;
  previousVisibleId: string | null;
}

export interface NotesColumnBlockItems {
  column: NotesColumnBlock;
  items: NotesBlockTreeItem[];
}

export interface NotesTabBlockItems {
  label: NotesParagraphBlock;
  items: NotesBlockTreeItem[];
}

export interface NotesTableOfContentsItem {
  blockId: string;
  title: string;
  level: 1 | 2 | 3 | 4 | 5 | 6;
}
