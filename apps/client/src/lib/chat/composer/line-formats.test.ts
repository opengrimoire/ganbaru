import { describe, expect, it } from "vitest";
import {
  chatComposerLineBreakEdit,
  chatComposerLineFormatState,
  mapChatComposerOffset,
  toggleChatComposerLineFormat,
  type ChatComposerLineFormat,
  type ChatComposerTextEdit,
} from "./line-formats";

function applyEdits(text: string, edits: readonly ChatComposerTextEdit[]): string {
  return [...edits].reverse().reduce((value, edit) => value.slice(0, edit.start) + edit.text + value.slice(edit.end), text);
}

function toggle(text: string, start: number, end: number, format: ChatComposerLineFormat) {
  const result = toggleChatComposerLineFormat(text, { start, end }, format);
  return { ...result, text: applyEdits(text, result.edits) };
}

describe("Chat composer line formats", () => {
  it("bullets the caret line and toggles it back off", () => {
    const on = toggle("Buy milk", 3, 3, "bulletList");

    expect(on.text).toBe("- Buy milk");
    expect(on.selection).toEqual({ start: 5, end: 5 });
    expect(chatComposerLineFormatState(on.text, on.selection).bulletList).toBe(true);
    const off = toggle(on.text, on.selection.start, on.selection.end, "bulletList");
    expect(off.text).toBe("Buy milk");
    expect(off.selection).toEqual({ start: 3, end: 3 });
  });

  it("numbers every non-blank touched line from one and skips blank lines", () => {
    const text = "first\n\nsecond\nthird";
    const result = toggle(text, 2, 15, "orderedList");

    expect(result.text).toBe("1. first\n\n2. second\n3. third");
    expect(chatComposerLineFormatState(result.text, { start: 0, end: result.text.length }).orderedList).toBe(true);
  });

  it("replaces one list kind with the other instead of nesting", () => {
    expect(toggle("1. a\n2. b", 0, 9, "bulletList").text).toBe("- a\n- b");
    expect(toggle("- a\nplain", 0, 9, "bulletList").text).toBe("- a\n- plain");
  });

  it("leaves a line alone when the selection ends at its start", () => {
    expect(toggle("one\ntwo", 0, 4, "blockquote").text).toBe("> one\ntwo");
  });

  it("quotes lines including blank ones and keeps list markers inside the quote", () => {
    const quoted = toggle("- a\n\nb", 0, 6, "blockquote");

    expect(quoted.text).toBe("> - a\n> \n> b");
    expect(chatComposerLineFormatState(quoted.text, { start: 0, end: quoted.text.length })).toMatchObject({
      blockquote: true,
      bulletList: false,
    });
    expect(toggle(quoted.text, 0, quoted.text.length, "blockquote").text).toBe("- a\n\nb");
    expect(toggle(quoted.text, 2, 2, "bulletList").text).toBe("> a\n> \n> b");
  });

  it("wraps touched lines in a code fence and removes it again", () => {
    const wrapped = toggle("say\nrun this\nthat", 5, 7, "codeBlock");

    expect(wrapped.text).toBe("say\n```\nrun this\n```\nthat");
    expect(wrapped.selection).toEqual({ start: 9, end: 11 });
    expect(wrapped.codeRange).toEqual({ start: 8, end: 16 });
    expect(chatComposerLineFormatState(wrapped.text, wrapped.selection).codeBlock).toBe(true);
    const unwrapped = toggle(wrapped.text, wrapped.selection.start, wrapped.selection.end, "codeBlock");
    expect(unwrapped.text).toBe("say\nrun this\nthat");
    expect(unwrapped.selection).toEqual({ start: 5, end: 7 });
  });

  it("opens an empty code block with the caret inside", () => {
    const result = toggle("", 0, 0, "codeBlock");

    expect(result.text).toBe("```\n\n```");
    expect(result.selection).toEqual({ start: 4, end: 4 });
  });

  it("removes an unterminated fence, which Markdown reads as a block to the end", () => {
    expect(toggle("```js\ncode", 7, 7, "codeBlock").text).toBe("code");
  });

  it("does not add list or quote markers inside a code block", () => {
    const text = "```\ncode\n```";

    expect(toggle(text, 5, 5, "bulletList").edits).toEqual([]);
    expect(toggle(text, 5, 5, "blockquote").edits).toEqual([]);
  });

  it("continues list and quote markers on a soft break and ends them on an empty item", () => {
    expect(chatComposerLineBreakEdit("- a", 3)).toEqual({ start: 3, end: 3, text: "\n- " });
    expect(chatComposerLineBreakEdit("> 9) a", 6)).toEqual({ start: 6, end: 6, text: "\n> 10) " });
    expect(chatComposerLineBreakEdit("a\n- ", 4)).toEqual({ start: 2, end: 4, text: "" });
    expect(chatComposerLineBreakEdit("plain", 5)).toBeNull();
    expect(chatComposerLineBreakEdit("- a", 1)).toBeNull();
    expect(chatComposerLineBreakEdit("```\n- a\n```", 7)).toBeNull();
  });

  it("maps offsets through insertions, replacements, and deletions", () => {
    const edits: ChatComposerTextEdit[] = [
      { start: 0, end: 0, text: "> " },
      { start: 4, end: 7, text: "" },
      { start: 9, end: 9, text: "!", keepBefore: true },
    ];

    expect(mapChatComposerOffset(0, edits)).toBe(2);
    expect(mapChatComposerOffset(5, edits)).toBe(6);
    expect(mapChatComposerOffset(8, edits)).toBe(7);
    expect(mapChatComposerOffset(9, edits)).toBe(8);
    expect(mapChatComposerOffset(10, edits)).toBe(10);
  });
});
