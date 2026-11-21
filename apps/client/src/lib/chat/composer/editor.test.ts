// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import type { ChatMessageReference } from "$lib/chat/contracts";
import { ChatComposerEditor } from "./editor";
import { chatComposerDocumentFromText } from "./rich-text";

const atlasReference: ChatMessageReference = {
  kind: "participant",
  metadata: {
    referenceId: "reference:atlas",
    labelSnapshot: "Atlas",
    startOffset: 4,
    endOffset: 10,
    plainTextProjection: "@Atlas",
  },
  participantId: "participant:atlas",
  participantKind: "ai_agent",
};

describe("ChatComposerEditor", () => {
  it("maps a browser selection to visible-text offsets", () => {
    const root = document.createElement("div");
    document.body.append(root);
    const editor = new ChatComposerEditor(root, chatComposerDocumentFromText("Review the calendar"), {
      onChange: vi.fn(),
      onSelectionChange: vi.fn(),
    });
    root.focus();
    setSelection(root, 11, 19);

    expect(document.getSelection()?.toString()).toBe("calendar");
    expect(editor.selection()).toEqual({ start: 11, end: 19 });
    root.remove();
  });

  it("renders a soft break as two sibling editor lines", () => {
    const root = document.createElement("div");
    document.body.append(root);
    const onChange = vi.fn();
    const editor = new ChatComposerEditor(root, chatComposerDocumentFromText("Example"), {
      onChange,
      onSelectionChange: vi.fn(),
    });
    root.focus();
    setSelection(root, 7, 7);
    editor.handleKeydown(new KeyboardEvent("keydown", {
      key: "Enter",
      shiftKey: true,
      cancelable: true,
    }));

    expect(root.querySelectorAll(":scope > [data-chat-composer-line]")).toHaveLength(2);
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ plainText: "Example\n" }));
    root.remove();
  });

  it("advances the caret against the new document after native input", () => {
    const root = document.createElement("div");
    document.body.append(root);
    const editor = new ChatComposerEditor(root, chatComposerDocumentFromText("Example"), {
      onChange: vi.fn(),
      onSelectionChange: vi.fn(),
    });
    root.focus();
    setSelection(root, 7, 7);
    editor.handleBeforeInput(new InputEvent("beforeinput", { data: "x", inputType: "insertText" }));
    const line = root.querySelector<HTMLElement>("[data-chat-composer-line]");
    if (!line) throw new Error("Editor line did not render");
    line.textContent = "Examplex";
    setSelection(root, 8, 8);
    editor.handleInput();

    expect(editor.plainText()).toBe("Examplex");
    expect(editor.selection()).toEqual({ start: 8, end: 8 });
    root.remove();
  });

  it("anchors the caret to the empty editor line after an external draft clear", () => {
    const root = document.createElement("div");
    root.tabIndex = 0;
    document.body.append(root);
    const onSelectionChange = vi.fn();
    const editor = new ChatComposerEditor(root, chatComposerDocumentFromText("Example"), {
      onChange: vi.fn(),
      onSelectionChange,
    });
    root.focus();
    setSelection(root, 7, 7);

    editor.setDocument(chatComposerDocumentFromText(""));

    const sentinel = root.querySelector<HTMLElement>("[data-chat-composer-sentinel]");
    expect(sentinel?.textContent).toBe("\u200b");
    expect(document.getSelection()?.anchorNode).toBe(sentinel?.firstChild);
    expect(editor.selection()).toEqual({ start: 0, end: 0 });
    expect(onSelectionChange).toHaveBeenLastCalledWith({ start: 0, end: 0 }, []);
    root.remove();
  });

  it("preserves whitespace-only native input", () => {
    const root = document.createElement("div");
    document.body.append(root);
    const editor = new ChatComposerEditor(root, chatComposerDocumentFromText(""), {
      onChange: vi.fn(),
      onSelectionChange: vi.fn(),
    });
    root.focus();
    const line = root.querySelector<HTMLElement>("[data-chat-composer-line]");
    if (!line) throw new Error("Editor line did not render");
    line.textContent = "   ";
    setSelection(root, 3, 3);
    editor.handleInput();

    expect(editor.plainText()).toBe("   ");
    expect(editor.selection()).toEqual({ start: 3, end: 3 });
    root.remove();
  });

  it("commits one undo step after IME composition", async () => {
    const root = document.createElement("div");
    document.body.append(root);
    const editor = new ChatComposerEditor(root, chatComposerDocumentFromText("abc"), {
      onChange: vi.fn(),
      onSelectionChange: vi.fn(),
    });
    root.focus();
    setSelection(root, 3, 3);
    editor.handleCompositionStart();
    const line = root.querySelector<HTMLElement>("[data-chat-composer-line]");
    if (!line) throw new Error("Editor line did not render");
    line.textContent = "abcñ";
    setSelection(root, 4, 4);
    editor.handleInput();
    editor.handleCompositionEnd();
    await Promise.resolve();
    editor.handleKeydown(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, cancelable: true }));

    expect(editor.plainText()).toBe("abc");
    root.remove();
  });

  it("renders references as noneditable atoms and deletes them atomically", () => {
    const root = document.createElement("div");
    document.body.append(root);
    const editor = new ChatComposerEditor(
      root,
      chatComposerDocumentFromText("Ask @Atlas now"),
      { onChange: vi.fn(), onSelectionChange: vi.fn() },
      [atlasReference],
    );
    root.focus();
    editor.focus({ start: 10, end: 10 });

    const atom = root.querySelector<HTMLElement>("[data-chat-reference-id='reference:atlas']");
    expect(atom?.contentEditable).toBe("false");
    expect(atom?.textContent).toBe("@Atlas");
    const deleted = editor.handleBeforeInput(new InputEvent("beforeinput", {
      inputType: "deleteContentBackward",
      cancelable: true,
    }));

    expect(deleted).toBe(true);
    expect(editor.plainText()).toBe("Ask  now");
    expect(editor.messageReferences()).toEqual([]);
    editor.handleKeydown(new KeyboardEvent("keydown", {
      key: "z",
      ctrlKey: true,
      cancelable: true,
    }));
    expect(editor.plainText()).toBe("Ask @Atlas now");
    expect(editor.messageReferences()).toEqual([atlasReference]);
    root.remove();
  });

  it("toggles inline marks from shortcuts on a selection and undoes them", () => {
    const { root, editor, onChange } = mountEditor("Ship it now");
    setSelection(root, 5, 7);

    editor.handleKeydown(shortcut("X", "KeyX", { shiftKey: true }));
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ markdown: "Ship ~~it~~ now" }));
    expect(root.querySelector("s")?.textContent).toBe("it");
    expect(editor.formatState().marks).toEqual(["strikethrough"]);
    editor.handleKeydown(shortcut("X", "KeyX", { shiftKey: true }));
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ markdown: "Ship it now" }));
    editor.handleKeydown(shortcut("C", "KeyC", { shiftKey: true }));
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ markdown: "Ship `it` now" }));
    editor.handleKeydown(shortcut("z", "KeyZ"));
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ markdown: "Ship it now" }));
    root.remove();
  });

  it("stores an inline mark for the next typed text when nothing is selected", () => {
    const { root, editor, onChange } = mountEditor("Ship ");
    setSelection(root, 5, 5);

    editor.handleKeydown(shortcut("C", "KeyC", { shiftKey: true }));
    expect(editor.formatState().marks).toEqual(["code"]);
    editor.handleBeforeInput(new InputEvent("beforeinput", { data: "x", inputType: "insertText", cancelable: true }));
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ markdown: "Ship `x`" }));
    root.remove();
  });

  it("applies line formats to the caret line without a selection and across selected lines", () => {
    const { root, editor, onChange } = mountEditor("one\ntwo");
    setSelection(root, 1, 1);

    editor.handleKeydown(shortcut("*", "Digit8", { shiftKey: true }));
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ markdown: "- one\ntwo" }));
    expect(editor.selection()).toEqual({ start: 3, end: 3 });
    expect(editor.formatState().lines.bulletList).toBe(true);
    editor.applyFormat({ kind: "line", format: "orderedList" });
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ markdown: "1. one\ntwo" }));
    editor.handleKeydown(shortcut("z", "KeyZ"));
    editor.handleKeydown(shortcut("z", "KeyZ"));
    expect(editor.plainText()).toBe("one\ntwo");
    root.remove();
  });

  it("drops inline marks from lines that become a code block", () => {
    const { root, editor, onChange } = mountEditor("bold");
    setSelection(root, 0, 4);
    editor.toggleMark("bold");

    editor.handleKeydown(shortcut("C", "KeyC", { shiftKey: true, altKey: true }));
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ markdown: "```\nbold\n```" }));
    root.remove();
  });

  it("keeps reference atoms whole when formatting a selection around them", () => {
    const { root, editor, onChange } = mountEditor("Ask @Atlas now", [atlasReference]);
    editor.focus({ start: 0, end: 14 });

    editor.applyFormat({ kind: "mark", mark: "bold" });
    expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ markdown: "**Ask @Atlas now**" }));
    expect(root.querySelector("strong [data-chat-reference-id='reference:atlas']")).not.toBeNull();
    editor.applyFormat({ kind: "line", format: "blockquote" });
    expect(editor.plainText()).toBe("> Ask @Atlas now");
    expect(editor.messageReferences()[0]?.metadata).toMatchObject({ startOffset: 6, endOffset: 12 });
    root.remove();
  });

  it("continues a list on a soft break", () => {
    const { root, editor } = mountEditor("- one");
    setSelection(root, 5, 5);

    editor.handleKeydown(new KeyboardEvent("keydown", { key: "Enter", shiftKey: true, cancelable: true }));
    expect(editor.plainText()).toBe("- one\n- ");
    expect(editor.selection()).toEqual({ start: 8, end: 8 });
    root.remove();
  });

  it("leaves Enter and Mod+Enter to the composer for queueing and steering", () => {
    const { root, editor } = mountEditor("reply");
    setSelection(root, 0, 5);

    expect(editor.handleKeydown(new KeyboardEvent("keydown", { key: "Enter", cancelable: true }))).toBe(false);
    expect(editor.handleKeydown(new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, cancelable: true }))).toBe(false);
    expect(editor.handleKeydown(new KeyboardEvent("keydown", { key: ".", ctrlKey: true, cancelable: true }))).toBe(false);
    expect(editor.plainText()).toBe("reply");
    root.remove();
  });

  it("ignores native underline formatting that Markdown cannot store", () => {
    const { root, editor, onChange } = mountEditor("text");
    setSelection(root, 0, 4);
    const event = new InputEvent("beforeinput", { inputType: "formatUnderline", cancelable: true });

    expect(editor.handleBeforeInput(event)).toBe(true);
    expect(event.defaultPrevented).toBe(true);
    expect(onChange).not.toHaveBeenCalled();
    root.remove();
  });
});

function mountEditor(text: string, references: ChatMessageReference[] = []) {
  const root = document.createElement("div");
  root.tabIndex = 0;
  document.body.append(root);
  const onChange = vi.fn();
  const editor = new ChatComposerEditor(
    root,
    chatComposerDocumentFromText(text),
    { onChange, onSelectionChange: vi.fn() },
    references,
  );
  root.focus();
  return { root, editor, onChange };
}

function shortcut(
  key: string,
  code: string,
  modifiers: Partial<Pick<KeyboardEventInit, "shiftKey" | "altKey">> = {},
): KeyboardEvent {
  return new KeyboardEvent("keydown", { key, code, ctrlKey: true, cancelable: true, ...modifiers });
}

function setSelection(root: HTMLDivElement, start: number, end: number): void {
  const text = root.querySelector("[data-chat-composer-line]")?.firstChild;
  if (!(text instanceof Text)) throw new Error("Editor text did not render");
  const range = document.createRange();
  range.setStart(text, start);
  range.setEnd(text, end);
  const selection = document.getSelection();
  if (!selection) throw new Error("Document selection is unavailable");
  selection.removeAllRanges();
  selection.addRange(range);
}
