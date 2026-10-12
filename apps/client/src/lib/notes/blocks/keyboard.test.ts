import { describe, expect, it } from "vitest";
import {
  planNotesKeyboardAction,
  type NotesKeyboardPlanInput,
} from "./keyboard";
import type { NotesBlockType } from "$lib/notes/types";

function plan(input: Partial<NotesKeyboardPlanInput>) {
  return planNotesKeyboardAction({
    key: "Enter",
    shiftKey: false,
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    text: "",
    selectionStart: 0,
    selectionEnd: 0,
    blockType: "paragraph",
    previousBlockType: null,
    isOnlyBlock: false,
    ...input,
  });
}

describe("notes keyboard planning", () => {
  it("reduces empty indentation before exiting the list, then removes the marker at the margin", () => {
    expect(plan({ key: "Enter", blockType: "numbered_list_item", indentationDepth: 2 })).toMatchObject({ type: "outdent" });
    expect(plan({ key: "Backspace", blockType: "numbered_list_item", text: "Keep", indentationDepth: 2 })).toMatchObject({ type: "outdent" });
    expect(plan({ key: "Backspace", blockType: "numbered_list_item", text: "Keep", indentationDepth: 0 })).toMatchObject({ type: "remove_block_format" });
    expect(plan({ key: "Backspace", blockType: "paragraph", text: "Keep", indentationDepth: 2, selectionStart: 1, selectionEnd: 2 })).toEqual({ type: "none" });
  });

  it("indents selected code lines and reverses both tabs and space indentation", () => {
    expect(plan({ key: "Tab", blockType: "code", text: "a\nb\nc", selectionStart: 0, selectionEnd: 4 })).toMatchObject({ type: "replace_text", text: "\ta\n\tb\nc", selection: { start: 1, end: 6 } });
    expect(plan({ key: "Tab", shiftKey: true, blockType: "code", text: "\ta\n    b", selectionStart: 0, selectionEnd: 8 })).toMatchObject({ type: "replace_text", text: "a\nb", selection: { start: 0, end: 3 } });
    expect(plan({ key: "Tab", shiftKey: true, blockType: "code", text: "\nx", selectionStart: 0, selectionEnd: 0 })).toMatchObject({ type: "replace_text", text: "\nx", selection: { start: 0, end: 0 } });
  });

  it("splits rich text blocks on Enter", () => {
    expect(plan({ key: "Enter", text: "Hello" })).toEqual({
      type: "split_text_block",
      selectionStart: 0,
      selectionEnd: 0,
      text: "Hello",
      preventDefault: true,
    });
    expect(
      plan({
        key: "Enter",
        text: "Hello",
        selectionStart: 2,
        selectionEnd: 4,
        blockType: "heading_2",
      }),
    ).toEqual({
      type: "split_text_block",
      selectionStart: 2,
      selectionEnd: 4,
      text: "Hello",
      preventDefault: true,
    });
  });

  it("enters a toggle body from its title, including an empty nested toggle", () => {
    expect(plan({ blockType: "toggle", text: "Details", selectionStart: 7, selectionEnd: 7 }))
      .toMatchObject({ type: "split_text_block" });
    expect(plan({ blockType: "toggle", indentationDepth: 2 }))
      .toMatchObject({ type: "split_text_block" });
    expect(plan({ blockType: "paragraph", indentationDepth: 1 }))
      .toMatchObject({ type: "outdent" });
    expect(plan({ key: "Backspace", blockType: "paragraph", indentationDepth: 1 }))
      .toMatchObject({ type: "outdent" });
    expect(plan({ blockType: "toggle", ctrlKey: true }))
      .toMatchObject({ type: "toggle_block_open" });
    expect(plan({ blockType: "toggle", metaKey: true }))
      .toMatchObject({ type: "toggle_block_open" });
  });

  it("allows newline insertion for Shift+Enter and code Enter", () => {
    expect(plan({ key: "Enter", shiftKey: true })).toEqual({
      type: "insert_newline",
      preventDefault: true,
    });
    expect(plan({ key: "Enter", blockType: "code", text: "let x = 1;" })).toEqual({
      type: "insert_newline",
      preventDefault: true,
    });
  });

  it("creates a paragraph after code on Ctrl+Enter", () => {
    expect(plan({ key: "Enter", blockType: "code", ctrlKey: true })).toEqual({
      type: "create_sibling",
      preventDefault: true,
    });
  });

  it("converts empty list items while opening callout bodies and exiting empty child rows", () => {
    expect(plan({ key: "Enter", blockType: "bulleted_list_item", text: "" })).toEqual({
      type: "convert_to_paragraph",
      preventDefault: true,
    });
    expect(plan({ key: "Enter", blockType: "callout", text: "" })).toEqual({
      type: "split_text_block",
      selectionStart: 0,
      selectionEnd: 0,
      text: "",
      preventDefault: true,
    });
    expect(plan({ key: "Enter", blockType: "paragraph", indentationDepth: 1 })).toMatchObject({ type: "outdent" });
  });

  it("plans Backspace deletion, only-block recovery, and merge", () => {
    expect(plan({ key: "Backspace", text: "" })).toEqual({
      type: "delete_block",
      preventDefault: true,
    });
    expect(plan({ key: "Backspace", text: "", isOnlyBlock: true })).toEqual({
      type: "convert_to_paragraph",
      preventDefault: true,
    });
    expect(
      plan({
        key: "Backspace",
        text: "Text",
        selectionStart: 0,
        selectionEnd: 0,
        previousBlockType: "paragraph",
      }),
    ).toEqual({
      type: "merge_with_previous",
      preventDefault: true,
    });
  });

  it.each<NotesBlockType>([
    "heading_1", "heading_2", "heading_3", "heading_4", "bulleted_list_item",
    "numbered_list_item", "to_do", "toggle", "callout", "quote",
  ])("removes %s formatting at the start before deleting or merging text", (blockType) => {
    for (const text of ["", "Text"]) {
      for (const previousBlockType of [null, "paragraph"] as const) {
        expect(plan({ key: "Backspace", blockType, text, previousBlockType })).toEqual({
          type: "remove_block_format", preventDefault: true,
        });
      }
    }
    expect(plan({ key: "Backspace", blockType, text: "Text", selectionStart: 1, selectionEnd: 1 }))
      .toEqual({ type: "none" });
    expect(plan({ key: "Backspace", blockType, text: "Text", selectionEnd: 3 }))
      .toEqual({ type: "none" });
    expect(plan({ key: "Backspace", blockType, text: "Text", ctrlKey: true }))
      .toEqual({ type: "none" });
    expect(plan({ key: "Delete", blockType, text: "Text" })).toEqual({ type: "none" });
  });

  it.each<NotesBlockType>(["paragraph", "code", "template", "button"])(
    "retains deletion and merging for %s without an ordinary text prefix", (blockType) => {
      expect(plan({ key: "Backspace", blockType })).toEqual({ type: "delete_block", preventDefault: true });
      expect(plan({ key: "Backspace", blockType, text: "Text", previousBlockType: "paragraph" }))
        .toEqual({ type: "merge_with_previous", preventDefault: true });
    },
  );

  it("plans Tab nesting and Shift+Tab outdent", () => {
    expect(plan({ key: "Tab" })).toEqual({ type: "nest", preventDefault: true });
    expect(plan({ key: "Tab", shiftKey: true })).toEqual({
      type: "outdent",
      preventDefault: true,
    });
  });

  it("recognizes physical Tab when the logical key is unidentified without overriding remapped keys", () => {
    for (const key of ["Unidentified", ""]) {
      expect(plan({ key, code: "Tab" })).toEqual({ type: "nest", preventDefault: true });
      expect(plan({ key, code: "Tab", shiftKey: true })).toEqual({ type: "outdent", preventDefault: true });
    }
    expect(plan({ key: "Unidentified", code: "KeyA", shiftKey: true })).toEqual({ type: "none" });
    expect(plan({ key: "x", code: "Tab", shiftKey: true })).toEqual({ type: "none" });
    for (const modifier of ["altKey", "ctrlKey", "metaKey"] as const) {
      expect(plan({ key: "Unidentified", code: "Tab", shiftKey: true, [modifier]: true })).toEqual({ type: "none" });
    }
  });

  it("plans primary modifier movement shortcuts", () => {
    expect(plan({ key: "ArrowUp", ctrlKey: true, shiftKey: true })).toEqual({
      type: "move_up",
      preventDefault: true,
    });
    expect(plan({ key: "ArrowDown", metaKey: true, shiftKey: true })).toEqual({
      type: "move_down",
      preventDefault: true,
    });
  });

  it("accepts slash characters produced with Shift and leaves code input literal", () => {
    expect(plan({ key: "/", shiftKey: true })).toEqual({ type: "open_slash_menu", preventDefault: false });
    expect(plan({ key: "/", blockType: "code" })).toEqual({ type: "none" });
  });

  it("opens slash commands only at the start of an empty block", () => {
    expect(plan({ key: "/" })).toEqual({
      type: "open_slash_menu",
      preventDefault: false,
    });
    expect(plan({ key: "/", text: "x", selectionStart: 1, selectionEnd: 1 })).toEqual({
      type: "none",
    });
  });

  it("applies text shortcuts on Space or Enter", () => {
    expect(plan({ key: " ", text: "##" })).toEqual({
      type: "apply_text_shortcut",
      blockType: "heading_2",
      preventDefault: true,
    });
    expect(plan({ key: " ", text: "####" })).toEqual({
      type: "apply_text_shortcut",
      blockType: "heading_4",
      preventDefault: true,
    });
    expect(plan({ key: " ", text: ">" })).toEqual({
      type: "apply_text_shortcut",
      blockType: "toggle",
      preventDefault: true,
    });
    expect(plan({ key: " ", text: "\"" })).toEqual({
      type: "apply_text_shortcut",
      blockType: "quote",
      preventDefault: true,
    });
    expect(plan({ key: "Enter", text: "```" })).toEqual({
      type: "apply_text_shortcut",
      blockType: "code",
      preventDefault: true,
    });
  });

  it("modifies toggle open state on Ctrl+Enter", () => {
    expect(plan({ key: "Enter", ctrlKey: true, blockType: "toggle" })).toEqual({
      type: "toggle_block_open",
      preventDefault: true,
    });
  });
});
