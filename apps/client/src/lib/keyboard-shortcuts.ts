export const SHORTCUT_MODIFIER_TOKEN = "Mod";

export interface KeyboardModifierState {
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}

interface ShortcutModifierOptions {
  shift?: boolean;
}

function currentPlatform(): string {
  return typeof navigator === "undefined" ? "" : navigator.platform;
}

export function isApplePlatform(platform: string = currentPlatform()): boolean {
  return /^(Mac|iPhone|iPad|iPod)/i.test(platform);
}

export function shortcutModifierLabel(platform: string = currentPlatform()): "Ctrl" | "Cmd" {
  return isApplePlatform(platform) ? "Cmd" : "Ctrl";
}

export function shortcutParts(
  shortcut: string,
  platform: string = currentPlatform(),
): string[] {
  const modLabel = shortcutModifierLabel(platform);
  return shortcut.split(" + ").map((part) =>
    part === SHORTCUT_MODIFIER_TOKEN ? modLabel : part,
  );
}

export function formatShortcut(
  shortcut: string,
  platform: string = currentPlatform(),
): string {
  return shortcutParts(shortcut, platform).join(" + ");
}

export function hasShortcutModifier(
  event: Pick<KeyboardModifierState, "ctrlKey" | "metaKey">,
  platform: string = currentPlatform(),
): boolean {
  return isApplePlatform(platform)
    ? event.metaKey && !event.ctrlKey
    : event.ctrlKey && !event.metaKey;
}

export function hasOnlyShortcutModifier(
  event: KeyboardModifierState,
  options: ShortcutModifierOptions = {},
  platform: string = currentPlatform(),
): boolean {
  return hasShortcutModifier(event, platform)
    && !event.altKey
    && event.shiftKey === (options.shift ?? false);
}

export type AppNavigationShortcut =
  | { type: "settings" }
  | { type: "relative-tab"; direction: -1 | 1 }
  /** Zero-based position; the ninth position means the last tab. */
  | { type: "tab"; index: number }
  | { type: "new-tab" }
  | { type: "reopen-tab" }
  | { type: "close-tab" };

/** Resolve app and workspace tab navigation independently of focus inside a document or input. */
export function appNavigationShortcut(
  event: KeyboardModifierState & Pick<KeyboardEvent, "key" | "code" | "isComposing">,
): AppNavigationShortcut | null {
  if (event.isComposing) return null;
  const key = event.key.toLowerCase();
  if (hasOnlyShortcutModifier(event) && key === ",") return { type: "settings" };
  if (event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey && /^[1-9]$/.test(key)) {
    return { type: "tab", index: Number(key) - 1 };
  }
  if (hasShortcutModifier(event) && !event.altKey && (event.key === "Tab" || event.code === "Tab")) {
    return { type: "relative-tab", direction: event.shiftKey ? -1 : 1 };
  }
  if (hasOnlyShortcutModifier(event) && (event.key === "PageUp" || event.key === "PageDown")) {
    return { type: "relative-tab", direction: event.key === "PageUp" ? -1 : 1 };
  }
  if (hasOnlyShortcutModifier(event, { shift: true }) && key === "t") return { type: "reopen-tab" };
  if (!hasOnlyShortcutModifier(event)) return null;
  if (key === "t") return { type: "new-tab" };
  if (key === "w") return { type: "close-tab" };
  return null;
}
