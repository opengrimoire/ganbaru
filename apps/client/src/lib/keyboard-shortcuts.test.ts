import { describe, expect, it } from "vitest";
import {
  appNavigationShortcut,
  formatShortcut,
  hasOnlyShortcutModifier,
  hasShortcutModifier,
  isApplePlatform,
  shortcutParts,
} from "./keyboard-shortcuts";

describe("keyboard shortcuts", () => {
  it("detects Apple platforms from navigator-style platform strings", () => {
    expect(isApplePlatform("MacIntel")).toBe(true);
    expect(isApplePlatform("iPad")).toBe(true);
    expect(isApplePlatform("Linux x86_64")).toBe(false);
    expect(isApplePlatform("Win32")).toBe(false);
  });

  it("formats the primary modifier for the current platform", () => {
    expect(formatShortcut("Mod + Shift + T", "Linux x86_64")).toBe("Ctrl + Shift + T");
    expect(formatShortcut("Mod + Shift + T", "MacIntel")).toBe("Cmd + Shift + T");
    expect(shortcutParts("Mod + Enter", "MacIntel")).toEqual(["Cmd", "Enter"]);
  });

  it("matches the platform primary modifier", () => {
    const ctrlEvent = {
      altKey: false,
      ctrlKey: true,
      metaKey: false,
      shiftKey: false,
    };
    const metaEvent = {
      altKey: false,
      ctrlKey: false,
      metaKey: true,
      shiftKey: false,
    };

    expect(hasShortcutModifier(ctrlEvent, "Linux x86_64")).toBe(true);
    expect(hasShortcutModifier(metaEvent, "Linux x86_64")).toBe(false);
    expect(hasShortcutModifier(ctrlEvent, "MacIntel")).toBe(false);
    expect(hasShortcutModifier(metaEvent, "MacIntel")).toBe(true);
  });

  it("matches exact modifier combinations", () => {
    expect(
      hasOnlyShortcutModifier(
        { altKey: false, ctrlKey: true, metaKey: false, shiftKey: true },
        { shift: true },
        "Linux x86_64",
      ),
    ).toBe(true);
    expect(
      hasOnlyShortcutModifier(
        { altKey: false, ctrlKey: true, metaKey: false, shiftKey: false },
        { shift: true },
        "Linux x86_64",
      ),
    ).toBe(false);
  });
});

describe("app navigation shortcuts", () => {
  const key = (overrides: Partial<Parameters<typeof appNavigationShortcut>[0]>) => ({
    key: "Tab", code: "Tab", altKey: false, ctrlKey: false, metaKey: false,
    shiftKey: false, isComposing: false, ...overrides,
  });

  it("accepts numbered navigation and both directions of tab navigation", () => {
    expect(appNavigationShortcut(key({ key: "3", code: "Digit3", altKey: true }), 4)).toEqual({ type: "view", index: 2 });
    expect(appNavigationShortcut(key({ ctrlKey: true }), 4)).toEqual({ type: "relative-view", direction: 1 });
    expect(appNavigationShortcut(key({ ctrlKey: true, shiftKey: true }), 4)).toEqual({ type: "relative-view", direction: -1 });
    expect(appNavigationShortcut(key({ key: "ISO_Left_Tab", ctrlKey: true, shiftKey: true }), 4)).toEqual({ type: "relative-view", direction: -1 });
    expect(appNavigationShortcut(key({ key: ",", ctrlKey: true }), 4)).toEqual({ type: "settings" });
    expect(appNavigationShortcut(key({ key: "PageUp", code: "PageUp", ctrlKey: true }), 4)).toEqual({ type: "relative-view", direction: -1 });
  });

  it("leaves typing, indentation, composition, and AltGr to the focused editor", () => {
    for (const event of [
      key({}), key({ shiftKey: true }), key({ ctrlKey: true, isComposing: true }),
      key({ key: "3", code: "Digit3", altKey: true, ctrlKey: true }),
      key({ key: "5", code: "Digit5", altKey: true }),
      key({ key: "b", code: "KeyB", ctrlKey: true }),
    ]) expect(appNavigationShortcut(event, 4)).toBeNull();
  });
});
