// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import { attachNotesBlockSelectionDelegates, createNotesBlockSelectionController } from "./selection-controller.svelte";

describe("Notes block selection controller", () => {
  it("removes delegated listeners when the list action is destroyed", () => {
    const node = document.createElement("div");
    const pointerDown = vi.fn();
    const pointerOver = vi.fn();
    const keydown = vi.fn();
    const action = attachNotesBlockSelectionDelegates(node, { pointerDown, pointerOver, keydown });

    node.dispatchEvent(new Event("pointerdown"));
    node.dispatchEvent(new Event("pointerover"));
    node.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect([pointerDown, pointerOver, keydown].map((handler) => handler.mock.calls.length))
      .toEqual([1, 1, 1]);

    action.destroy();
    node.dispatchEvent(new Event("pointerdown"));
    node.dispatchEvent(new Event("pointerover"));
    node.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect([pointerDown, pointerOver, keydown].map((handler) => handler.mock.calls.length))
      .toEqual([1, 1, 1]);
  });
});

/** Builds a rendered list with contenteditable blocks and selectable margins. */
function selectionHarness() {
  const list = document.createElement("div");
  list.innerHTML = ["first", "second", "third"].map((id) => `<div data-notes-selectable-block-id="${id}" data-notes-block-selection-zone><div contenteditable="true">${id}</div></div>`).join("");
  const row = (id: string) => list.querySelector<HTMLElement>(`[data-notes-selectable-block-id="${id}"]`)!;
  const undo = vi.fn(async () => true);
  const focusRow = vi.fn();
  const pasteBlocks = vi.fn(async () => null);
  const focusTextEditorAtEnd = vi.fn(() => true);
  const controller = createNotesBlockSelectionController({
    hydrateSubtrees: async (ids) => ids,
    undo, redo: vi.fn(async () => true), readPageId: () => "page", readListElement: () => list,
    readRenderedBlockIds: () => ["first", "second", "third"],
    readTreeState: () => ({ blocksById: {}, childIdsByParentId: {} }),
    blockIdFromEvent: (event) => event.target instanceof Element ? event.target.closest<HTMLElement>("[data-notes-selectable-block-id]")?.dataset.notesSelectableBlockId ?? null : null,
    targetIsEditable: (target) => target instanceof Element && target.closest("[contenteditable]") !== null,
    targetIsSelectionZone: () => true, focusTextEditorAtEnd,
    focusRow, handleNavigationKeydown: () => false,
    pasteBlocks, duplicateBlocks: async () => null,
    moveBlocks: async () => undefined, deleteBlocks: async () => undefined,
  });
  const delegates = controller.delegation(list);
  const pointer = (target: HTMLElement, type: string, shiftKey = false) => {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, shiftKey });
    Object.defineProperty(event, "pointerId", { value: 1 });
    target.dispatchEvent(event);
  };
  return { controller, row, pointer, undo, pasteBlocks, focusRow, focusTextEditorAtEnd, destroy: delegates.destroy };
}

describe("Notes block selection gestures", () => {
  it("selects a margin range and supports select all and undo from a focused row", () => {
    const h = selectionHarness();
    h.pointer(h.row("first"), "pointerdown");
    h.pointer(h.row("third"), "pointerdown", true);
    expect(h.controller.selection?.selectedBlockIds).toEqual(["first", "second", "third"]);
    h.row("third").dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "z", ctrlKey: true }));
    expect(h.undo).toHaveBeenCalledOnce();
    expect(h.controller.selection).toBeNull();
    h.row("first").dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "a", ctrlKey: true }));
    expect(h.controller.selection?.selectedBlockIds).toEqual(["first", "second", "third"]);
    h.destroy();
  });

  it("leaves Escape in text to the editor without clearing its range or selecting its row", async () => {
    const h = selectionHarness();
    const editor = h.row("first").firstElementChild as HTMLElement;
    document.body.append(h.row("first").parentElement!);
    const range = document.createRange();
    range.selectNodeContents(editor);
    window.getSelection()?.removeAllRanges();
    window.getSelection()?.addRange(range);
    try {
      const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "Escape" });
      editor.dispatchEvent(event);
      await Promise.resolve();
      expect(event.defaultPrevented).toBe(false);
      expect(h.controller.selection).toBeNull();
      expect(h.row("first").hasAttribute("data-notes-block-selected")).toBe(false);
      expect(h.focusRow).not.toHaveBeenCalled();
      expect(window.getSelection()?.toString()).toBe("first");
    } finally {
      window.getSelection()?.removeAllRanges();
      h.row("first").parentElement?.remove();
      h.destroy();
    }
  });

  it("uses Escape to leave an intentional block selection and return to text editing", async () => {
    const h = selectionHarness();
    h.pointer(h.row("first"), "pointerdown");
    await Promise.resolve();
    expect(h.row("first").hasAttribute("data-notes-block-selected")).toBe(true);
    h.row("first").dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "Escape" }));
    await Promise.resolve();
    expect(h.controller.selection).toBeNull();
    expect(h.row("first").hasAttribute("data-notes-block-selected")).toBe(false);
    expect(h.focusTextEditorAtEnd).toHaveBeenCalledWith("first");
    h.destroy();
  });

  it("leaves text drags to the document selection controller", () => {
    const h = selectionHarness();
    h.pointer(h.row("first").firstElementChild as HTMLElement, "pointerdown");
    h.pointer(h.row("first"), "pointerover");
    expect(h.controller.selection).toBeNull();
    h.pointer(h.row("second"), "pointerover");
    expect(h.controller.selection).toBeNull();
    h.destroy();
  });
});


describe("system clipboard ownership", () => {
  it("does not paste stale internal blocks after another app replaces the clipboard", async () => {
    const original = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: {
      readText: async () => "External content",
    } });
    const h = selectionHarness();
    try {
      h.controller.setClipboard({ mode: "copy", pageId: "page", rootBlockIds: ["first"],
        subtreeBlockIds: ["first"], plainText: "First" });
      await h.controller.paste("second");
      expect(h.pasteBlocks).not.toHaveBeenCalled();
      expect(h.controller.clipboard).toBeNull();
      expect(h.focusTextEditorAtEnd).toHaveBeenCalledWith("second");
    } finally {
      if (original) Object.defineProperty(navigator, "clipboard", original);
      else Reflect.deleteProperty(navigator, "clipboard");
      h.destroy();
    }
  });
});
