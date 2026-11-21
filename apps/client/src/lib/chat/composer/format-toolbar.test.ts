import { describe, expect, it } from "vitest";
import {
  chatFormatAriaKeyShortcut,
  chatFormatToolbarPlacement,
  chatFormatToolbarRovingIndex,
  chatFormatToolbarVisible,
  type ChatFormatToolbarVisibilityInput,
} from "./format-toolbar";

const shown: ChatFormatToolbarVisibilityInput = {
  focused: true,
  disabled: false,
  selection: { start: 2, end: 6 },
  composing: false,
  menuOpen: false,
  dismissed: null,
  mobileShell: false,
  lastInput: "pointer",
};

const viewport = { left: 0, top: 0, right: 800, bottom: 600 };
const editor = { left: 100, top: 400, right: 700, bottom: 500 };

describe("Chat formatting toolbar visibility", () => {
  it("shows for a pointer or keyboard selection in a focused composer", () => {
    expect(chatFormatToolbarVisible(shown)).toBe(true);
    expect(chatFormatToolbarVisible({ ...shown, lastInput: "keyboard" })).toBe(true);
  });

  it("hides for a collapsed, missing, or unfocused selection", () => {
    expect(chatFormatToolbarVisible({ ...shown, selection: { start: 4, end: 4 } })).toBe(false);
    expect(chatFormatToolbarVisible({ ...shown, selection: null })).toBe(false);
    expect(chatFormatToolbarVisible({ ...shown, focused: false })).toBe(false);
    expect(chatFormatToolbarVisible({ ...shown, disabled: true })).toBe(false);
  });

  it("stays out of the way of composition and autocomplete menus", () => {
    expect(chatFormatToolbarVisible({ ...shown, composing: true })).toBe(false);
    expect(chatFormatToolbarVisible({ ...shown, menuOpen: true })).toBe(false);
  });

  it("leaves touch selections and the Android shell to the platform selection menu", () => {
    expect(chatFormatToolbarVisible({ ...shown, lastInput: "touch" })).toBe(false);
    expect(chatFormatToolbarVisible({ ...shown, mobileShell: true, lastInput: "keyboard" })).toBe(false);
  });

  it("stays dismissed for the same selection and returns once the selection changes", () => {
    expect(chatFormatToolbarVisible({ ...shown, dismissed: { start: 2, end: 6 } })).toBe(false);
    expect(chatFormatToolbarVisible({ ...shown, dismissed: { start: 2, end: 5 } })).toBe(true);
  });
});

describe("Chat formatting toolbar placement", () => {
  it("centers above the selection when it fits", () => {
    expect(chatFormatToolbarPlacement({
      selection: { left: 300, top: 420, right: 400, bottom: 440 },
      clip: editor,
      viewport,
      width: 200,
      height: 32,
    })).toEqual({ left: 250, top: 382, side: "above" });
  });

  it("goes below the selection when there is no room above", () => {
    expect(chatFormatToolbarPlacement({
      selection: { left: 300, top: 20, right: 400, bottom: 40 },
      clip: { ...editor, top: 0 },
      viewport,
      width: 200,
      height: 32,
    })).toEqual({ left: 250, top: 46, side: "below" });
  });

  it("clamps to the viewport edges horizontally", () => {
    const placement = (left: number, right: number) => chatFormatToolbarPlacement({
      selection: { left, top: 420, right, bottom: 440 },
      clip: { ...editor, left: 0, right: 800 },
      viewport,
      width: 200,
      height: 32,
    });

    expect(placement(0, 20)?.left).toBe(8);
    expect(placement(780, 800)?.left).toBe(592);
  });

  it("anchors to the visible part of a selection the editor scrolls", () => {
    expect(chatFormatToolbarPlacement({
      selection: { left: 300, top: 360, right: 400, bottom: 440 },
      clip: editor,
      viewport,
      width: 200,
      height: 32,
    })?.top).toBe(362);
  });

  it("hides when the selection scrolled out of view", () => {
    expect(chatFormatToolbarPlacement({
      selection: { left: 300, top: 360, right: 400, bottom: 380 },
      clip: editor,
      viewport,
      width: 200,
      height: 32,
    })).toBeNull();
  });

  it("hides rather than covering the selection when neither side fits", () => {
    expect(chatFormatToolbarPlacement({
      selection: { left: 20, top: 30, right: 120, bottom: 150 },
      clip: { left: 0, top: 0, right: 280, bottom: 180 },
      viewport: { left: 0, top: 0, right: 280, bottom: 180 },
      width: 260,
      height: 32,
    })).toBeNull();
  });

  it("fits the minimum window when a single line is selected", () => {
    expect(chatFormatToolbarPlacement({
      selection: { left: 20, top: 120, right: 120, bottom: 140 },
      clip: { left: 0, top: 100, right: 280, bottom: 170 },
      viewport: { left: 0, top: 0, right: 280, bottom: 180 },
      width: 260,
      height: 32,
    })).toEqual({ left: 8, top: 82, side: "above" });
  });
});

describe("Chat formatting toolbar keyboard", () => {
  it("moves between buttons with arrows, Home, and End, wrapping at the ends", () => {
    expect(chatFormatToolbarRovingIndex("ArrowRight", 0, 8)).toBe(1);
    expect(chatFormatToolbarRovingIndex("ArrowRight", 7, 8)).toBe(0);
    expect(chatFormatToolbarRovingIndex("ArrowLeft", 0, 8)).toBe(7);
    expect(chatFormatToolbarRovingIndex("Home", 5, 8)).toBe(0);
    expect(chatFormatToolbarRovingIndex("End", 2, 8)).toBe(7);
    expect(chatFormatToolbarRovingIndex("Enter", 2, 8)).toBeNull();
    expect(chatFormatToolbarRovingIndex("ArrowRight", 0, 0)).toBeNull();
  });

  it("names shortcuts with the platform modifier for assistive technology", () => {
    expect(chatFormatAriaKeyShortcut("Mod + Shift + 7", "Linux x86_64")).toBe("Control+Shift+7");
    expect(chatFormatAriaKeyShortcut("Mod + Alt + Shift + C", "MacIntel")).toBe("Meta+Alt+Shift+C");
  });
});
