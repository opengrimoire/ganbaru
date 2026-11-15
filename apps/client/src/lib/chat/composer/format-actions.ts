import { hasOnlyShortcutModifier, hasShortcutModifier, type KeyboardModifierState } from "$lib/keyboard-shortcuts";
import type { ChatComposerLineFormat } from "./line-formats";
import type { ChatComposerMark } from "./rich-text";

/** One formatting change a Chat composer applies to its selection or caret line. */
export type ChatComposerFormatAction =
  | { kind: "mark"; mark: ChatComposerMark }
  | { kind: "line"; format: ChatComposerLineFormat };

export type ChatComposerFormatActionId = ChatComposerMark | ChatComposerLineFormat;

/** A formatting action with its shortcut in the `Mod + Key` notation of `formatShortcut`. */
export interface ChatComposerFormatActionDefinition {
  id: ChatComposerFormatActionId;
  action: ChatComposerFormatAction;
  shortcut: string;
  /** Toolbar group; the toolbar separates consecutive groups. */
  group: "inline" | "list" | "quote" | "code";
}

/**
 * The formatting actions in toolbar order, with Slack's message formatting shortcuts.
 *
 * Underline, headings, and dividers are absent because Chat messages are Markdown that has no
 * underline, and headings and dividers do not belong in a chat message.
 */
export const CHAT_COMPOSER_FORMAT_ACTIONS: readonly ChatComposerFormatActionDefinition[] = [
  { id: "bold", action: { kind: "mark", mark: "bold" }, shortcut: "Mod + B", group: "inline" },
  { id: "italic", action: { kind: "mark", mark: "italic" }, shortcut: "Mod + I", group: "inline" },
  { id: "strikethrough", action: { kind: "mark", mark: "strikethrough" }, shortcut: "Mod + Shift + X", group: "inline" },
  { id: "orderedList", action: { kind: "line", format: "orderedList" }, shortcut: "Mod + Shift + 7", group: "list" },
  { id: "bulletList", action: { kind: "line", format: "bulletList" }, shortcut: "Mod + Shift + 8", group: "list" },
  { id: "blockquote", action: { kind: "line", format: "blockquote" }, shortcut: "Mod + Shift + 9", group: "quote" },
  { id: "code", action: { kind: "mark", mark: "code" }, shortcut: "Mod + Shift + C", group: "code" },
  { id: "codeBlock", action: { kind: "line", format: "codeBlock" }, shortcut: "Mod + Alt + Shift + C", group: "code" },
];

type ShortcutEvent = KeyboardModifierState & Pick<KeyboardEvent, "key" | "code">;

/**
 * Resolves a keydown to a formatting action. Digits match the physical key because Shift
 * changes the produced character by layout, and the code block shortcut matches the physical
 * C key because Option changes the produced character on Apple platforms.
 */
export function chatComposerFormatShortcut(
  event: ShortcutEvent,
  platform?: string,
): ChatComposerFormatAction | null {
  const key = event.key.toLowerCase();
  if (hasOnlyShortcutModifier(event, {}, platform)) {
    if (key === "b") return { kind: "mark", mark: "bold" };
    if (key === "i") return { kind: "mark", mark: "italic" };
    return null;
  }
  if (hasOnlyShortcutModifier(event, { shift: true }, platform)) {
    if (key === "x") return { kind: "mark", mark: "strikethrough" };
    if (key === "c") return { kind: "mark", mark: "code" };
    if (event.code === "Digit7") return { kind: "line", format: "orderedList" };
    if (event.code === "Digit8") return { kind: "line", format: "bulletList" };
    if (event.code === "Digit9") return { kind: "line", format: "blockquote" };
    return null;
  }
  if (hasShortcutModifier(event, platform) && event.altKey && event.shiftKey && event.code === "KeyC") {
    return { kind: "line", format: "codeBlock" };
  }
  return null;
}
