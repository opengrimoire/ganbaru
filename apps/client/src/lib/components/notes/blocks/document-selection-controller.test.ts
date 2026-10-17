// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { tick } from "svelte";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import type { NotesBlock } from "$lib/notes/types";
import { createNotesDocumentSelectionController } from "./document-selection-controller.svelte";
import { createNotesBlockSelectionController } from "./selection-controller.svelte";
import { createNotesBlockNavigationController } from "./navigation-controller";

/** Mount real selection listeners on independent editing hosts. */
function harness(count = 3, type: NotesBlock["type"] = "paragraph", hiddenCalloutIndices: readonly number[] = []) {
  const ids = Array.from({ length: count }, (_, index) => `block-${index}`);
  const hiddenCalloutIds = new Set(hiddenCalloutIndices.map((index) => ids[index]));
  const blocks = new Map(ids.map((id) => [id, {
    ...createBlockWrite(id, type, id), object: "block", parent: { type: "page_id", page_id: "page" },
    created_time: "", last_edited_time: "", has_children: false, in_trash: false,
    source_provider: null, source_object_id: null, source_last_edited_time: null,
  } as NotesBlock]));
  const list = document.createElement("div");
  list.innerHTML = ids.map((id) => `<div data-notes-selectable-block-id="${id}"><div role="textbox" contenteditable="true" data-notes-block-id="${id}">${id}</div></div>`).join("");
  document.body.append(list);
  const replace = vi.fn(async (): Promise<void | boolean> => undefined);
  const deleteBlocks = vi.fn(async (): Promise<void | boolean> => undefined);
  const hydrate = vi.fn(async () => undefined);
  const outlineSubtreeIds = vi.fn((rootBlockIds: readonly string[]) => [...rootBlockIds]);
  const focus = vi.fn();
  const indent = vi.fn(async () => undefined);
  const link = vi.fn(async () => undefined);
  const navigation = createNotesBlockNavigationController({
    readListElement: () => list, readRenderedBlockIds: () => ids,
    readBlock: (id) => blocks.get(id), isHiddenCalloutLabel: (id) => hiddenCalloutIds.has(id), requestFocus: focus,
  });
  const focusRow = vi.fn();
  const hydrateSubtrees = vi.fn(async (ids: readonly string[]) => ids);
  const blockSelection = createNotesBlockSelectionController({
    hydrateSubtrees,
    undo: vi.fn(async () => true), redo: vi.fn(async () => true),
    readPageId: () => "page", readListElement: () => list, readRenderedBlockIds: () => ids,
    readTreeState: () => ({ blocksById: Object.fromEntries(blocks), childIdsByParentId: { page: ids } }),
    blockIdFromEvent: navigation.blockIdFromEvent, targetIsEditable: navigation.targetIsEditable,
    targetIsSelectionZone: navigation.targetIsSelectionZone, focusTextEditorAtEnd: navigation.focusTextEditorAtEnd,
    focusRow, handleNavigationKeydown: navigation.handleKeydown,
    pasteBlocks: async () => null, duplicateBlocks: async () => null,
    moveBlocks: async () => undefined, deleteBlocks,
  });
  const blockDelegates = blockSelection.delegation(list);
  const controller = createNotesDocumentSelectionController({
    readIds: () => ids, readPageId: () => "page", readBlock: (id) => blocks.get(id),
    isHiddenCalloutLabel: (id) => hiddenCalloutIds.has(id),
    hydrate, outlineSubtreeIds, replace, indent, link, format: vi.fn(async () => undefined), focus, clearBlockSelection: () => blockSelection.setSelection(null),
    undo: vi.fn(async () => true), redo: vi.fn(async () => true),
  });
  const attached = controller.delegation(list);
  const editor = (index: number) => list.querySelector<HTMLElement>(`[data-notes-block-id="${ids[index]}"]`)!;
  const key = (index: number, key: string, options: KeyboardEventInit = {}) => {
    const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...options });
    editor(index).dispatchEvent(event);
    return event;
  };
  return { controller, blockSelection, hydrateSubtrees, outlineSubtreeIds, focusRow, replace, deleteBlocks, indent, link, hydrate, focus, blocks, ids, editor, key,
    destroy() { attached.destroy(); blockDelegates.destroy(); },
  };
}

/** Replace a text editing host with the atomic note surface used by the component. */
function noteRow(h: ReturnType<typeof harness>, index: number): HTMLButtonElement {
  const id = h.ids[index];
  const block = h.blocks.get(id)!;
  h.blocks.set(id, { ...block, type: "child_page", child_page: { title: "Nested note" } });
  const button = document.createElement("button");
  button.dataset.notesAtomicBlock = id;
  button.textContent = "Nested note";
  h.editor(index).replaceWith(button);
  return button;
}

describe("document ranges containing note rows", () => {
  it("selects across a single note button without opening it on pointer release", async () => {
    const h = harness(1);
    const button = noteRow(h, 0);
    vi.spyOn(button, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 100, 20));
    button.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0, clientX: 1, clientY: 10 }));
    button.dispatchEvent(new MouseEvent("pointermove", { bubbles: true, buttons: 1, clientX: 99, clientY: 10 }));
    await tick();
    expect(h.controller.selection).toEqual({ anchor: { blockId: h.ids[0], offset: 0 }, focus: { blockId: h.ids[0], offset: 1 } });
    const click = new MouseEvent("click", { bubbles: true, cancelable: true });
    button.dispatchEvent(click);
    expect(click.defaultPrevented).toBe(true);
    h.destroy();
  });

  it("extends to a note at the end of a document and copies its local reference", async () => {
    const h = harness(2);
    noteRow(h, 1);
    const root = h.editor(0);
    document.getSelection()?.collapse(root.firstChild, 2);
    h.key(0, "ArrowDown", { shiftKey: true });
    await tick();
    expect(h.controller.selection?.focus).toEqual({ blockId: h.ids[1], offset: 1 });
    const setData = vi.fn();
    const copy = new Event("copy", { bubbles: true, cancelable: true });
    Object.defineProperty(copy, "clipboardData", { value: { setData } });
    root.dispatchEvent(copy);
    expect(setData).toHaveBeenCalledWith("text/plain", `ock-0\n\n[Nested note](#notes?page=${h.ids[1]})`);
    h.destroy();
  });

  it("selects a single note with Shift+Right and pastes beside an unselected note", async () => {
    const h = harness(1);
    const button = noteRow(h, 0);
    button.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", shiftKey: true, bubbles: true, cancelable: true }));
    await tick();
    expect(h.controller.selection).toEqual({ anchor: { blockId: h.ids[0], offset: 0 }, focus: { blockId: h.ids[0], offset: 1 } });
    h.controller.clear();
    const paste = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(paste, "clipboardData", { value: { getData: (type: string) => type === "text/plain" ? "After" : "" } });
    button.dispatchEvent(paste);
    await tick();
    expect(h.replace).toHaveBeenCalledWith([h.ids[0]], 1, 1, "After", undefined);
    h.destroy();
  });

  it("includes a note when pointer selection starts on its button", async () => {
    const h = harness(2);
    const button = noteRow(h, 0);
    vi.spyOn(button, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 100, 20));
    button.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0, clientX: 1, clientY: 10 }));
    h.editor(1).dispatchEvent(new MouseEvent("pointermove", { bubbles: true, buttons: 1, clientX: 100, clientY: 100 }));
    await tick();
    expect(h.controller.selection?.anchor).toEqual({ blockId: h.ids[0], offset: 0 });
    expect(h.controller.selection?.focus.blockId).toBe(h.ids[1]);
    h.destroy();
  });
});

/** Model wrapped browser line rectangles and caret hit-testing in jsdom. */
function mockVisualLines(
  editors: readonly HTMLElement[],
  lineStarts: readonly (readonly number[])[],
): () => void {
  const characterWidth = 10;
  const lineHeight = 20;
  const left = 20;
  const top = (index: number) => 100 + index * 100;
  const lineForOffset = (index: number, offset: number) => {
    const starts = lineStarts[index] ?? [0];
    let line = 0;
    while (line + 1 < starts.length && offset >= starts[line + 1]) line += 1;
    return line;
  };
  const lineRect = (index: number, line: number) => {
    const starts = lineStarts[index] ?? [0];
    const end = starts[line + 1] ?? (editors[index].textContent?.length ?? 0);
    return new DOMRect(left, top(index) + line * lineHeight, (end - starts[line]) * characterWidth, lineHeight);
  };
  const caretRect = (index: number, offset: number) => {
    const line = lineForOffset(index, offset);
    const start = lineStarts[index]?.[line] ?? 0;
    return new DOMRect(left + (offset - start) * characterWidth, top(index) + line * lineHeight, 0, lineHeight);
  };
  const originalBounding = Object.getOwnPropertyDescriptor(Range.prototype, "getBoundingClientRect");
  const originalClient = Object.getOwnPropertyDescriptor(Range.prototype, "getClientRects");
  Object.defineProperty(Range.prototype, "getBoundingClientRect", {
    configurable: true,
    value: function (this: Range) {
      const index = editors.findIndex((editor) => editor.contains(this.startContainer));
      return index < 0 ? new DOMRect() : caretRect(index, this.startOffset);
    },
  });
  Object.defineProperty(Range.prototype, "getClientRects", {
    configurable: true,
    value: function (this: Range) {
      const index = editors.findIndex((editor) => editor.contains(this.startContainer));
      if (index < 0) return [];
      const fullText = this.startContainer.nodeType === Node.TEXT_NODE
        && this.startOffset === 0 && this.endOffset === (this.startContainer.textContent?.length ?? 0);
      return fullText
        ? (lineStarts[index] ?? [0]).map((_, line) => lineRect(index, line))
        : [caretRect(index, this.startOffset)];
    },
  });
  const rectSpies = editors.map((editor, index) => vi.spyOn(editor, "getBoundingClientRect")
    .mockImplementation(() => new DOMRect(left, top(index), 200, (lineStarts[index]?.length ?? 1) * lineHeight)));
  Object.defineProperty(document, "caretPositionFromPoint", {
    configurable: true,
    value: (x: number, y: number) => {
      const index = editors.findIndex((_, candidate) => y >= top(candidate)
        && y < top(candidate) + (lineStarts[candidate]?.length ?? 1) * lineHeight);
      if (index < 0) return null;
      const starts = lineStarts[index] ?? [0];
      const line = Math.floor((y - top(index)) / lineHeight);
      const start = starts[line];
      const end = starts[line + 1] ?? (editors[index].textContent?.length ?? 0);
      const offset = Math.min(end, Math.max(start, start + Math.round((x - left) / characterWidth)));
      return { offsetNode: editors[index].firstChild, offset };
    },
  });
  return () => {
    if (originalBounding) Object.defineProperty(Range.prototype, "getBoundingClientRect", originalBounding);
    else Reflect.deleteProperty(Range.prototype, "getBoundingClientRect");
    if (originalClient) Object.defineProperty(Range.prototype, "getClientRects", originalClient);
    else Reflect.deleteProperty(Range.prototype, "getClientRects");
    rectSpies.forEach((spy) => spy.mockRestore());
    Reflect.deleteProperty(document, "caretPositionFromPoint");
  };
}

afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); Reflect.deleteProperty(document, "caretPositionFromPoint"); document.body.replaceChildren(); window.getSelection()?.removeAllRanges(); });

describe("Notes document selection", () => {
  it("links a backward partial document selection when pasting a URL instead of replacing its words", async () => {
    const h = harness();
    const selection = { anchor: { blockId: h.ids[2], offset: 3 }, focus: { blockId: h.ids[0], offset: 2 } };
    await h.controller.select(selection);
    const paste = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(paste, "clipboardData", { value: { getData: (type: string) => type === "text/plain" ? "https://example.com/tasks" : "" } });
    h.editor(0).dispatchEvent(paste);
    await tick(); await tick();
    expect(h.link).toHaveBeenCalledExactlyOnceWith(h.ids, 2, 3, "https://example.com/tasks", selection);
    expect(h.replace).not.toHaveBeenCalled();
    expect(h.controller.selection).toEqual(selection);
    h.destroy();
  });

  it("honors plain-text paste over a document range instead of applying a hyperlink", async () => {
    const h = harness();
    const selection = { anchor: { blockId: h.ids[0], offset: 2 }, focus: { blockId: h.ids[2], offset: 3 } };
    await h.controller.select(selection);
    h.key(0, "v", { ctrlKey: true, shiftKey: true });
    const paste = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(paste, "clipboardData", { value: { getData: (type: string) => type === "text/plain" ? "https://example.com/tasks" : "<b>Tasks</b>" } });
    h.editor(0).dispatchEvent(paste);
    await tick(); await tick();
    expect(h.link).not.toHaveBeenCalled();
    expect(h.replace).toHaveBeenCalledExactlyOnceWith(h.ids, 2, 3, "https://example.com/tasks", undefined, selection);
    h.destroy();
  });

  it.each([3, 4])("includes named and empty databases in a whole-note copy with %i blocks", async (count) => {
    const h = harness(count);
    const pageId = "10000000-0000-4000-8000-000000000001";
    for (const [index, title] of [[1, "Tasks"], [2, ""]] as const) {
      const id = `10000000-0000-4000-8000-00000000000${index + 1}`;
      const previousId = h.ids[index];
      const database: NotesBlock = { ...h.blocks.get(previousId)!, id,
        parent: { type: "page_id", page_id: pageId }, type: "child_database", child_database: { title } };
      const row = h.editor(index).parentElement!;
      h.editor(index).replaceWith(document.createElement("section"));
      row.dataset.notesSelectableBlockId = id;
      row.firstElementChild!.textContent = title || "New database";
      h.ids[index] = id;
      h.blocks.delete(previousId);
      h.blocks.set(id, database);
    }
    h.key(0, "a", { ctrlKey: true });
    await tick();
    const setData = vi.fn();
    const copy = new Event("copy", { bubbles: true, cancelable: true });
    Object.defineProperty(copy, "clipboardData", { value: { setData } });
    h.editor(0).dispatchEvent(copy);
    expect(setData).toHaveBeenCalledWith("text/plain", `block-0\n\n[Tasks](#notes?page=${pageId}&block=${h.ids[1]})\n\n[New database](#notes?page=${pageId}&block=${h.ids[2]})${count === 4 ? "\n\nblock-3" : ""}`);
    expect(h.controller.error).toBeNull();
    expect(copy.defaultPrevented).toBe(true);
    h.destroy();
  });

  it("copies hidden children when Ctrl+A selects a single closed toggle", async () => {
    const h = harness(1, "toggle");
    const root = h.blocks.get(h.ids[0])!;
    if (root.type !== "toggle") throw new Error("Expected toggle");
    root.toggle.ganbaru_open = false;
    const child = { ...createBlockWrite("hidden", "paragraph", "Inside"),
      parent: { type: "block_id", block_id: root.id } } as NotesBlock;
    h.blocks.set(child.id, child);
    h.outlineSubtreeIds.mockReturnValue([root.id, child.id]);
    h.key(0, "a", { ctrlKey: true });
    await tick();
    expect(h.controller.selection).not.toBeNull();

    const setData = vi.fn();
    const event = new Event("copy", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: { setData } });
    h.editor(0).dispatchEvent(event);
    expect(setData).toHaveBeenCalledWith("text/plain", "- block-0\n\n    Inside");
    expect(setData).toHaveBeenCalledWith("text/html", '<details open data-notes-toggle-open="false"><summary>block-0</summary><p>Inside</p></details>');
    h.destroy();
  });

  it("copies both toggle bodies in document order when the first toggle is closed", async () => {
    const h = harness(5);
    const set = (id: string, type: "toggle" | "paragraph", text: string, parentId?: string) => {
      const previous = h.blocks.get(id)!;
      h.blocks.set(id, { ...previous, ...createBlockWrite(id, type, text),
        parent: parentId ? { type: "block_id", block_id: parentId } : previous.parent,
      } as NotesBlock);
    };
    set(h.ids[0], "toggle", "Example one");
    set(h.ids[1], "toggle", "Example two");
    const first = h.blocks.get(h.ids[0])!;
    if (first.type !== "toggle") throw new Error("Expected toggle");
    first.toggle.ganbaru_open = false;
    for (const [index, text] of ["First row", "Second row", "Third row"].entries()) {
      set(h.ids[index + 2], "paragraph", text, h.ids[1]);
    }
    const hiddenIds = ["hidden-1", "hidden-2", "hidden-3"];
    for (const [index, text] of ["First row", "Second row", "Third row"].entries()) {
      h.blocks.set(hiddenIds[index], { ...createBlockWrite(hiddenIds[index], "paragraph", text),
        parent: { type: "block_id", block_id: first.id } } as NotesBlock);
    }
    h.outlineSubtreeIds.mockImplementation((roots) => roots[0] === first.id
      ? [first.id, ...hiddenIds] : [...roots]);
    h.key(1, "a", { ctrlKey: true });
    await tick();

    const setData = vi.fn();
    const event = new Event("copy", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: { setData } });
    h.editor(1).dispatchEvent(event);
    const expectedMarkdown = [
      "- Example one", "", "    First row", "    ", "    Second row", "    ", "    Third row",
      "", "- Example two", "", "    First row", "    ", "    Second row", "    ", "    Third row",
    ].join("\n");
    expect(setData).toHaveBeenCalledWith("text/plain", expectedMarkdown);
    const html = setData.mock.calls.find(([type]) => type === "text/html")?.[1] as string;
    expect(html).toContain('<details open data-notes-toggle-open="false"><summary>Example one</summary><p>First row</p><p>Second row</p><p>Third row</p></details>');
    expect(html).toContain("<details open><summary>Example two</summary><p>First row</p><p>Second row</p><p>Third row</p></details>");
    h.controller.clear();
    await h.controller.select({ anchor: { blockId: h.ids[0], offset: 0 },
      focus: { blockId: h.ids[4], offset: "Third row".length } });
    setData.mockClear();
    h.editor(1).dispatchEvent(event);
    expect(setData).toHaveBeenCalledWith("text/plain", expectedMarkdown);
    h.destroy();
  });

  it("hydrates an unloaded closed toggle child before the asynchronous clipboard write", async () => {
    const h = harness(1, "toggle");
    const root = h.blocks.get(h.ids[0])!;
    if (root.type !== "toggle") throw new Error("Expected toggle");
    root.toggle.ganbaru_open = false;
    const child = { ...createBlockWrite("hidden", "paragraph", "Inside"),
      parent: { type: "block_id", block_id: root.id } } as NotesBlock;
    h.outlineSubtreeIds.mockReturnValue([root.id, child.id]);
    h.hydrate.mockImplementation(async () => { h.blocks.set(child.id, child); });
    const copied: Record<string, Blob>[] = [];
    vi.stubGlobal("ClipboardItem", class { constructor(data: Record<string, Blob>) { copied.push(data); } });
    vi.stubGlobal("navigator", { clipboard: { write: vi.fn(async () => undefined) } });
    h.key(0, "a", { ctrlKey: true });
    await tick();
    await h.controller.copy();
    expect(h.hydrate).toHaveBeenCalledWith([root.id, child.id]);
    expect(await copied[0]["text/plain"].text()).toContain("    Inside");
    h.destroy();
  });

  it("hydrates unloaded descendants before exporting whole blocks", async () => {
    const h = harness();
    const child = h.blocks.get(h.ids[1])!;
    child.parent = { type: "block_id", block_id: h.ids[0] };
    h.blocks.delete(child.id);
    h.hydrateSubtrees.mockImplementation(async () => {
      h.blocks.set(child.id, child);
      return [h.ids[0], child.id];
    });
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    h.blockSelection.setSelection({ anchorBlockId: h.ids[0], focusBlockId: h.ids[0], selectedBlockIds: [h.ids[0]] });
    await h.blockSelection.copy("copy");
    expect(h.hydrateSubtrees).toHaveBeenCalledExactlyOnceWith([h.ids[0]]);
    expect(writeText).toHaveBeenCalledExactlyOnceWith("block-0\n\nblock-1");
    expect(h.blockSelection.clipboard?.subtreeBlockIds).toEqual([h.ids[0], child.id]);
    h.destroy();
  });

  it("preserves document selection when database deletion is cancelled", async () => {
    const h = harness();
    h.replace.mockResolvedValue(false);
    const selection = { anchor: { blockId: h.ids[0], offset: 2 }, focus: { blockId: h.ids[2], offset: 4 } };
    await h.controller.select(selection);
    await h.controller.replace("");
    expect(h.controller.selection).toEqual(selection);
    expect(h.editor(0).textContent).toBe("block-0");
    expect(h.editor(2).textContent).toBe("block-2");
    h.destroy();
  });

  it("keeps cancelled block cuts as reusable copies without clearing the selected blocks", async () => {
    const h = harness();
    vi.stubGlobal("navigator", { clipboard: { writeText: vi.fn(async () => undefined) } });
    h.deleteBlocks.mockResolvedValue(false);
    const selected = { anchorBlockId: h.ids[0], focusBlockId: h.ids[2], selectedBlockIds: h.ids };
    h.blockSelection.setSelection(selected);
    await h.blockSelection.copy("cut");
    expect(h.blockSelection.selection).toEqual(selected);
    expect(h.blockSelection.clipboard).toMatchObject({ mode: "copy", subtreeBlockIds: h.ids });
    await h.blockSelection.remove();
    expect(h.blockSelection.selection).toEqual(selected);
    h.destroy();
  });

  it("writes rich document and whole-block copies, and retains document text after failed cuts", async () => {
    const h = harness(3, "heading_2");
    const contents: Record<string, Blob>[] = [];
    class TestClipboardItem {
      constructor(data: Record<string, Blob>) { contents.push(data); }
    }
    const write = vi.fn(async () => undefined);
    vi.stubGlobal("ClipboardItem", TestClipboardItem);
    vi.stubGlobal("navigator", { clipboard: { write } });
    await h.controller.select({ anchor: { blockId: h.ids[0], offset: 2 }, focus: { blockId: h.ids[2], offset: 4 } });
    await h.controller.copy();
    expect(Object.keys(contents[0])).toEqual(["text/plain", "text/html"]);
    expect(contents[0]["text/html"].size).toBeGreaterThan(contents[0]["text/plain"].size);
    write.mockRejectedValueOnce(new Error("clipboard denied"));
    await expect(h.controller.copy(true)).rejects.toThrow("clipboard denied");
    expect(h.replace).not.toHaveBeenCalled();
    h.controller.clear();
    h.blockSelection.setSelection({ anchorBlockId: h.ids[0], focusBlockId: h.ids[2], selectedBlockIds: h.ids });
    await h.blockSelection.copy("copy");
    expect(Object.keys(contents.at(-1)!)).toEqual(["text/plain", "text/html"]);
    expect(h.blockSelection.clipboard?.plainText).toBe("## block-0\n\n## block-1\n\n## block-2");
    h.destroy();
  });

  it.each(["paragraph", "numbered_list_item", "bulleted_list_item", "to_do"] as const)(
    "places a caret near a padding or marker click in a %s row without selecting the block",
    async (type) => {
      const h = harness(3, type);
      const root = h.editor(1);
      const row = root.parentElement!;
      const marker = document.createElement("span");
      marker.textContent = "2.";
      row.prepend(marker);
      vi.spyOn(root, "getBoundingClientRect").mockReturnValue(new DOMRect(30, 20, 100, 60));
      const hit = vi.fn((x: number, y: number) => x >= 30 && x < 130 && y >= 20 && y < 80
        ? { offsetNode: root.firstChild, offset: x < 50 ? 4 : 7 } : null);
      Object.defineProperty(document, "caretPositionFromPoint", { configurable: true, value: hit });
      h.blockSelection.setSelection({ anchorBlockId: h.ids[0], focusBlockId: h.ids[0], selectedBlockIds: [h.ids[0]] });
      for (const [target, x, expected] of [[marker, 10, 4], [row, 150, 7]] as const) {
        const event = new MouseEvent("pointerdown", { bubbles: true, cancelable: true, button: 0, clientX: x, clientY: 65 });
        target.dispatchEvent(event);
        await tick();
        expect(event.defaultPrevented).toBe(true);
        expect(document.activeElement).toBe(root);
        expect(window.getSelection()?.focusNode).toBe(root.firstChild);
        expect(window.getSelection()?.focusOffset).toBe(expected);
        expect(h.focus).toHaveBeenLastCalledWith({ blockId: h.ids[1], offset: expected }, true);
        expect(window.getSelection()?.isCollapsed).toBe(true);
        expect(h.blockSelection.selection).toBeNull();
        expect(row.hasAttribute("data-notes-block-selected")).toBe(false);
        expect(h.focusRow).not.toHaveBeenCalled();
      }
      h.key(1, "ArrowDown", { shiftKey: true });
      await tick();
      expect(h.controller.selection?.focus.blockId).toBe(h.ids[2]);
      expect(h.blockSelection.selection).toBeNull();
      h.destroy();
    },
  );

  it("extends text from a row margin with Shift-click and dragging, including within one editor", async () => {
    const h = harness();
    let hitIndex = 1;
    let offset = 2;
    Object.defineProperty(document, "caretPositionFromPoint", { configurable: true,
      value: () => ({ offsetNode: h.editor(hitIndex).firstChild, offset }),
    });
    window.getSelection()?.collapse(h.editor(0).firstChild, 1);
    h.editor(1).parentElement!.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true, button: 0, shiftKey: true }));
    expect(h.controller.selection).toEqual({ anchor: { blockId: h.ids[0], offset: 1 }, focus: { blockId: h.ids[1], offset: 2 } });
    h.editor(1).parentElement!.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true, button: 0 }));
    offset = 5;
    h.editor(1).dispatchEvent(new MouseEvent("pointermove", { bubbles: true, cancelable: true, buttons: 1 }));
    expect(window.getSelection()?.toString()).toBe("ock");
    hitIndex = 2;
    offset = 4;
    // Pointer capture can keep the original target while the hit moves into another row.
    h.editor(1).parentElement!.dispatchEvent(new MouseEvent("pointermove", { bubbles: true, cancelable: true, buttons: 1 }));
    expect(h.controller.selection).toEqual({ anchor: { blockId: h.ids[1], offset: 2 }, focus: { blockId: h.ids[2], offset: 4 } });
    expect(h.blockSelection.selection).toBeNull();
    await tick();
    h.destroy();
  });

  it("focuses an empty row when its padding has no browser caret hit", () => {
    const h = harness();
    const root = h.editor(1);
    root.replaceChildren();
    const block = h.blocks.get(h.ids[1])!;
    h.blocks.set(block.id, { ...block, ...createBlockWrite(block.id, "paragraph", "") } as NotesBlock);
    root.parentElement!.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true, button: 0 }));
    expect(document.activeElement).toBe(root);
    expect(window.getSelection()?.focusNode).toBe(root);
    expect(window.getSelection()?.focusOffset).toBe(0);
    expect(h.focus).toHaveBeenLastCalledWith({ blockId: h.ids[1], offset: 0 }, true);
    expect(h.blockSelection.selection).toBeNull();
    h.destroy();
  });

  it("leaves row controls to their own pointer handlers even when a block was selected", () => {
    const h = harness();
    for (const tag of ["button", "input", "select", "a"]) {
      const control = document.createElement(tag);
      h.editor(1).parentElement!.append(control);
      h.blockSelection.setSelection({ anchorBlockId: h.ids[0], focusBlockId: h.ids[0], selectedBlockIds: [h.ids[0]] });
      const event = new MouseEvent("pointerdown", { bubbles: true, cancelable: true, button: 0, shiftKey: true });
      control.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(false);
      expect(h.blockSelection.selection).toBeNull();
      expect(h.controller.selection).toBeNull();
      expect(h.focusRow).not.toHaveBeenCalled();
    }
    h.destroy();
  });

  it.each(["paragraph", "numbered_list_item", "bulleted_list_item", "to_do"] as const)(
    "excludes the untouched endpoint row when indenting a keyboard selection in %s blocks",
    async (type) => {
      const h = harness(4, type);
      for (const backward of [false, true]) {
        h.controller.clear();
        const origin = backward ? 3 : 1;
        window.getSelection()?.collapse(h.editor(origin).firstChild, 0);
        if (backward) {
          await h.controller.select({ anchor: { blockId: h.ids[3], offset: 0 }, focus: { blockId: h.ids[1], offset: 0 } });
        } else {
          h.key(origin, "ArrowDown", { shiftKey: true });
          await tick();
          h.key(origin, "ArrowDown", { shiftKey: true });
          await tick();
        }
        const selected = h.controller.selection;
        expect(selected).toEqual({
          anchor: { blockId: h.ids[origin], offset: 0 },
          focus: { blockId: h.ids[backward ? 1 : 3], offset: 0 },
        });
        h.key(origin, "Tab");
        await vi.waitFor(() => expect(h.indent).toHaveBeenLastCalledWith(h.ids.slice(1, 3), "nest", selected));
        h.key(origin, "Tab", { shiftKey: true });
        await vi.waitFor(() => expect(h.indent).toHaveBeenLastCalledWith(h.ids.slice(1, 3), "outdent", selected));
        expect(h.controller.selection).toEqual(selected);
        const setData = vi.fn();
        const copy = new Event("copy", { bubbles: true, cancelable: true });
        Object.defineProperty(copy, "clipboardData", { value: { setData } });
        h.editor(origin).dispatchEvent(copy);
        const expected = {
          paragraph: "block-1\n\nblock-2\n",
          numbered_list_item: "1. block-1\n2. block-2\n",
          bulleted_list_item: "- block-1\n- block-2\n",
          to_do: "- [ ] block-1\n- [ ] block-2\n",
        }[type];
        expect(setData).toHaveBeenCalledWith("text/plain", expected);
      }
      h.destroy();
    },
  );

  it("contracts across the line break before removing the previous row's last character", async () => {
    const h = harness(4, "numbered_list_item");
    const native = window.getSelection()!;
    await h.controller.select({ anchor: { blockId: h.ids[1], offset: 0 }, focus: { blockId: h.ids[3], offset: 0 } });
    h.key(1, "ArrowLeft", { shiftKey: true });
    await tick();
    expect(h.controller.selection?.focus).toEqual({ blockId: h.ids[2], offset: 7 });
    // jsdom has no Selection.modify. Model the browser's movement inside this text node.
    Object.defineProperty(native, "modify", { configurable: true, value: () => {
      native.collapse(native.focusNode, native.focusOffset - 1);
    } });
    try {
      h.key(1, "ArrowLeft", { shiftKey: true });
      await tick();
      expect(h.controller.selection?.focus).toEqual({ blockId: h.ids[2], offset: 6 });
      const setData = vi.fn();
      const cut = new Event("cut", { bubbles: true, cancelable: true });
      Object.defineProperty(cut, "clipboardData", { value: { setData } });
      h.editor(1).dispatchEvent(cut);
      expect(setData).toHaveBeenCalledWith("text/plain", "1. block-1\n2. block-");
      expect(h.replace).toHaveBeenCalledWith(h.ids.slice(1, 3), 0, 6, "", undefined, {
        anchor: { blockId: h.ids[1], offset: 0 }, focus: { blockId: h.ids[2], offset: 6 },
      });
    } finally {
      Reflect.deleteProperty(native, "modify");
      h.destroy();
    }
  });

  it.each(["Tab", "Unidentified"])("routes Tab and Shift+Tab over the full document selection without moving focus (key: %s)", async (key) => {
    const h = harness();
    window.getSelection()?.collapse(h.editor(1).firstChild, 2);
    h.key(1, "a", { ctrlKey: true });
    await tick();
    const selected = h.controller.selection;
    expect(h.key(1, "Tab").defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(h.indent).toHaveBeenLastCalledWith(h.ids, "nest", selected));
    expect(h.key(1, key, { code: "Tab", shiftKey: true }).defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(h.indent).toHaveBeenLastCalledWith(h.ids, "outdent", selected));
    expect(h.controller.selection).toEqual(selected);
    h.destroy();
  });

  it("selects the whole document from a text caret, including more than 200 blocks", async () => {
    const h = harness(220);
    window.getSelection()?.collapse(h.editor(1).firstChild, 2);
    expect(h.key(1, "a", { ctrlKey: true }).defaultPrevented).toBe(true);
    await tick();
    expect(h.controller.pinnedIds).toEqual([]);
    expect(h.controller.selection?.focus.blockId).toBe(h.ids.at(-1));
    expect(window.getSelection()?.anchorNode).toBe(h.editor(0).firstChild);
    expect(window.getSelection()?.focusNode).toBe(h.editor(219).firstChild);
    h.destroy();
  });

  it("does not load unmounted content while only extending a text selection", async () => {
    const h = harness(4);
    const missing = h.blocks.get(h.ids[2])!;
    h.blocks.delete(missing.id);
    h.hydrate.mockImplementation(async () => { h.blocks.set(missing.id, missing); });
    vi.stubGlobal("navigator", { clipboard: { writeText: vi.fn(async () => undefined) } });
    await h.controller.select({
      anchor: { blockId: h.ids[0], offset: 2 }, focus: { blockId: h.ids[3], offset: 4 },
    });
    expect(h.controller.pinnedIds).toEqual([]);
    expect(h.hydrate).not.toHaveBeenCalled();
    await h.controller.copy();
    expect(h.hydrate).toHaveBeenCalledWith(h.ids);
    h.destroy();
  });

  it("coalesces repeated cross-block keyboard repaints into one frame", async () => {
    class TestHighlight { constructor(..._ranges: Range[]) {} }
    const registry = new Map<string, TestHighlight>();
    const painted = vi.fn((name: string, highlight: TestHighlight) => registry.set(name, highlight));
    vi.stubGlobal("CSS", { highlights: { set: painted, delete: (name: string) => registry.delete(name) } });
    vi.stubGlobal("Highlight", TestHighlight);
    const frames: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      frames.push(callback);
      return frames.length;
    });
    const h = harness();
    await h.controller.select({
      anchor: { blockId: h.ids[0], offset: 2 }, focus: { blockId: h.ids[2], offset: 4 },
    });
    expect(painted).toHaveBeenCalledTimes(1);
    for (const key of ["ArrowUp", "ArrowDown", "ArrowUp", "ArrowDown"]) {
      h.key(2, key, { shiftKey: true });
      await tick();
    }
    expect(h.controller.selection?.anchor).toEqual({ blockId: h.ids[0], offset: 2 });
    expect(painted).toHaveBeenCalledTimes(1);
    expect(frames).toHaveLength(1);
    frames[0](0);
    expect(painted).toHaveBeenCalledTimes(2);
    h.destroy();
  });

  it("stops pending keyboard movement when Shift is released during hydration", async () => {
    const h = harness();
    const missing = h.blocks.get(h.ids[1])!;
    h.blocks.delete(missing.id);
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    h.hydrate.mockImplementation(async () => { await gate; h.blocks.set(missing.id, missing); });
    window.getSelection()?.collapse(h.editor(0).firstChild, 4);
    h.key(0, "ArrowDown", { shiftKey: true });
    await vi.waitFor(() => expect(h.hydrate).toHaveBeenCalledExactlyOnceWith([h.ids[1]]));
    h.key(0, "ArrowDown", { shiftKey: true });
    document.dispatchEvent(new KeyboardEvent("keyup", { key: "Shift", bubbles: true }));
    release();
    await tick();
    await tick();
    expect(h.controller.selection).toBeNull();
    expect(h.hydrate).toHaveBeenCalledTimes(1);
    h.destroy();
  });

  it("paints every text segment even when the native selection stays inside the active editor", async () => {
    class TestHighlight {
      constructor(...ranges: Range[]) { this.ranges = ranges; }
      ranges: Range[];
    }
    const registry = new Map<string, TestHighlight>();
    vi.stubGlobal("CSS", { highlights: registry });
    vi.stubGlobal("Highlight", TestHighlight);
    const h = harness();
    const native = window.getSelection()!;
    vi.spyOn(native, "setBaseAndExtent").mockImplementation(() => native.collapse(h.editor(1).firstChild, 2));
    await h.controller.select({ anchor: { blockId: h.ids[0], offset: 6 }, focus: { blockId: h.ids[2], offset: 5 } });
    expect(native.anchorNode).toBe(h.editor(1).firstChild);
    expect([...registry.values()][0].ranges.map((range) => range.toString())).toEqual(["0", "block-1", "block"]);
    h.editor(0).parentElement!.remove();
    h.controller.repaint();
    expect([...registry.values()][0].ranges.map((range) => range.toString())).toEqual(["block-1", "block"]);
    h.controller.clear();
    expect(registry.size).toBe(0);
    expect(document.querySelector("[data-notes-painted-selection]")).toBeNull();
    h.destroy();
  });

  it("repaints and edits the complete restored selection after replacement", async () => {
    class TestHighlight {
      constructor(...ranges: Range[]) { this.ranges = ranges; }
      ranges: Range[];
    }
    const registry = new Map<string, TestHighlight>();
    vi.stubGlobal("CSS", { highlights: registry });
    vi.stubGlobal("Highlight", TestHighlight);
    const h = harness();
    h.key(1, "a", { ctrlKey: true });
    const saved = h.controller.selection!;
    await h.controller.replace("");
    expect(registry.size).toBe(0);
    // Undo restores the tree before asking the document controller to restore this range.
    await h.controller.select(saved);
    expect([...registry.values()][0].ranges.map((range) => range.toString())).toEqual(h.ids);
    h.key(0, "X");
    expect(h.replace).toHaveBeenLastCalledWith(h.ids, 0, Number.MAX_SAFE_INTEGER, "X", undefined, saved);
    await tick();
    h.destroy();
  });

  it("keeps selection highlights isolated between open editors and removes them on unmount", () => {
    const registry = new Map<string, object>();
    vi.stubGlobal("CSS", { highlights: registry });
    vi.stubGlobal("Highlight", class { constructor(..._ranges: Range[]) {} });
    const first = harness();
    const second = harness();
    first.key(0, "a", { ctrlKey: true });
    second.key(0, "a", { ctrlKey: true });
    expect(registry.size).toBe(2);
    first.destroy();
    expect(registry.size).toBe(1);
    second.destroy();
    expect(registry.size).toBe(0);
  });

  it("keeps text typed while a whole-page replacement loads its offscreen content", async () => {
    const h = harness();
    const missing = h.blocks.get(h.ids[2])!;
    h.blocks.delete(missing.id);
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    h.hydrate.mockImplementation(async () => { await gate; h.blocks.set(missing.id, missing); });
    h.key(0, "a", { ctrlKey: true });
    h.key(0, "X");
    h.key(0, "Y");
    await tick();
    release();
    await vi.waitFor(() => expect(h.replace).toHaveBeenCalledWith(h.ids, 0, Number.MAX_SAFE_INTEGER, "XY", undefined, { anchor: { blockId: h.ids[0], offset: 0 }, focus: { blockId: h.ids[h.ids.length - 1], offset: Number.MAX_SAFE_INTEGER } }));
    h.destroy();
  });

  it("does not let a cancelled hydration discard typing for a newer selection", async () => {
    const h = harness();
    const missing = h.blocks.get(h.ids[2])!;
    h.blocks.delete(missing.id);
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    h.hydrate.mockImplementation(async () => { await gate; h.blocks.set(missing.id, missing); });
    h.key(0, "a", { ctrlKey: true });
    h.key(0, "X");
    await vi.waitFor(() => expect(h.hydrate).toHaveBeenCalled());
    h.controller.clear();
    h.key(0, "a", { ctrlKey: true });
    h.key(0, "Y");
    h.key(0, "Z");
    release();
    await vi.waitFor(() => expect(h.replace).toHaveBeenCalledExactlyOnceWith(h.ids, 0, Number.MAX_SAFE_INTEGER, "YZ", undefined, { anchor: { blockId: h.ids[0], offset: 0 }, focus: { blockId: h.ids[h.ids.length - 1], offset: Number.MAX_SAFE_INTEGER } }));
    h.destroy();
  });

  it("moves the caret across block edges and navigates to the complete document boundary", () => {
    const h = harness();
    window.getSelection()?.collapse(h.editor(1).firstChild, 0);
    expect(h.key(1, "ArrowLeft").defaultPrevented).toBe(true);
    expect(h.focus).toHaveBeenLastCalledWith({ blockId: h.ids[0], offset: Number.MAX_SAFE_INTEGER });
    expect(h.key(1, "End", { ctrlKey: true }).defaultPrevented).toBe(true);
    expect(h.focus).toHaveBeenLastCalledWith({ blockId: h.ids[2], offset: Number.MAX_SAFE_INTEGER });
    h.destroy();
  });

  it("extends backwards across an editing host boundary without rounding to whole blocks", async () => {
    const h = harness();
    window.getSelection()?.collapse(h.editor(1).firstChild, 0);
    h.key(1, "ArrowLeft", { shiftKey: true });
    await tick(); await tick(); await tick();
    expect(h.controller.selection).toEqual({
      anchor: { blockId: h.ids[1], offset: 0 }, focus: { blockId: h.ids[0], offset: 7 },
    });
    h.destroy();
  });

  it("moves one visual line per Shift+Up or Shift+Down and keeps the original text anchor", async () => {
    const h = harness();
    const restoreGeometry = mockVisualLines([h.editor(0), h.editor(1), h.editor(2)], [[0], [0, 3, 6], [0]]);
    const native = window.getSelection()!;
    native.collapse(h.editor(1).firstChild, 4);
    try {
      for (const [key, block, offset] of [
        ["ArrowUp", 1, 1], ["ArrowUp", 0, 1], ["ArrowDown", 1, 1],
        ["ArrowDown", 1, 4], ["ArrowDown", 1, 7], ["ArrowDown", 2, 1],
      ] as const) {
        expect(h.key(1, key, { shiftKey: true }).defaultPrevented).toBe(true);
        await tick();
        if (block === 1 && offset === 4) {
          expect(h.controller.selection).toBeNull();
          expect(native.anchorOffset).toBe(4);
        } else {
          expect(h.controller.selection).toEqual({
            anchor: { blockId: h.ids[1], offset: 4 },
            focus: { blockId: h.ids[block], offset },
          });
        }
      }
      expect(native.anchorNode).toBe(h.editor(1).firstChild);
      expect(native.anchorOffset).toBe(4);
    } finally {
      restoreGeometry();
      h.destroy();
    }
  });

  it("does not repeat a wrapped line when a boundary offset has ambiguous caret geometry", async () => {
    const h = harness();
    const restoreGeometry = mockVisualLines([h.editor(0), h.editor(1), h.editor(2)], [[0, 3, 6], [0], [0]]);
    const bounding = Range.prototype.getBoundingClientRect;
    Object.defineProperty(Range.prototype, "getBoundingClientRect", { configurable: true,
      value: function (this: Range) {
        if (this.startContainer === h.editor(0).firstChild && this.startOffset === 3) {
          return new DOMRect(20, 100, 0, 20);
        }
        return bounding.call(this);
      },
    });
    window.getSelection()?.collapse(h.editor(0).firstChild, 0);
    try {
      for (const [key, offset] of [["ArrowDown", 3], ["ArrowDown", 6], ["ArrowUp", 3]] as const) {
        h.key(0, key, { shiftKey: true });
        await tick();
        expect(h.controller.selection).toEqual({
          anchor: { blockId: h.ids[0], offset: 0 }, focus: { blockId: h.ids[0], offset },
        });
      }
    } finally {
      restoreGeometry();
      h.destroy();
    }
  });

  it("crosses a block edge on the first vertical key and keeps the anchor after reversing direction", async () => {
    const h = harness();
    const restoreGeometry = mockVisualLines([h.editor(0), h.editor(1), h.editor(2)], [[0], [0], [0]]);
    const native = window.getSelection()!;
    native.collapse(h.editor(1).firstChild, 4);
    try {
      h.key(1, "ArrowUp", { shiftKey: true });
      await tick();
      expect(h.controller.selection).toEqual({
        anchor: { blockId: h.ids[1], offset: 4 }, focus: { blockId: h.ids[0], offset: 4 },
      });
      h.key(1, "ArrowDown", { shiftKey: true });
      await tick();
      expect(h.controller.selection).toBeNull();
      expect(native.anchorNode).toBe(h.editor(1).firstChild);
      expect(native.anchorOffset).toBe(4);
      h.key(1, "ArrowDown", { shiftKey: true });
      await tick();
      expect(h.controller.selection).toEqual({
        anchor: { blockId: h.ids[1], offset: 4 }, focus: { blockId: h.ids[2], offset: 4 },
      });
    } finally {
      restoreGeometry();
      h.destroy();
    }
  });

  it("retains the original anchor when a reversed range reenters its editor at another offset", async () => {
    const h = harness();
    const restoreGeometry = mockVisualLines([h.editor(0), h.editor(1), h.editor(2)], [[0], [0], [0]]);
    const normalHit = document.caretPositionFromPoint!.bind(document);
    const native = window.getSelection()!;
    native.collapse(h.editor(1).firstChild, 4);
    try {
      h.key(1, "ArrowUp", { shiftKey: true });
      await tick();
      Object.defineProperty(document, "caretPositionFromPoint", { configurable: true,
        value: (x: number, y: number) => y >= 200 && y < 220
          ? { offsetNode: h.editor(1).firstChild, offset: 2 }
          : normalHit(x, y),
      });
      h.key(1, "ArrowDown", { shiftKey: true });
      await tick();
      expect(h.controller.selection).toEqual({
        anchor: { blockId: h.ids[1], offset: 4 }, focus: { blockId: h.ids[1], offset: 2 },
      });
      h.key(1, "ArrowDown", { shiftKey: true });
      await tick();
      expect(h.controller.selection).toEqual({
        anchor: { blockId: h.ids[1], offset: 4 }, focus: { blockId: h.ids[2], offset: 4 },
      });
    } finally {
      restoreGeometry();
      h.destroy();
    }
  });

  it("keeps a visual column through a short block and reaches the next full line", async () => {
    const h = harness();
    const short = h.blocks.get(h.ids[1])!;
    h.blocks.set(short.id, { ...short, ...createBlockWrite(short.id, "paragraph", "x") } as NotesBlock);
    h.editor(1).textContent = "x";
    const restoreGeometry = mockVisualLines([h.editor(0), h.editor(1), h.editor(2)], [[0], [0], [0]]);
    window.getSelection()?.collapse(h.editor(0).firstChild, 4);
    try {
      h.key(0, "ArrowDown", { shiftKey: true });
      await tick();
      expect(h.controller.selection?.focus).toEqual({ blockId: h.ids[1], offset: 1 });
      h.key(0, "ArrowDown", { shiftKey: true });
      await tick();
      expect(h.controller.selection).toEqual({
        anchor: { blockId: h.ids[0], offset: 4 }, focus: { blockId: h.ids[2], offset: 4 },
      });
    } finally {
      restoreGeometry();
      h.destroy();
    }
  });

  it("returns a same-editor keyboard selection to native editing when Shift is released", async () => {
    const h = harness();
    const restoreGeometry = mockVisualLines([h.editor(0), h.editor(1), h.editor(2)], [[0, 3], [0], [0]]);
    const native = window.getSelection()!;
    native.collapse(h.editor(0).firstChild, 4);
    try {
      h.key(0, "ArrowUp", { shiftKey: true });
      await tick();
      expect(h.controller.selection).toEqual({
        anchor: { blockId: h.ids[0], offset: 4 }, focus: { blockId: h.ids[0], offset: 1 },
      });
      document.dispatchEvent(new KeyboardEvent("keyup", { key: "Shift", bubbles: true }));
      expect(h.controller.selection).toBeNull();
      expect(native.anchorOffset).toBe(4);
      expect(native.focusOffset).toBe(1);
    } finally {
      restoreGeometry();
      h.destroy();
    }
  });

  it("selects to the first note line's start without changing its original anchor", async () => {
    const h = harness();
    const restoreGeometry = mockVisualLines([h.editor(0), h.editor(1), h.editor(2)], [[0], [0], [0]]);
    const native = window.getSelection()!;
    native.collapse(h.editor(0).firstChild, 4);
    try {
      h.key(0, "ArrowUp", { shiftKey: true });
      await tick();
      expect(h.controller.selection).toEqual({
        anchor: { blockId: h.ids[0], offset: 4 }, focus: { blockId: h.ids[0], offset: 0 },
      });
      document.dispatchEvent(new KeyboardEvent("keyup", { key: "Shift", bubbles: true }));
      expect(native.anchorOffset).toBe(4);
      expect(native.focusOffset).toBe(0);
      expect(native.toString()).toBe("bloc");
    } finally {
      restoreGeometry();
      h.destroy();
    }
  });

  it("skips a hidden callout label when extending text in either direction", async () => {
    const h = harness(4, "paragraph", [1]);
    const calloutId = h.ids[1];
    const prior = h.blocks.get(calloutId)!;
    h.blocks.set(calloutId, { ...prior, ...createBlockWrite(calloutId, "callout"), has_children: true } as NotesBlock);
    h.editor(1).remove();
    const native = window.getSelection()!;
    native.collapse(h.editor(0).firstChild, 7);
    h.key(0, "ArrowDown", { shiftKey: true });
    await tick();
    expect(h.controller.selection?.focus).toEqual({ blockId: h.ids[2], offset: 0 });
    expect(h.editor(0).closest("[data-notes-document-selection]")).not.toBeNull();
    h.controller.clear();
    native.collapse(h.editor(2).firstChild, 0);
    h.key(2, "ArrowUp", { shiftKey: true });
    await tick();
    expect(h.controller.selection?.focus).toEqual({ blockId: h.ids[0], offset: 7 });
    h.destroy();
  });

  it("extends to the document boundary and replaces a reverse partial range", async () => {
    const h = harness();
    window.getSelection()?.collapse(h.editor(1).firstChild, 3);
    h.key(1, "Home", { ctrlKey: true, shiftKey: true });
    await tick();
    h.key(1, "X");
    expect(h.replace).toHaveBeenCalledWith(h.ids.slice(0, 2), 0, 3, "X", undefined, { anchor: { blockId: h.ids[1], offset: 3 }, focus: { blockId: h.ids[0], offset: 0 } });
    await tick();
    expect(h.controller.selection).toBeNull();
    h.destroy();
  });

  it("keeps the original text anchor when Shift-clicking another editor", async () => {
    const h = harness();
    window.getSelection()?.collapse(h.editor(0).firstChild, 2);
    Object.defineProperty(document, "caretPositionFromPoint", { configurable: true, value: () => ({ offsetNode: h.editor(2).firstChild, offset: 4 }) });
    h.editor(2).dispatchEvent(new MouseEvent("pointerdown", { button: 0, shiftKey: true, bubbles: true, cancelable: true }));
    expect(h.controller.selection).toEqual({ anchor: { blockId: h.ids[0], offset: 2 }, focus: { blockId: h.ids[2], offset: 4 } });
    h.destroy();
  });

  it("retains partial endpoints while dragging across editing hosts", () => {
    const h = harness();
    let target = 0;
    Object.defineProperty(document, "caretPositionFromPoint", { configurable: true, value: () => ({ offsetNode: h.editor(target).firstChild, offset: target === 0 ? 2 : 4 }) });
    h.editor(0).dispatchEvent(new MouseEvent("pointerdown", { button: 0, bubbles: true, cancelable: true }));
    target = 2;
    h.editor(2).dispatchEvent(new MouseEvent("pointermove", { buttons: 1, bubbles: true, cancelable: true }));
    expect(h.controller.selection).toEqual({ anchor: { blockId: h.ids[0], offset: 2 }, focus: { blockId: h.ids[2], offset: 4 } });
    h.destroy();
  });

  it("commits composed text once for the entire range instead of saving the temporary host text", async () => {
    const h = harness();
    await h.controller.select({ anchor: { blockId: h.ids[0], offset: 2 }, focus: { blockId: h.ids[2], offset: 4 } });
    const restoreSelection = vi.spyOn(window.getSelection()!, "setBaseAndExtent");
    h.editor(0).dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    expect(h.editor(0).closest("[data-notes-document-selection-composing]")).not.toBeNull();
    h.controller.repaint();
    expect(restoreSelection).not.toHaveBeenCalled();
    h.editor(0).dispatchEvent(new InputEvent("beforeinput", { bubbles: true, inputType: "insertCompositionText", data: "に", isComposing: true }));
    expect(h.replace).not.toHaveBeenCalled();
    h.editor(0).dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "日本" }));
    expect(h.editor(0).closest("[data-notes-document-selection-composing]")).toBeNull();
    expect(h.replace).toHaveBeenCalledExactlyOnceWith(h.ids, 2, 4, "日本", undefined, { anchor: { blockId: h.ids[0], offset: 2 }, focus: { blockId: h.ids[2], offset: 4 } });
    h.destroy();
  });

  it("copies partial endpoints with line separators and removes listeners on destroy", async () => {
    const h = harness();
    await h.controller.select({ anchor: { blockId: h.ids[0], offset: 6 }, focus: { blockId: h.ids[2], offset: 5 } });
    const setData = vi.fn();
    const event = new Event("copy", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: { setData } });
    h.editor(0).dispatchEvent(event);
    expect(setData).toHaveBeenCalledWith("text/plain", "0\n\nblock-1\n\nblock");
    expect(setData).toHaveBeenCalledWith("text/html", "<p>0</p><p>block-1</p><p>block</p>");
    h.destroy();
    expect(h.key(0, "a", { ctrlKey: true }).defaultPrevented).toBe(false);
  });
});
