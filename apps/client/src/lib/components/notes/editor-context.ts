import { getContext, setContext } from "svelte";
import { getNotes } from "$lib/stores/notes.svelte";
import type { NotesEditorStore } from "$lib/stores/notes/editor-session.svelte";

const NOTES_EDITOR_CONTEXT = Symbol("notes-editor");

/** Scope all block renderers and panels to their owning live editor session. */
export function provideNotesEditor(store: NotesEditorStore): void {
  setContext(NOTES_EDITOR_CONTEXT, store);
}

/** Standalone block renderers use the active workspace when no editor owns them. */
export function getNotesEditor(): NotesEditorStore {
  return getContext<NotesEditorStore | undefined>(NOTES_EDITOR_CONTEXT) ?? getNotes();
}
