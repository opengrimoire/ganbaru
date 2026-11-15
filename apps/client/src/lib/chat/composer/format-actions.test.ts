import { describe, expect, it } from "vitest";
import { CHAT_COMPOSER_FORMAT_ACTIONS, chatComposerFormatShortcut } from "./format-actions";

const LINUX = "Linux x86_64";
const MAC = "MacIntel";

function key(
  keyValue: string,
  code: string,
  modifiers: Partial<{ ctrlKey: boolean; metaKey: boolean; shiftKey: boolean; altKey: boolean }> = {},
) {
  return { key: keyValue, code, ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...modifiers };
}

describe("Chat composer format shortcuts", () => {
  it("resolves every listed shortcut on Linux and Windows with Ctrl", () => {
    expect(chatComposerFormatShortcut(key("b", "KeyB", { ctrlKey: true }), LINUX)).toEqual({ kind: "mark", mark: "bold" });
    expect(chatComposerFormatShortcut(key("i", "KeyI", { ctrlKey: true }), LINUX)).toEqual({ kind: "mark", mark: "italic" });
    expect(chatComposerFormatShortcut(key("X", "KeyX", { ctrlKey: true, shiftKey: true }), LINUX))
      .toEqual({ kind: "mark", mark: "strikethrough" });
    expect(chatComposerFormatShortcut(key("C", "KeyC", { ctrlKey: true, shiftKey: true }), LINUX))
      .toEqual({ kind: "mark", mark: "code" });
    expect(chatComposerFormatShortcut(key("&", "Digit7", { ctrlKey: true, shiftKey: true }), LINUX))
      .toEqual({ kind: "line", format: "orderedList" });
    expect(chatComposerFormatShortcut(key("*", "Digit8", { ctrlKey: true, shiftKey: true }), LINUX))
      .toEqual({ kind: "line", format: "bulletList" });
    expect(chatComposerFormatShortcut(key(")", "Digit9", { ctrlKey: true, shiftKey: true }), LINUX))
      .toEqual({ kind: "line", format: "blockquote" });
    expect(chatComposerFormatShortcut(key("C", "KeyC", { ctrlKey: true, shiftKey: true, altKey: true }), LINUX))
      .toEqual({ kind: "line", format: "codeBlock" });
  });

  it("uses Command on Apple platforms and matches the physical C key under Option", () => {
    expect(chatComposerFormatShortcut(key("b", "KeyB", { metaKey: true }), MAC)).toEqual({ kind: "mark", mark: "bold" });
    expect(chatComposerFormatShortcut(key("b", "KeyB", { ctrlKey: true }), MAC)).toBeNull();
    expect(chatComposerFormatShortcut(key("Ç", "KeyC", { metaKey: true, shiftKey: true, altKey: true }), MAC))
      .toEqual({ kind: "line", format: "codeBlock" });
  });

  it("ignores Enter, send, steer, and stop shortcuts", () => {
    expect(chatComposerFormatShortcut(key("Enter", "Enter"), LINUX)).toBeNull();
    expect(chatComposerFormatShortcut(key("Enter", "Enter", { ctrlKey: true }), LINUX)).toBeNull();
    expect(chatComposerFormatShortcut(key("Enter", "Enter", { shiftKey: true }), LINUX)).toBeNull();
    expect(chatComposerFormatShortcut(key(".", "Period", { ctrlKey: true }), LINUX)).toBeNull();
    expect(chatComposerFormatShortcut(key("B", "KeyB", { ctrlKey: true, shiftKey: true }), LINUX)).toBeNull();
    expect(chatComposerFormatShortcut(key("c", "KeyC", { ctrlKey: true }), LINUX)).toBeNull();
  });

  it("lists a distinct shortcut for every toolbar action", () => {
    const shortcuts = CHAT_COMPOSER_FORMAT_ACTIONS.map((action) => action.shortcut);

    expect(new Set(shortcuts).size).toBe(shortcuts.length);
  });
});
