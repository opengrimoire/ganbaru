import type { NotesRichTextAnnotationName } from "./core";

export interface NotesRichTextFormattingShortcutInput {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

/**
 * Resolve a selected-text formatting shortcut to the annotation it toggles.
 */
export function notesRichTextFormattingShortcutAnnotationName(
  input: NotesRichTextFormattingShortcutInput,
): NotesRichTextAnnotationName | null {
  if (!(input.ctrlKey || input.metaKey) || input.altKey) return null;
  const key = input.key.toLowerCase();
  if (!input.shiftKey && key === "b") return "bold";
  if (!input.shiftKey && key === "i") return "italic";
  if (!input.shiftKey && key === "u") return "underline";
  if (input.shiftKey && key === "s") return "strikethrough";
  if (!input.shiftKey && key === "e") return "code";
  return null;
}

/**
 * Return whether the input asks to open the selected-text link editor.
 */
export function notesRichTextLinkShortcutRequested(
  input: NotesRichTextFormattingShortcutInput,
): boolean {
  return (input.ctrlKey || input.metaKey)
    && !input.altKey
    && !input.shiftKey
    && input.key.toLowerCase() === "k";
}

/**
 * Return whether the input asks to convert selected text into an inline equation.
 */
export function notesRichTextEquationShortcutRequested(
  input: NotesRichTextFormattingShortcutInput,
): boolean {
  return (input.ctrlKey || input.metaKey)
    && input.shiftKey
    && !input.altKey
    && input.key.toLowerCase() === "e";
}
