import type { NotesWorkingMarkdownTreeItem } from "$lib/notes/working-markdown/tree";
import type { NotesWorkingMarkdownTreeRead } from "$lib/notes/types";

const EMPTY_WORKING_MARKDOWN_TREE: NotesWorkingMarkdownTreeRead = Object.freeze({
  roots: [],
  unavailableWorkingFolderIds: [],
});

/** Return the empty tree because mobile has no project working-folder authority. */
export async function listNotesWorkingMarkdown(): Promise<NotesWorkingMarkdownTreeRead> {
  return EMPTY_WORKING_MARKDOWN_TREE;
}

/** Return no working Markdown rows in the mobile composition. */
export function buildWorkingMarkdownTreeItems(): NotesWorkingMarkdownTreeItem[] {
  return [];
}

export { default as NotesWorkingMarkdownTree } from "./NotesWorkingMarkdownTree.mobile.svelte";
