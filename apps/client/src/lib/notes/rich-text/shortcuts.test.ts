import { describe, expect, it } from "vitest";
import {
  applyRichTextAnnotations,
  createTextRichText,
  planRichTextEquationConversion,
  richTextAnnotationTogglePatch,
  richTextAnnotationsForSelection,
  type NotesRichTextAnnotationName,
} from "./core";
import {
  notesRichTextEquationShortcutRequested,
  notesRichTextFormattingShortcutAnnotationName,
  notesRichTextLinkShortcutRequested,
  type NotesRichTextFormattingShortcutInput,
} from "./shortcuts";

interface FormattingShortcutCase {
  key: string;
  shiftKey: boolean;
  annotation: NotesRichTextAnnotationName;
}

const formattingShortcuts: readonly FormattingShortcutCase[] = [
  { key: "b", shiftKey: false, annotation: "bold" },
  { key: "i", shiftKey: false, annotation: "italic" },
  { key: "u", shiftKey: false, annotation: "underline" },
  { key: "s", shiftKey: true, annotation: "strikethrough" },
  { key: "e", shiftKey: false, annotation: "code" },
];

function shortcutInput(
  shortcut: FormattingShortcutCase,
  modifier: "ctrl" | "meta",
): NotesRichTextFormattingShortcutInput {
  return {
    key: shortcut.key,
    shiftKey: shortcut.shiftKey,
    ctrlKey: modifier === "ctrl",
    metaKey: modifier === "meta",
    altKey: false,
  };
}

function applyShortcutOnce(shortcut: FormattingShortcutCase) {
  const source = [createTextRichText("Format me")];
  const name = notesRichTextFormattingShortcutAnnotationName(shortcutInput(shortcut, "ctrl"));
  if (!name) throw new Error(`Shortcut ${shortcut.key} was not recognized`);
  const range = richTextAnnotationsForSelection(source, 0, 9);
  return applyRichTextAnnotations(
    source,
    0,
    9,
    richTextAnnotationTogglePatch(range.annotations, name),
  );
}

describe("notes rich text formatting shortcuts", () => {
  it.each(formattingShortcuts)(
    "maps Ctrl+$key to $annotation",
    (shortcut) => {
      expect(notesRichTextFormattingShortcutAnnotationName(shortcutInput(shortcut, "ctrl")))
        .toBe(shortcut.annotation);
    },
  );

  it.each(formattingShortcuts)(
    "maps Cmd+$key to $annotation",
    (shortcut) => {
      expect(notesRichTextFormattingShortcutAnnotationName(shortcutInput(shortcut, "meta")))
        .toBe(shortcut.annotation);
    },
  );

  it.each(formattingShortcuts)(
    "applies and removes $annotation through the selected rich text range",
    (shortcut) => {
      const applied = applyShortcutOnce(shortcut);
      const appliedItem = applied[0];
      expect(appliedItem?.type).toBe("text");
      if (appliedItem?.type !== "text") return;
      expect(appliedItem.annotations[shortcut.annotation]).toBe(true);

      const name = notesRichTextFormattingShortcutAnnotationName(shortcutInput(shortcut, "ctrl"));
      if (!name) throw new Error(`Shortcut ${shortcut.key} was not recognized`);
      const range = richTextAnnotationsForSelection(applied, 0, 9);
      const removed = applyRichTextAnnotations(
        applied,
        0,
        9,
        richTextAnnotationTogglePatch(range.annotations, name),
      );
      const removedItem = removed[0];
      expect(removedItem?.type).toBe("text");
      if (removedItem?.type !== "text") return;
      expect(removedItem.annotations[shortcut.annotation]).toBe(false);
    },
  );

  it("ignores shortcuts without Ctrl or Cmd", () => {
    expect(
      notesRichTextFormattingShortcutAnnotationName({
        key: "b",
        ctrlKey: false,
        metaKey: false,
        shiftKey: false,
        altKey: false,
      }),
    ).toBeNull();
  });

  it("ignores Alt-modified formatting shortcuts", () => {
    expect(
      notesRichTextFormattingShortcutAnnotationName({
        key: "b",
        ctrlKey: true,
        metaKey: false,
        shiftKey: false,
        altKey: true,
      }),
    ).toBeNull();
  });
});

describe("notes rich text link shortcuts", () => {
  it("maps Ctrl+K and Cmd+K to link editing", () => {
    expect(
      notesRichTextLinkShortcutRequested({
        key: "k",
        ctrlKey: true,
        metaKey: false,
        shiftKey: false,
        altKey: false,
      }),
    ).toBe(true);
    expect(
      notesRichTextLinkShortcutRequested({
        key: "k",
        ctrlKey: false,
        metaKey: true,
        shiftKey: false,
        altKey: false,
      }),
    ).toBe(true);
  });

  it("ignores modified or unrelated link shortcuts", () => {
    expect(
      notesRichTextLinkShortcutRequested({
        key: "k",
        ctrlKey: true,
        metaKey: false,
        shiftKey: true,
        altKey: false,
      }),
    ).toBe(false);
    expect(
      notesRichTextLinkShortcutRequested({
        key: "k",
        ctrlKey: true,
        metaKey: false,
        shiftKey: false,
        altKey: true,
      }),
    ).toBe(false);
    expect(
      notesRichTextLinkShortcutRequested({
        key: "b",
        ctrlKey: true,
        metaKey: false,
        shiftKey: false,
        altKey: false,
      }),
    ).toBe(false);
  });
});

describe("notes rich text equation shortcuts", () => {
  it("maps Ctrl+Shift+E and Cmd+Shift+E to inline equation conversion", () => {
    expect(
      notesRichTextEquationShortcutRequested({
        key: "e",
        ctrlKey: true,
        metaKey: false,
        shiftKey: true,
        altKey: false,
      }),
    ).toBe(true);
    expect(
      notesRichTextEquationShortcutRequested({
        key: "e",
        ctrlKey: false,
        metaKey: true,
        shiftKey: true,
        altKey: false,
      }),
    ).toBe(true);
  });

  it("ignores unmodified, Alt-modified, or unrelated equation shortcuts", () => {
    expect(
      notesRichTextEquationShortcutRequested({
        key: "e",
        ctrlKey: true,
        metaKey: false,
        shiftKey: false,
        altKey: false,
      }),
    ).toBe(false);
    expect(
      notesRichTextEquationShortcutRequested({
        key: "e",
        ctrlKey: true,
        metaKey: false,
        shiftKey: true,
        altKey: true,
      }),
    ).toBe(false);
    expect(
      notesRichTextEquationShortcutRequested({
        key: "k",
        ctrlKey: true,
        metaKey: false,
        shiftKey: true,
        altKey: false,
      }),
    ).toBe(false);
  });

  it("plans the Ctrl+Shift+E conversion flow for selected text", () => {
    const input: NotesRichTextFormattingShortcutInput = {
      key: "e",
      ctrlKey: true,
      metaKey: false,
      shiftKey: true,
      altKey: false,
    };

    expect(notesRichTextEquationShortcutRequested(input)).toBe(true);
    expect(planRichTextEquationConversion("Use  e=mc^2  here", 4, 12)).toEqual({
      type: "convert",
      start: 4,
      end: 12,
      expression: "e=mc^2",
      cursor: 10,
    });
  });
});
