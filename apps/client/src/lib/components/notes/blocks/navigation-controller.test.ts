// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesBlock } from "$lib/notes/types";
import { createRichText } from "$lib/notes/blocks/factory";
import { createNotesBlockNavigationController } from "./navigation-controller";

function paragraph(id: string): NotesBlock {
  return {
    object: "block",
    id,
    parent: { type: "page_id", page_id: "page-1" },
    created_time: "2026-01-01T00:00:00Z",
    last_edited_time: "2026-01-01T00:00:00Z",
    has_children: false,
    in_trash: false,
    type: "paragraph",
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    paragraph: { rich_text: [], color: "default" },
  };
}

function mockRangeGeometry(rect: DOMRect, rects: DOMRectList): void {
  Object.defineProperty(Range.prototype, "getBoundingClientRect", {
    configurable: true,
    value: () => rect,
  });
  Object.defineProperty(Range.prototype, "getClientRects", {
    configurable: true,
    value: () => rects,
  });
}

describe("Notes block navigation controller", () => {
  it("moves from text through a database title and back in both directions", () => {
    const list = document.createElement("div");
    list.innerHTML = `
      <div data-notes-selectable-block-id="before"><div contenteditable="true" role="textbox" data-notes-block-id="before">Above</div></div>
      <div data-notes-selectable-block-id="database"><input data-notes-database-title value="Tasks"><input data-cell value="Cell"></div>
      <div data-notes-selectable-block-id="after"><div contenteditable="true" role="textbox" data-notes-block-id="after">Below</div></div>
    `;
    document.body.append(list);
    const database: NotesBlock = { ...paragraph("database"), type: "child_database", child_database: { title: "Tasks" } };
    const requestFocus = vi.fn();
    const controller = createNotesBlockNavigationController({
      readListElement: () => list, readRenderedBlockIds: () => ["before", "database", "after"],
      readBlock: (id) => {
        if (id === database.id) return database;
        const block = paragraph(id);
        if (block.type === "paragraph") block.paragraph.rich_text = [createRichText(id === "before" ? "Above" : "Below")];
        return block;
      },
      isHiddenCalloutLabel: () => false, requestFocus,
    });
    const title = list.querySelector<HTMLInputElement>("[data-notes-database-title]")!;
    for (const [id, key, targetId] of [
      ["before", "ArrowDown", "database"], ["database", "ArrowDown", "after"],
      ["after", "ArrowUp", "database"], ["database", "ArrowUp", "before"],
    ] as const) {
      const target = id === "database" ? title : list.querySelector<HTMLElement>(`[data-notes-block-id='${id}']`)!;
      if (id !== "database") document.getSelection()?.collapse(target.firstChild, key === "ArrowDown" ? 5 : 0);
      const event = new KeyboardEvent("keydown", { key, cancelable: true });
      Object.defineProperty(event, "target", { value: target });
      expect(controller.handleKeydown(event, id)).toBe(true);
      expect(event.defaultPrevented).toBe(true);
      expect(requestFocus).toHaveBeenLastCalledWith(targetId, targetId === "database" ? null
        : { start: key === "ArrowDown" ? 0 : 5, end: key === "ArrowDown" ? 0 : 5 });
    }
    for (const init of [
      { key: "ArrowLeft" }, { key: "ArrowRight" }, { key: "Enter" },
      { key: "ArrowDown", shiftKey: true }, { key: "ArrowUp", altKey: true },
      { key: "ArrowDown", isComposing: true },
    ]) {
      const event = new KeyboardEvent("keydown", { cancelable: true, ...init });
      Object.defineProperty(event, "target", { value: title });
      expect(controller.handleKeydown(event, database.id)).toBe(false);
      expect(event.defaultPrevented).toBe(false);
    }
    const cellEvent = new KeyboardEvent("keydown", { key: "ArrowDown", cancelable: true });
    Object.defineProperty(cellEvent, "target", { value: list.querySelector("[data-cell]") });
    expect(controller.handleKeydown(cellEvent, database.id)).toBe(false);
    expect(controller.targetIsEditable(title)).toBe(true);
    expect(requestFocus).toHaveBeenCalledTimes(4);
  });

  it("navigates from a focused note button and inserts paragraphs on either side", () => {
    const list = document.createElement("div");
    list.innerHTML = '<div data-notes-selectable-block-id="note"><button data-notes-atomic-block="note">Note</button></div>';
    document.body.append(list);
    const button = list.querySelector("button")!;
    const block: NotesBlock = { ...paragraph("note"), type: "child_page", child_page: { title: "Note" } };
    const requestFocus = vi.fn();
    const insertParagraphAdjacent = vi.fn();
    const controller = createNotesBlockNavigationController({
      readListElement: () => list, readRenderedBlockIds: () => ["before", "note", "after"],
      readBlock: (id) => id === "note" ? block : paragraph(id),
      isHiddenCalloutLabel: () => false, requestFocus, insertParagraphAdjacent,
    });
    list.addEventListener("keydown", (event) => controller.handleKeydown(event, "note"));
    for (const key of ["ArrowUp", "ArrowDown"]) button.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
    expect(requestFocus.mock.calls.map(([id]) => id)).toEqual(["before", "after"]);
    for (const shiftKey of [false, true]) {
      const event = new KeyboardEvent("keydown", { key: "Enter", shiftKey, bubbles: true, cancelable: true });
      button.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(true);
    }
    expect(insertParagraphAdjacent.mock.calls).toEqual([["note", "next"], ["note", "previous"]]);
  });

  it("does not inherit a containing layout's block-selection zone into a text row", () => {
    const list = document.createElement("div");
    list.innerHTML = `<div data-notes-selectable-block-id="layout" data-notes-block-selection-zone>
      <div data-notes-selectable-block-id="text"><span class="marker">1.</span></div>
    </div>`;
    const controller = createNotesBlockNavigationController({
      readListElement: () => list, readRenderedBlockIds: () => [], readBlock: () => undefined,
      isHiddenCalloutLabel: () => false, requestFocus: vi.fn(),
    });
    expect(controller.targetIsSelectionZone(list.firstElementChild)).toBe(true);
    expect(controller.targetIsSelectionZone(list.querySelector(".marker"))).toBe(false);
  });

  afterEach(() => {
    Reflect.deleteProperty(Range.prototype, "getBoundingClientRect");
    Reflect.deleteProperty(Range.prototype, "getClientRects");
    Reflect.deleteProperty(document, "caretPositionFromPoint");
    document.getSelection()?.removeAllRanges();
    document.body.replaceChildren();
  });

  it("moves from the last visual position of one editor to the next rendered block", () => {
    mockRangeGeometry({
      width: 0,
      height: 0,
    } as DOMRect, [] as unknown as DOMRectList);
    const list = document.createElement("div");
    list.innerHTML = `
      <div data-notes-selectable-block-id="first"><div contenteditable="true" role="textbox" data-notes-block-id="first">a</div></div>
      <div data-notes-selectable-block-id="second"><div contenteditable="true" role="textbox" data-notes-block-id="second"></div></div>
    `;
    document.body.append(list);
    const editor = list.querySelector<HTMLElement>("[data-notes-block-id='first']")!;
    const text = editor.firstChild!;
    const range = document.createRange();
    range.setStart(text, 1);
    range.collapse(true);
    const selection = document.getSelection()!;
    selection.removeAllRanges();
    selection.addRange(range);
    const requestFocus = vi.fn();
    const blocks = new Map([
      ["first", paragraph("first")],
      ["second", paragraph("second")],
    ]);
    const controller = createNotesBlockNavigationController({
      readListElement: () => list,
      readRenderedBlockIds: () => ["first", "second"],
      readBlock: (id) => blocks.get(id),
      isHiddenCalloutLabel: () => false,
      requestFocus,
    });
    const event = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
    Object.defineProperty(event, "target", { value: editor });
    expect(controller.handleKeydown(event, "first")).toBe(true);
    expect(requestFocus).toHaveBeenCalledWith("second", { start: 0, end: 0 });
  });

  it("moves directly across a callout's hidden label in both directions", () => {
    mockRangeGeometry({ width: 0, height: 0 } as DOMRect, [] as unknown as DOMRectList);
    const list = document.createElement("div");
    list.innerHTML = `
      <div data-notes-selectable-block-id="before"><div contenteditable="true" role="textbox" data-notes-block-id="before">Above</div></div>
      <div data-notes-selectable-block-id="callout"></div>
      <div data-notes-selectable-block-id="child"><div contenteditable="true" role="textbox" data-notes-block-id="child">Inside</div></div>
    `;
    document.body.append(list);
    const before = paragraph("before");
    if (before.type !== "paragraph") throw new Error("Expected paragraph");
    before.paragraph.rich_text = [createRichText("Above")];
    const child = paragraph("child");
    if (child.type !== "paragraph") throw new Error("Expected paragraph");
    child.paragraph.rich_text = [createRichText("Inside")];
    const blocks = new Map([[before.id, before], [child.id, child]]);
    const requestFocus = vi.fn();
    const controller = createNotesBlockNavigationController({
      readListElement: () => list,
      readRenderedBlockIds: () => ["before", "callout", "child"],
      readBlock: (id) => blocks.get(id),
      isHiddenCalloutLabel: (id) => id === "callout",
      requestFocus,
    });
    for (const [id, offset, key, targetId, targetOffset] of [
      ["before", 5, "ArrowDown", "child", 0],
      ["child", 0, "ArrowUp", "before", 5],
    ] as const) {
      const editor = list.querySelector<HTMLElement>(`[data-notes-block-id='${id}']`)!;
      document.getSelection()?.collapse(editor.firstChild, offset);
      const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
      Object.defineProperty(event, "target", { value: editor });
      expect(controller.handleKeydown(event, id)).toBe(true);
      expect(requestFocus).toHaveBeenLastCalledWith(targetId, { start: targetOffset, end: targetOffset });
    }
    const hiddenRow = list.querySelector<HTMLElement>("[data-notes-selectable-block-id='callout']")!;
    const hiddenRowEvent = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
    Object.defineProperty(hiddenRowEvent, "target", { value: hiddenRow });
    expect(controller.handleKeydown(hiddenRowEvent, "callout")).toBe(true);
    expect(requestFocus).toHaveBeenLastCalledWith("child", { start: 0, end: 0 });
    const boundaryController = createNotesBlockNavigationController({
      readListElement: () => list,
      readRenderedBlockIds: () => ["callout", "child"],
      readBlock: (id) => blocks.get(id),
      isHiddenCalloutLabel: (id) => id === "callout",
      requestFocus,
    });
    const home = new KeyboardEvent("keydown", { key: "Home", ctrlKey: true, bubbles: true, cancelable: true });
    Object.defineProperty(home, "target", { value: list.querySelector("[data-notes-block-id='child']") });
    expect(boundaryController.handleKeydown(home, "child")).toBe(true);
    expect(requestFocus).toHaveBeenLastCalledWith("child", { start: 0, end: 0 });
    expect(requestFocus).toHaveBeenCalledTimes(4);
  });

  it("keeps an arrow key within a wrapped block before the last visual line", () => {
    mockRangeGeometry({
      top: 100,
      left: 40,
      width: 0,
      height: 18,
    } as DOMRect, [
      { top: 100, bottom: 118, left: 40, right: 160, width: 120, height: 18 },
      { top: 120, bottom: 138, left: 40, right: 100, width: 60, height: 18 },
    ] as unknown as DOMRectList);
    const list = document.createElement("div");
    list.innerHTML = `
      <div data-notes-selectable-block-id="first"><div contenteditable="true" role="textbox" data-notes-block-id="first">ab</div></div>
      <div data-notes-selectable-block-id="second"><div contenteditable="true" role="textbox" data-notes-block-id="second"></div></div>
    `;
    document.body.append(list);
    const editor = list.querySelector<HTMLElement>("[data-notes-block-id='first']")!;
    const range = document.createRange();
    range.setStart(editor.firstChild!, 1);
    range.collapse(true);
    document.getSelection()?.addRange(range);
    const requestFocus = vi.fn();
    const controller = createNotesBlockNavigationController({
      readListElement: () => list,
      readRenderedBlockIds: () => ["first", "second"],
      readBlock: (id) => id === "first" || id === "second" ? paragraph(id) : undefined,
      isHiddenCalloutLabel: () => false,
      requestFocus,
    });
    const event = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
    Object.defineProperty(event, "target", { value: editor });
    expect(controller.handleKeydown(event, "first")).toBe(false);
    expect(event.defaultPrevented).toBe(false);
    expect(requestFocus).not.toHaveBeenCalled();
  });

  it("keeps the original visual column when moving through a short block", () => {
    const list = document.createElement("div");
    list.innerHTML = ["abcdefgh", "x", "abcdefgh"].map((text, index) =>
      `<div data-notes-selectable-block-id="row-${index}"><div contenteditable="true" role="textbox" data-notes-block-id="row-${index}">${text}</div></div>`,
    ).join("");
    document.body.append(list);
    const editors = Array.from(list.querySelectorAll<HTMLElement>("[contenteditable='true']"));
    editors.forEach((editor, index) => {
      vi.spyOn(editor, "getBoundingClientRect").mockReturnValue(new DOMRect(40, 100 + index * 40, 200, 20));
    });
    Object.defineProperty(Range.prototype, "getBoundingClientRect", { configurable: true,
      value: function (this: Range) {
        const index = editors.findIndex((editor) => editor.contains(this.startContainer));
        return new DOMRect(40 + this.startOffset * 10, 100 + index * 40, 0, 20);
      },
    });
    Object.defineProperty(Range.prototype, "getClientRects", { configurable: true,
      value: function (this: Range) {
        const index = editors.findIndex((editor) => editor.contains(this.startContainer));
        return [new DOMRect(40, 100 + index * 40, 80, 20)];
      },
    });
    Object.defineProperty(document, "caretPositionFromPoint", { configurable: true,
      value: (x: number, y: number) => {
        const index = editors.findIndex((_, candidate) => y >= 100 + candidate * 40 && y < 120 + candidate * 40);
        if (index < 0) return null;
        const length = editors[index].textContent?.length ?? 0;
        return { offsetNode: editors[index].firstChild, offset: Math.min(length, Math.max(0, Math.round((x - 40) / 10))) };
      },
    });
    const requestFocus = vi.fn();
    const blocks = new Map(editors.map((editor, index) => {
      const id = `row-${index}`;
      const block = paragraph(id);
      if (block.type !== "paragraph") throw new Error("Expected paragraph");
      block.paragraph.rich_text = [createRichText(editor.textContent ?? "")];
      return [id, block] as const;
    }));
    const controller = createNotesBlockNavigationController({
      readListElement: () => list,
      readRenderedBlockIds: () => ["row-0", "row-1", "row-2"],
      readBlock: (id) => blocks.get(id),
      isHiddenCalloutLabel: () => false,
      requestFocus,
    });
    const down = (index: number) => {
      const event = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
      Object.defineProperty(event, "target", { value: editors[index] });
      expect(controller.handleKeydown(event, `row-${index}`)).toBe(true);
    };
    document.getSelection()?.collapse(editors[0].firstChild, 4);
    down(0);
    expect(requestFocus).toHaveBeenLastCalledWith("row-1", { start: 1, end: 1 });
    document.getSelection()?.collapse(editors[1].firstChild, 1);
    down(1);
    expect(requestFocus).toHaveBeenLastCalledWith("row-2", { start: 4, end: 4 });
  });
});
