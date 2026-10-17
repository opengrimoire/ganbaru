import { describe, expect, it } from "vitest";
import {
  createInlineRenameHistory,
  inlineRenameHistoryAction,
  inlineRenameInputKind,
  inlineRenameHistoryValue,
  recordInlineRenameValue,
  stepInlineRenameHistory,
} from "./inline-rename-history";

describe("inline rename history", () => {
  it("undoes to the original name and redoes the typed value", () => {
    let history = createInlineRenameHistory("Folder");
    history = recordInlineRenameValue(history, "1", "insert", 0);
    history = stepInlineRenameHistory(history, "undo");
    expect(inlineRenameHistoryValue(history)).toBe("Folder");
    history = stepInlineRenameHistory(history, "redo");
    expect(inlineRenameHistoryValue(history)).toBe("1");
  });

  it("drops the redo branch after new input", () => {
    let history = createInlineRenameHistory("Note");
    history = recordInlineRenameValue(history, "Note 1", "insert", 0);
    history = recordInlineRenameValue(history, "Note 12", "insert", 1_500);
    history = stepInlineRenameHistory(history, "undo");
    history = recordInlineRenameValue(history, "Note 13", "insert", 1_600);

    expect(history.entries).toEqual(["Note", "Note 1", "Note 13"]);
    expect(stepInlineRenameHistory(history, "redo")).toBe(history);
  });

  it("coalesces continuous typing and deletion into separate undo steps", () => {
    let history = createInlineRenameHistory("Folder");
    history = recordInlineRenameValue(history, "a", "insert", 0);
    history = recordInlineRenameValue(history, "ab", "insert", 100);
    history = recordInlineRenameValue(history, "abc", "insert", 200);
    history = recordInlineRenameValue(history, "ab", "delete", 300);
    history = recordInlineRenameValue(history, "a", "delete", 400);

    expect(history.entries).toEqual(["Folder", "abc", "a"]);
    history = stepInlineRenameHistory(history, "undo");
    expect(inlineRenameHistoryValue(history)).toBe("abc");
    history = stepInlineRenameHistory(history, "undo");
    expect(inlineRenameHistoryValue(history)).toBe("Folder");
  });

  it("classifies browser input types", () => {
    expect(inlineRenameInputKind("insertText")).toBe("insert");
    expect(inlineRenameInputKind("deleteContentBackward")).toBe("delete");
    expect(inlineRenameInputKind("historyUndo")).toBe("replace");
  });

  it("recognizes undo and redo shortcuts", () => {
    const shortcut = (key: string, shiftKey = false) => ({
      key,
      shiftKey,
      ctrlKey: true,
      metaKey: false,
      altKey: false,
    });

    expect(inlineRenameHistoryAction(shortcut("z"))).toBe("undo");
    expect(inlineRenameHistoryAction(shortcut("z", true))).toBe("redo");
    expect(inlineRenameHistoryAction(shortcut("y"))).toBe("redo");
  });
});
