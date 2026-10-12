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
  | { type: "relative-view"; direction: -1 | 1 }
  | { type: "view"; index: number };

/** Resolve app navigation independently of focus inside a document or input. */
export function appNavigationShortcut(
  event: KeyboardModifierState & Pick<KeyboardEvent, "key" | "code" | "isComposing">,
  viewCount: number,
): AppNavigationShortcut | null {
  if (event.isComposing) return null;
  if (hasOnlyShortcutModifier(event) && event.key === ",") return { type: "settings" };
  if (event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey && /^[1-9]$/.test(event.key)) {
    const index = Number(event.key) - 1;
    return index < viewCount ? { type: "view", index } : null;
  }
  if (hasShortcutModifier(event) && !event.altKey && (event.key === "Tab" || event.code === "Tab")) {
    return { type: "relative-view", direction: event.shiftKey ? -1 : 1 };
  }
  if (hasOnlyShortcutModifier(event) && (event.key === "PageUp" || event.key === "PageDown")) {
    return { type: "relative-view", direction: event.key === "PageUp" ? -1 : 1 };
  }
  return null;
}
