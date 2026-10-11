// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import {
  clampNotesTextSelection,
  markNotesSelectedMentionChips,
  notesMentionChipSelectionAction,
  createNotesDocumentSelectionPainter,
  notesEditableOffsetFromDomPoint,
  notesEditableSelectionViewportRect,
  notesDirectionalSelectionFromEditableRoot,
  notesDocumentRange,
  notesPlainTextFromEditableRoot,
  notesSelectionForFocus,
  notesTextSelectionFromEditableRoot,
  notesTextSelectionFromControl,
  planNotesSelectionReconciliation,
  restoreNotesEditableSelection,
  setNotesEditableDirectionalSelection,
} from "./selection";

// Load test-only Node APIs without introducing Node timer globals into the browser type environment.
const { readFileSync } = await vi.importActual<{
  readFileSync: (path: string, encoding: "utf8") => string;
}>("node:fs");
const { fileURLToPath, URL: NodeUrl } = await vi.importActual<{
  URL: typeof URL;
  fileURLToPath: (url: string) => string;
}>("node:url");
const appStyles = readFileSync(fileURLToPath(new NodeUrl("../../../app.css", import.meta.url).href), "utf8");

describe("notes editor selection helpers", () => {
  it.each([false, true])("excludes a row reached only at offset zero from row commands (backward: %s)", (backward) => {
    const start = { blockId: "b", offset: 0 };
    const end = { blockId: "d", offset: 0 };
    expect(notesDocumentRange(["a", "b", "c", "d"], {
      anchor: backward ? end : start, focus: backward ? start : end,
    })).toEqual({ start, end, blockIds: ["b", "c", "d"], rowBlockIds: ["b", "c"] });
  });

  it("keeps partially selected endpoint rows and explicit whole-document endpoints", () => {
    for (const offset of [1, Number.MAX_SAFE_INTEGER]) {
      expect(notesDocumentRange(["a", "b", "c"], {
        anchor: { blockId: "a", offset: 0 }, focus: { blockId: "c", offset },
      })?.rowBlockIds).toEqual(["a", "b", "c"]);
    }
    expect(notesDocumentRange(["a"], {
      anchor: { blockId: "a", offset: 0 }, focus: { blockId: "a", offset: 0 },
    })?.rowBlockIds).toEqual(["a"]);
  });

  it("lets a new explicit caret request replace the previous selected range", () => {
    expect(
      planNotesSelectionReconciliation({
        focusRequestIsNew: true,
        focusRequestedForEditor: true,
        requestedSelection: { start: 8, end: 8 },
        currentSelection: { start: 2, end: 5 },
        textLength: 12,
        editorActive: true,
      }),
    ).toEqual({
      focusEditor: true,
      selection: { start: 8, end: 8 },
    });
  });

  it("preserves the collapsed caret after the explicit request has been applied", () => {
    expect(
      planNotesSelectionReconciliation({
        focusRequestIsNew: false,
        focusRequestedForEditor: true,
        requestedSelection: { start: 8, end: 8 },
        currentSelection: { start: 8, end: 8 },
        textLength: 12,
        editorActive: true,
      }),
    ).toEqual({
      focusEditor: false,
      selection: { start: 8, end: 8 },
    });
  });

  it("normalizes reversed control selections", () => {
    expect(notesTextSelectionFromControl({ selectionStart: 8, selectionEnd: 3 })).toEqual({
      start: 3,
      end: 8,
    });
  });

  it("treats missing selection ends as a collapsed selection", () => {
    expect(notesTextSelectionFromControl({ selectionStart: 4, selectionEnd: null })).toEqual({
      start: 4,
      end: 4,
    });
  });

  it("clamps selections inside the current text length", () => {
    expect(clampNotesTextSelection({ start: -2, end: 12 }, 5)).toEqual({
      start: 0,
      end: 5,
    });
  });

  it("prefers requested focus selections over remembered editor selections", () => {
    expect(notesSelectionForFocus({
      requestedSelection: { start: 2, end: 5 },
      currentSelection: { start: 8, end: 8 },
      textLength: 10,
      fallback: "end",
    })).toEqual({ start: 2, end: 5 });
  });

  it("restores remembered selections instead of falling back to the text end", () => {
    expect(notesSelectionForFocus({
      requestedSelection: null,
      currentSelection: { start: 3, end: 7 },
      textLength: 12,
      fallback: "end",
    })).toEqual({ start: 3, end: 7 });
  });

  it("uses explicit start or end fallbacks only when no selection is known", () => {
    expect(notesSelectionForFocus({
      requestedSelection: null,
      currentSelection: null,
      textLength: 12,
      fallback: "start",
    })).toEqual({ start: 0, end: 0 });
    expect(notesSelectionForFocus({
      requestedSelection: null,
      currentSelection: null,
      textLength: 12,
      fallback: "end",
    })).toEqual({ start: 12, end: 12 });
  });

  it("clamps remembered focus selections to reloaded text bounds", () => {
    expect(notesSelectionForFocus({
      requestedSelection: null,
      currentSelection: { start: 4, end: 20 },
      textLength: 9,
      fallback: "end",
    })).toEqual({ start: 4, end: 9 });
  });

  it("reads plain text from a rich editable surface without keeping markup", () => {
    const root = document.createElement("div");
    root.innerHTML = "<span>Hello</span><br><span>world</span>";

    expect(notesPlainTextFromEditableRoot(root)).toBe("Hello\nworld");
  });

  it("reads soft line wrappers as plain text newlines", () => {
    const root = document.createElement("div");
    root.innerHTML = [
      "<div data-notes-editor-line=\"true\"><span>Hello</span></div>",
      "<div data-notes-editor-line=\"true\"><span>world</span></div>",
    ].join("");

    expect(notesPlainTextFromEditableRoot(root)).toBe("Hello\nworld");
  });

  it("keeps trailing text-node soft line breaks", () => {
    const root = document.createElement("div");
    const span = document.createElement("span");
    span.textContent = "Hello\n";
    root.append(span);

    expect(notesPlainTextFromEditableRoot(root)).toBe("Hello\n");
  });

  it("ignores trailing line sentinels while preserving the soft line break", () => {
    const root = document.createElement("div");
    root.innerHTML = [
      "<span>Hello\n</span>",
      "<span data-notes-editor-sentinel=\"trailing-line\">&#8203;</span>",
    ].join("");

    expect(notesPlainTextFromEditableRoot(root)).toBe("Hello\n");
  });

  it("keeps sentinel-backed soft line breaks without visible text", () => {
    const root = document.createElement("div");
    root.innerHTML = [
      "<div data-notes-editor-line=\"true\"><span data-notes-editor-sentinel=\"empty-line\">&#8203;</span></div>",
      "<div data-notes-editor-line=\"true\"><span data-notes-editor-sentinel=\"empty-line\">&#8203;</span></div>",
    ].join("");

    expect(notesPlainTextFromEditableRoot(root)).toBe("\n");
  });

  it("maps trailing line sentinels to the previous text offset", () => {
    const root = document.createElement("div");
    root.innerHTML = [
      "<span>Hello\n</span>",
      "<span data-notes-editor-sentinel=\"trailing-line\">&#8203;</span>",
    ].join("");
    const sentinel = root.querySelector("[data-notes-editor-sentinel]");
    expect(sentinel).toBeInstanceOf(HTMLElement);
    if (!(sentinel instanceof HTMLElement)) return;

    expect(notesEditableOffsetFromDomPoint(root, sentinel.firstChild ?? sentinel, 1)).toBe(6);
  });

  it("maps explicit trailing line sentinels to the previous text offset", () => {
    const root = document.createElement("div");
    root.innerHTML = [
      "<span>Hello</span>",
      "<br>",
      "<span data-notes-editor-sentinel=\"trailing-line\">&#8203;</span>",
    ].join("");
    const sentinel = root.querySelector("[data-notes-editor-sentinel]");
    expect(sentinel).toBeInstanceOf(HTMLElement);
    if (!(sentinel instanceof HTMLElement)) return;

    expect(notesEditableOffsetFromDomPoint(root, sentinel.firstChild ?? sentinel, 1)).toBe(6);
  });

  it("maps DOM points around soft line breaks to plain text offsets", () => {
    const root = document.createElement("div");
    root.innerHTML = "<span>Hello</span><br><span>world</span>";
    const firstText = root.querySelector("span")?.firstChild;
    const secondText = root.querySelectorAll("span")[1]?.firstChild;
    expect(firstText).toBeInstanceOf(Text);
    expect(secondText).toBeInstanceOf(Text);
    if (!(firstText instanceof Text) || !(secondText instanceof Text)) return;

    expect(notesEditableOffsetFromDomPoint(root, firstText, 5)).toBe(5);
    expect(notesEditableOffsetFromDomPoint(root, root, 2)).toBe(6);
    expect(notesEditableOffsetFromDomPoint(root, secondText, 0)).toBe(6);
  });

  it("maps DOM points around line-wrapped soft breaks to plain text offsets", () => {
    const root = document.createElement("div");
    root.innerHTML = [
      "<div data-notes-editor-line=\"true\"><span>Hello</span></div>",
      "<div data-notes-editor-line=\"true\"><span>world</span></div>",
    ].join("");
    const firstText = root.querySelector("span")?.firstChild;
    const secondText = root.querySelectorAll("span")[1]?.firstChild;
    expect(firstText).toBeInstanceOf(Text);
    expect(secondText).toBeInstanceOf(Text);
    if (!(firstText instanceof Text) || !(secondText instanceof Text)) return;

    expect(notesEditableOffsetFromDomPoint(root, firstText, 5)).toBe(5);
    expect(notesEditableOffsetFromDomPoint(root, secondText, 0)).toBe(6);
  });

  it("treats browser filler markup in an empty editable surface as empty text", () => {
    const root = document.createElement("div");

    root.innerHTML = "<br>";
    expect(notesPlainTextFromEditableRoot(root)).toBe("");

    root.innerHTML = "<span></span>";
    expect(notesPlainTextFromEditableRoot(root)).toBe("");

    root.innerHTML = "&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;";
    expect(notesPlainTextFromEditableRoot(root)).toBe("");

    root.innerHTML = "<span data-notes-editor-sentinel=\"empty-line\">&#8203;</span>";
    expect(notesPlainTextFromEditableRoot(root)).toBe("");

    root.innerHTML = "<div><br></div>";
    expect(notesPlainTextFromEditableRoot(root)).toBe("");
  });

  it("normalizes editable block wrappers as line breaks", () => {
    const root = document.createElement("div");
    root.innerHTML = "<div>First</div><div><span>Second</span></div>";

    expect(notesPlainTextFromEditableRoot(root)).toBe("First\nSecond");
  });

  it("maps rich editable selections to plain text offsets", () => {
    const root = document.createElement("div");
    root.innerHTML = "<span>Hello </span><span><strong>world</strong></span>";
    document.body.append(root);

    const firstText = root.querySelector("span")?.firstChild;
    const strongText = root.querySelector("strong")?.firstChild;
    expect(firstText).toBeInstanceOf(Text);
    expect(strongText).toBeInstanceOf(Text);
    if (!(firstText instanceof Text) || !(strongText instanceof Text)) return;

    const range = document.createRange();
    range.setStart(firstText, 3);
    range.setEnd(strongText, 2);
    const selection = document.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);

    expect(notesTextSelectionFromEditableRoot(root)).toEqual({ start: 3, end: 8 });
    root.remove();
  });

  it("restores rich editable selections from plain text offsets", () => {
    const root = document.createElement("div");
    root.innerHTML = "<span>Hello </span><span><em>world</em></span>";
    document.body.append(root);

    expect(restoreNotesEditableSelection(root, { start: 6, end: 11 })).toBe(true);
    expect(notesTextSelectionFromEditableRoot(root)).toEqual({ start: 6, end: 11 });
    root.remove();
  });

  it("restores selections after a trailing soft line break", () => {
    const root = document.createElement("div");
    root.innerHTML = [
      "<span>Hello\n</span>",
      "<span data-notes-editor-sentinel=\"trailing-line\">&#8203;</span>",
    ].join("");
    document.body.append(root);

    expect(restoreNotesEditableSelection(root, { start: 6, end: 6 })).toBe(true);
    expect(notesTextSelectionFromEditableRoot(root)).toEqual({ start: 6, end: 6 });
    root.remove();
  });

  it("restores selections after line-wrapped soft breaks", () => {
    const root = document.createElement("div");
    root.innerHTML = [
      "<div data-notes-editor-line=\"true\"><span>Hello</span></div>",
      "<div data-notes-editor-line=\"true\"><span>world</span></div>",
    ].join("");
    document.body.append(root);

    expect(restoreNotesEditableSelection(root, { start: 6, end: 6 })).toBe(true);
    expect(notesTextSelectionFromEditableRoot(root)).toEqual({ start: 6, end: 6 });
    const selection = document.getSelection();
    expect(selection?.anchorNode).toBe(root.querySelectorAll("span")[1]?.firstChild);
    expect(selection?.anchorOffset).toBe(0);
    root.remove();
  });

  it("restores selections inside line-wrapped empty soft lines", () => {
    const root = document.createElement("div");
    root.innerHTML = [
      "<div data-notes-editor-line=\"true\"><span>Hello</span></div>",
      "<div data-notes-editor-line=\"true\"><span data-notes-editor-sentinel=\"empty-line\">&#8203;</span></div>",
    ].join("");
    document.body.append(root);

    expect(restoreNotesEditableSelection(root, { start: 6, end: 6 })).toBe(true);
    expect(notesTextSelectionFromEditableRoot(root)).toEqual({ start: 6, end: 6 });
    const sentinel = root.querySelector("[data-notes-editor-sentinel=\"empty-line\"]");
    const selection = document.getSelection();
    expect(sentinel).toBeInstanceOf(HTMLElement);
    if (!(sentinel instanceof HTMLElement)) return;
    expect(selection?.anchorNode).toBe(sentinel.firstChild);
    expect(selection?.anchorOffset).toBe(0);
    root.remove();
  });

  it("restores remembered selections after rich editable markup is replaced", () => {
    const root = document.createElement("div");
    root.innerHTML = "<span>Alpha </span><span><strong>beta</strong></span>";
    document.body.append(root);

    const rememberedSelection = { start: 2, end: 10 };
    root.innerHTML = "<span>Alpha </span><span class=\"mention\">beta</span>";

    expect(restoreNotesEditableSelection(root, rememberedSelection)).toBe(true);
    expect(notesTextSelectionFromEditableRoot(root)).toEqual(rememberedSelection);
    root.remove();
  });

  it("reads a viewport rectangle from a non-collapsed editable selection", () => {
    const root = document.createElement("div");
    root.textContent = "Selected text";
    document.body.append(root);
    const textNode = root.firstChild;
    expect(textNode).toBeInstanceOf(Text);
    if (!(textNode instanceof Text)) return;

    const range = document.createRange();
    range.setStart(textNode, 0);
    range.setEnd(textNode, 8);
    const rect = {
      top: 10,
      right: 90,
      bottom: 30,
      left: 20,
      width: 70,
      height: 20,
    } as DOMRect;
    Object.defineProperty(range, "getBoundingClientRect", { value: () => rect });
    const selection = document.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);

    expect(notesEditableSelectionViewportRect(root)).toEqual({
      top: 10,
      right: 90,
      bottom: 30,
      left: 20,
      width: 70,
      height: 20,
    });
    root.remove();
  });
});


describe("Notes selection overlay fallback", () => {
  it("paints text rectangles without modifying editor content and merges overlapping inline boxes", () => {
    const list = document.createElement("div");
    list.innerHTML = '<div contenteditable="true"><strong>First</strong> second</div>';
    document.body.append(list);
    const root = list.firstElementChild!;
    const original = root.innerHTML;
    const range = document.createRange();
    range.selectNodeContents(root);
    Object.defineProperty(range, "getClientRects", { value: () => [
      new DOMRect(10, 20, 40, 16), new DOMRect(10, 20, 40, 16),
      new DOMRect(50, 20, 30, 16), new DOMRect(10, 40, 20, 16),
    ] });
    const painter = createNotesDocumentSelectionPainter(list);
    painter.paint([range]);
    const overlay = list.querySelector<HTMLElement>("[data-notes-selection-overlay]")!;
    expect(overlay.getAttribute("aria-hidden")).toBe("true");
    expect(overlay.style.pointerEvents).toBe("none");
    expect(overlay.children).toHaveLength(2);
    expect((overlay.firstElementChild as HTMLElement).style.width).toBe("70px");
    expect(root.innerHTML).toBe(original);
    painter.paint([]);
    expect(list.querySelector("[data-notes-selection-overlay]")).toBeNull();
    expect(list.hasAttribute("data-notes-painted-selection")).toBe(false);
    list.remove();
  });
});


describe("Notes selection highlight ownership", () => {
  it.each(["custom", "overlay"])("avoids stacking native and %s highlights and restores native selection on cleanup", (mode) => {
    vi.stubGlobal("CSS", { highlights: new Map<string, object>() });
    vi.stubGlobal("Highlight", mode === "custom" ? class {} : undefined);
    const list = document.createElement("div");
    list.innerHTML = '<div contenteditable="true"><strong>First</strong></div><div contenteditable="true">Second</div>';
    document.body.append(list);
    const range = document.createRange();
    range.selectNodeContents(list.firstElementChild!);
    Object.defineProperty(range, "getClientRects", { value: () => [new DOMRect(0, 0, 40, 20)] });
    const native = window.getSelection()!;
    native.removeAllRanges();
    native.addRange(range);
    const painter = createNotesDocumentSelectionPainter(list);
    try {
      painter.paint([range]);
      const name = list.getAttribute("data-notes-painted-selection");
      expect(name).toBeTruthy();
      const scope = `[data-notes-painted-selection="${name}"]`;
      const rule = [...document.styleSheets].flatMap((sheet) => [...sheet.cssRules])
        .find((rule) => rule instanceof CSSStyleRule && rule.selectorText === `${scope}::selection, ${scope} *::selection`);
      expect(rule).toBeInstanceOf(CSSStyleRule);
      expect((rule as CSSStyleRule).style.getPropertyValue("background-color")).toBe("transparent");
      expect(native.toString()).toBe("First");
      painter.paint([]);
      expect(list.hasAttribute("data-notes-painted-selection")).toBe(false);
      expect([...document.styleSheets].flatMap((sheet) => [...sheet.cssRules])).not.toContain(rule);
      expect(native.toString()).toBe("First");
    } finally {
      painter.clear();
      list.remove();
      native.removeAllRanges();
      vi.unstubAllGlobals();
    }
  });

  it("retains native highlighting when the overlay has no visible text rectangles", () => {
    vi.stubGlobal("Highlight", undefined);
    const list = document.createElement("div");
    list.textContent = "Hidden text";
    const range = document.createRange();
    range.selectNodeContents(list);
    Object.defineProperty(range, "getClientRects", { value: () => [] });
    const painter = createNotesDocumentSelectionPainter(list);
    try {
      painter.paint([range]);
      expect(list.hasAttribute("data-notes-painted-selection")).toBe(false);
      expect(list.querySelector("[data-notes-selection-overlay]")).toBeNull();
    } finally {
      painter.clear();
      vi.unstubAllGlobals();
    }
  });
});


describe("Notes highlight theme colors", () => {
  it.each(["custom", "overlay"])("uses a valid visible theme color in the %s painter", (mode) => {
    const selectionColors = [...appStyles.matchAll(/--selection-background:\s*([^;]+);/gu)].map((match) => match[1]);
    const primaryColors = [...appStyles.matchAll(/--primary:\s*([^;]+);/gu)].map((match) => match[1]);
    expect(selectionColors.length).toBeGreaterThanOrEqual(2);
    const registry = new Map<string, object>();
    vi.stubGlobal("CSS", { highlights: registry });
    vi.stubGlobal("Highlight", mode === "custom" ? class {} : undefined);
    const list = document.createElement("div");
    list.textContent = "Selected text";
    document.body.append(list);
    const painter = createNotesDocumentSelectionPainter(list);
    try {
      for (const [index, color] of selectionColors.entries()) {
        list.style.setProperty("--primary", primaryColors[index]);
        list.style.setProperty("--selection-background", color);
        const range = document.createRange();
        range.selectNodeContents(list.firstChild!);
        Object.defineProperty(range, "getClientRects", { value: () => [new DOMRect(0, 0, 100, 20)] });
        painter.paint([range]);
        const name = [...registry.keys()][0];
        const rule = [...document.styleSheets].flatMap((sheet) => [...sheet.cssRules])
          .find((candidate) => candidate instanceof CSSStyleRule && candidate.selectorText === `::highlight(${name})`);
        const declaration = mode === "custom" && rule instanceof CSSStyleRule
          ? rule.style.getPropertyValue("background-color")
          : list.querySelector<HTMLElement>("[data-notes-selection-overlay] span")?.style.backgroundColor ?? "";
        // Resolve the declaration against real light/dark theme values before CSS parsing.
        const resolved = declaration.replace(/var\((--[\w-]+)(?:,\s*([^)]*))?\)/gu,
          (_match: string, token: string, fallback: string | undefined) => list.style.getPropertyValue(token) || fallback || "");
        const actual = document.createElement("span");
        const expected = document.createElement("span");
        actual.style.backgroundColor = resolved;
        expected.style.backgroundColor = color;
        expect(actual.style.backgroundColor).not.toBe("");
        expect(actual.style.backgroundColor).toBe(expected.style.backgroundColor);
        painter.clear();
      }
    } finally {
      painter.clear();
      list.remove();
      vi.unstubAllGlobals();
    }
  });
});

describe("notes mention chip selection", () => {
  function chipRoot(): HTMLElement {
    const root = document.createElement("div");
    root.innerHTML = [
      "<div data-notes-editor-line=\"true\">",
      "<a data-notes-mention-chip=\"true\"><span data-notes-editor-sentinel=\"decoration\">📁</span>Eating</a>",
      "<span> plan</span>",
      "</div>",
    ].join("");
    document.body.append(root);
    return root;
  }

  it("reads mention text without its icon decoration", () => {
    const root = chipRoot();
    expect(notesPlainTextFromEditableRoot(root)).toBe("Eating plan");
    root.remove();
  });

  it("places carets at mention edges outside the chip element", () => {
    const root = chipRoot();
    const chip = root.querySelector("a");
    const line = chip?.parentNode;
    expect(restoreNotesEditableSelection(root, { start: 0, end: 0 })).toBe(true);
    expect(document.getSelection()?.anchorNode).toBe(line);
    expect(document.getSelection()?.anchorOffset).toBe(0);
    expect(restoreNotesEditableSelection(root, { start: 6, end: 6 })).toBe(true);
    expect(document.getSelection()?.anchorNode).toBe(line);
    expect(document.getSelection()?.anchorOffset).toBe(1);
    expect(notesTextSelectionFromEditableRoot(root)).toEqual({ start: 6, end: 6 });
    root.remove();
  });

  it("keeps a backward selection's anchor and focus when placing it", () => {
    const root = chipRoot();
    expect(setNotesEditableDirectionalSelection(root, { anchor: 11, focus: 0 })).toBe(true);
    expect(notesDirectionalSelectionFromEditableRoot(root)).toEqual({ anchor: 11, focus: 0 });
    expect(notesTextSelectionFromEditableRoot(root)).toEqual({ start: 0, end: 11 });
    root.remove();
  });

  it("marks a chip only while a ranged selection covers part of it", () => {
    const root = chipRoot();
    const chip = root.querySelector("a");
    restoreNotesEditableSelection(root, { start: 4, end: 9 });
    markNotesSelectedMentionChips(root);
    expect(chip?.getAttribute("data-notes-selected")).toBe("true");
    restoreNotesEditableSelection(root, { start: 6, end: 11 });
    markNotesSelectedMentionChips(root);
    expect(chip?.hasAttribute("data-notes-selected")).toBe(false);
    restoreNotesEditableSelection(root, { start: 0, end: 6 });
    markNotesSelectedMentionChips(root);
    expect(chip?.getAttribute("data-notes-selected")).toBe("true");
    restoreNotesEditableSelection(root, { start: 3, end: 3 });
    markNotesSelectedMentionChips(root);
    expect(chip?.hasAttribute("data-notes-selected")).toBe(false);
    root.remove();
  });

  it("clears chip marks once the selection leaves the editor", () => {
    const root = chipRoot();
    const outside = document.createElement("p");
    outside.textContent = "Elsewhere";
    document.body.append(outside);
    const action = notesMentionChipSelectionAction(root);
    restoreNotesEditableSelection(root, { start: 0, end: 11 });
    action.update();
    expect(root.querySelector("a")?.getAttribute("data-notes-selected")).toBe("true");
    document.getSelection()?.selectAllChildren(outside);
    action.update();
    expect(root.querySelector("a")?.hasAttribute("data-notes-selected")).toBe(false);
    action.destroy();
    outside.remove();
    root.remove();
  });
});
