// @vitest-environment jsdom

import { defaultRichTextAnnotations } from "$lib/notes/rich-text/core";
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import NotesTextContextMenu from "./NotesTextContextMenu.svelte";

describe("Notes text context menu", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
  });

  function renderMenu(hasSelection: boolean, focusOnOpen = false) {
    target = document.createElement("div");
    document.body.append(target);
    const onToggleAnnotation = vi.fn();
    const onConvert = vi.fn();
    const onCreateComment = vi.fn();
    const onCreateSuggestion = vi.fn();
    const onClose = vi.fn();
    component = mount(NotesTextContextMenu, {
      target,
      props: {
        position: { x: 40, y: 40 },
        focusOnOpen,
        annotations: defaultRichTextAnnotations(),
        blockType: "paragraph",
        hasSelection,
        canFormatSelection: hasSelection,
        canOpenLink: hasSelection,
        onToggleAnnotation,
        onColorSelect: vi.fn(),
        onCreateEquation: vi.fn(),
        onCreateComment,
        onCreateSuggestion,
        onOpenLink: vi.fn(),
        onCopyBlockLink: vi.fn(async () => undefined),
        onConvert,
        onInsert: vi.fn(),
        onCut: vi.fn(async () => undefined),
        onCopy: vi.fn(async () => undefined),
        onPaste: vi.fn(async () => undefined),
        onPastePlainText: vi.fn(async () => undefined),
        onClose,
      },
    });
    return { onToggleAnnotation, onConvert, onCreateComment, onCreateSuggestion, onClose };
  }

  function button(label: string): HTMLButtonElement {
    const found = Array.from(target?.querySelectorAll<HTMLButtonElement>("button") ?? [])
      .find((candidate) => candidate.textContent?.trim() === label);
    if (!found) throw new Error(`Missing context menu button: ${label}`);
    return found;
  }

  it("keeps block conversion available at a caret while selection actions stay disabled", async () => {
    const { onConvert, onClose } = renderMenu(false);
    await tick();
    expect(button("Format").disabled).toBe(true);
    expect(button("Comment").disabled).toBe(true);
    expect(button("Suggest edit").disabled).toBe(true);
    expect(button("Cut").disabled).toBe(true);
    expect(button("Copy").disabled).toBe(true);

    button("Paragraph").click();
    await tick();
    button("Heading 1").click();
    await Promise.resolve();
    expect(onConvert).toHaveBeenCalledWith("heading_1");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("offers formatting for selected text", async () => {
    const { onToggleAnnotation, onClose } = renderMenu(true);
    await tick();
    button("Format").click();
    await tick();
    button("Bold").click();
    await Promise.resolve();
    expect(onToggleAnnotation).toHaveBeenCalledWith("bold");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("offers comment and suggestion in the main menu", async () => {
    const { onCreateComment, onCreateSuggestion } = renderMenu(true);
    await tick();
    expect(target?.querySelector('[role="menu"]')?.contains(button("Comment"))).toBe(true);
    button("Comment").click();
    await Promise.resolve();
    expect(onCreateComment).toHaveBeenCalledOnce();
    expect(onCreateSuggestion).not.toHaveBeenCalled();
  });

  it("does not move focus from selected text when opened by pointer", async () => {
    const editor = document.createElement("div");
    editor.contentEditable = "true";
    editor.tabIndex = 0;
    editor.textContent = "Selected text";
    document.body.append(editor);
    editor.focus();
    const selection = document.getSelection();
    const range = document.createRange();
    range.selectNodeContents(editor);
    selection?.removeAllRanges();
    selection?.addRange(range);

    try {
      renderMenu(true);
      await tick();
      expect(document.activeElement).toBe(editor);
      expect(selection?.toString()).toBe("Selected text");

      button("Format").dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
      await tick();
      expect(document.activeElement).toBe(editor);
      expect(selection?.toString()).toBe("Selected text");
    } finally {
      selection?.removeAllRanges();
      editor.remove();
    }
  });

  it("focuses the menu when opened from the keyboard", async () => {
    renderMenu(true, true);
    await tick();
    await tick();
    expect(document.activeElement).toBe(button("Edit link"));
  });
});
