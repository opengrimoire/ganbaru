import type { NotesTextSelection } from "$lib/notes/editor/selection";
import {
  blockTypeForTextShortcut,
  isTextShortcutTriggerKey,
} from "./shortcuts";
import {
  notesEmptyEnterReturnsParagraph,
  notesEnterSplitsRichTextBlock,
} from "./enter";
import { notesBackspaceCanMergeBlockTypes, notesBackspaceRemovesBlockFormat } from "./backspace";
import type { NotesBlockType } from "$lib/notes/types";

export type NotesKeyboardAction =
  | { type: "none" }
  | { type: "replace_text"; text: string; selection: NotesTextSelection; preventDefault: true }
  | { type: "open_slash_menu"; preventDefault: false }
  | { type: "insert_newline"; preventDefault: true }
  | { type: "create_sibling"; preventDefault: true }
  | {
    type: "split_text_block";
    selectionStart: number;
    selectionEnd: number;
    text: string;
    preventDefault: true;
  }
  | { type: "convert_to_paragraph"; preventDefault: true }
  | { type: "remove_block_format"; preventDefault: true }
  | { type: "apply_text_shortcut"; blockType: NotesBlockType; preventDefault: true }
  | { type: "toggle_block_open"; preventDefault: true }
  | { type: "delete_block"; preventDefault: true }
  | { type: "merge_with_previous"; preventDefault: true }
  | { type: "nest"; preventDefault: true; selection?: NotesTextSelection }
  | { type: "outdent"; preventDefault: true; selection?: NotesTextSelection }
  | { type: "move_up"; preventDefault: true }
  | { type: "move_down"; preventDefault: true };

export interface NotesKeyboardPlanInput {
  key: string;
  code?: string;
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  text: string;
  selectionStart: number;
  selectionEnd: number;
  blockType: NotesBlockType;
  previousBlockType: NotesBlockType | null;
  isOnlyBlock: boolean;
  indentationDepth?: number;
}

/** Recognize Tab when WebKitGTK reports Shift+Tab as an unidentified logical key. */
export function isNotesTabKey(input: { key: string; code?: string }): boolean {
  return input.key === "Tab"
    || ((input.key === "Unidentified" || input.key === "") && input.code === "Tab");
}

/** Plan editor behavior from key state without touching the DOM. */
export function planNotesKeyboardAction(input: NotesKeyboardPlanInput): NotesKeyboardAction {
  if (input.altKey) return { type: "none" };

  const primaryModifier = input.ctrlKey || input.metaKey;
  if (primaryModifier && input.shiftKey && input.key === "ArrowUp") {
    return { type: "move_up", preventDefault: true };
  }
  if (primaryModifier && input.shiftKey && input.key === "ArrowDown") {
    return { type: "move_down", preventDefault: true };
  }
  if (primaryModifier && !input.shiftKey && input.key === "Enter" && input.blockType === "toggle") {
    return { type: "toggle_block_open", preventDefault: true };
  }

  if (input.metaKey) return { type: "none" };

  if (
    input.key === "/"
    && !input.ctrlKey
    && input.blockType !== "code"
    && input.selectionStart === 0
    && input.selectionEnd === 0
    && input.text.length === 0
  ) {
    return { type: "open_slash_menu", preventDefault: false };
  }

  if (isNotesTabKey(input) && !input.ctrlKey) {
    if (input.blockType === "code") return planCodeIndentation(input);
    return input.shiftKey
      ? { type: "outdent", preventDefault: true }
      : { type: "nest", preventDefault: true };
  }

  if ((input.indentationDepth ?? 0) > 0 && input.blockType !== "code"
    && !input.ctrlKey && !input.shiftKey && input.selectionStart === input.selectionEnd
    && ((input.key === "Backspace" && input.selectionStart === 0)
      || (input.key === "Enter" && input.text.length === 0 && input.blockType !== "toggle"))) {
    return { type: "outdent", preventDefault: true, selection: { start: input.selectionStart, end: input.selectionEnd } };
  }

  if (input.key === "Enter") {
    if (input.blockType === "code" && !input.ctrlKey) {
      return { type: "insert_newline", preventDefault: true };
    }
    if (input.blockType === "code" && input.ctrlKey) {
      return { type: "create_sibling", preventDefault: true };
    }
    if (input.shiftKey) {
      return { type: "insert_newline", preventDefault: true };
    }
    if (input.blockType === "paragraph" && isTextShortcutTriggerKey(input.key)) {
      const shortcutType = blockTypeForTextShortcut(input.text);
      if (shortcutType) {
        return {
          type: "apply_text_shortcut",
          blockType: shortcutType,
          preventDefault: true,
        };
      }
    }
    if (input.text.trim().length === 0 && notesEmptyEnterReturnsParagraph(input.blockType)) {
      return { type: "convert_to_paragraph", preventDefault: true };
    }
    if (notesEnterSplitsRichTextBlock(input.blockType)) {
      return {
        type: "split_text_block",
        selectionStart: input.selectionStart,
        selectionEnd: input.selectionEnd,
        text: input.text,
        preventDefault: true,
      };
    }
    return { type: "create_sibling", preventDefault: true };
  }

  if (
    input.key === " "
    && !input.ctrlKey
    && !input.shiftKey
    && input.blockType === "paragraph"
  ) {
    const shortcutType = blockTypeForTextShortcut(input.text);
    if (shortcutType) {
      return {
        type: "apply_text_shortcut",
        blockType: shortcutType,
        preventDefault: true,
      };
    }
  }

  if (input.key === "Backspace" && !input.ctrlKey && !input.shiftKey) {
    if (input.selectionStart !== input.selectionEnd) return { type: "none" };
    if (input.selectionStart === 0 && notesBackspaceRemovesBlockFormat(input.blockType)) {
      return { type: "remove_block_format", preventDefault: true };
    }
    if (input.text.length === 0) {
      return input.isOnlyBlock
        ? { type: "convert_to_paragraph", preventDefault: true }
        : { type: "delete_block", preventDefault: true };
    }
    if (
      input.selectionStart === 0
      && input.selectionEnd === 0
      && input.previousBlockType
      && notesBackspaceCanMergeBlockTypes(input.blockType, input.previousBlockType)
    ) {
      return { type: "merge_with_previous", preventDefault: true };
    }
  }

  return { type: "none" };
}

/** Indent code lines as text, preserving a selection over the changed lines. */
function planCodeIndentation(input: NotesKeyboardPlanInput): NotesKeyboardAction {
  const start = Math.min(input.selectionStart, input.selectionEnd);
  const end = Math.max(input.selectionStart, input.selectionEnd);
  if (!input.shiftKey && start === end) {
    return { type: "replace_text", text: input.text.slice(0, start) + "\t" + input.text.slice(end),
      selection: { start: start + 1, end: start + 1 }, preventDefault: true };
  }
  const lineStart = start === 0 ? 0 : input.text.lastIndexOf("\n", start - 1) + 1;
  const selectedEnd = end > start && input.text[end - 1] === "\n" ? end - 1 : end;
  const nextBreak = input.text.indexOf("\n", selectedEnd);
  const lineEnd = nextBreak < 0 ? input.text.length : nextBreak;
  const lines = input.text.slice(lineStart, lineEnd).split("\n");
  const changes = lines.map((line) => input.shiftKey ? -(line.match(/^(?:\t| {1,4})/u)?.[0].length ?? 0) : 1);
  const text = lines.map((line, index) => input.shiftKey ? line.slice(-changes[index]) : "\t" + line).join("\n");
  const delta = changes.reduce((sum, count) => sum + count, 0);
  const from = Math.max(lineStart, start + changes[0]);
  return { type: "replace_text", text: input.text.slice(0, lineStart) + text + input.text.slice(lineEnd),
    selection: { start: from, end: start === end ? from : Math.max(from, end + delta) }, preventDefault: true };
}
