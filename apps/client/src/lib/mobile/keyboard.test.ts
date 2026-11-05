// @vitest-environment jsdom

import { afterEach, describe, expect, it } from "vitest";
import {
  isTextEntryElement,
  keyboardSurfaceMode,
  keyboardSurfaceOf,
  keyboardSurfaceShift,
  revealScrollDelta,
} from "./keyboard";

describe("revealScrollDelta", () => {
  const visible = { top: 100, bottom: 500 };

  it("leaves a caret that already fits with the margin", () => {
    expect(revealScrollDelta({ top: 200, bottom: 220 }, visible, 16)).toBe(0);
  });

  it("scrolls a caret hidden under the keyboard up above it with the margin", () => {
    expect(revealScrollDelta({ top: 600, bottom: 620 }, visible, 16)).toBe(136);
  });

  it("scrolls a caret above the visible area down into it", () => {
    expect(revealScrollDelta({ top: 40, bottom: 60 }, visible, 16)).toBe(-76);
  });

  it("keeps the top of a target taller than the visible area in view", () => {
    expect(revealScrollDelta({ top: 300, bottom: 1200 }, visible, 16)).toBe(184);
  });

  it("does nothing when the margins leave no visible area", () => {
    expect(revealScrollDelta({ top: 600, bottom: 620 }, { top: 100, bottom: 120 }, 16)).toBe(0);
  });
});

describe("isTextEntryElement", () => {
  function input(type: string, setup: (element: HTMLInputElement) => void = () => {}): HTMLInputElement {
    const element = document.createElement("input");
    element.type = type;
    setup(element);
    return element;
  }

  it("accepts editable text fields and text areas", () => {
    expect(isTextEntryElement(input("text"))).toBe(true);
    expect(isTextEntryElement(input("search"))).toBe(true);
    expect(isTextEntryElement(document.createElement("textarea"))).toBe(true);
  });

  it("rejects controls that do not open a keyboard", () => {
    expect(isTextEntryElement(input("checkbox"))).toBe(false);
    expect(isTextEntryElement(input("range"))).toBe(false);
    expect(isTextEntryElement(input("text", (element) => { element.readOnly = true; }))).toBe(false);
    expect(isTextEntryElement(input("text", (element) => { element.disabled = true; }))).toBe(false);
    expect(isTextEntryElement(document.createElement("button"))).toBe(false);
    expect(isTextEntryElement(null)).toBe(false);
  });
});

describe("keyboardSurfaceOf", () => {
  afterEach(() => {
    document.body.replaceChildren();
  });

  function dialog(parent: HTMLElement, fixed: "dialog" | "overlay" | "none"): { overlay: HTMLElement; panel: HTMLElement; field: HTMLInputElement } {
    const overlay = document.createElement("div");
    if (fixed === "overlay") overlay.style.position = "fixed";
    const panel = document.createElement("div");
    panel.setAttribute("role", "dialog");
    panel.setAttribute("aria-modal", "true");
    if (fixed === "dialog") panel.style.position = "fixed";
    const field = document.createElement("input");
    panel.append(field);
    overlay.append(panel);
    parent.append(overlay);
    return { overlay, panel, field };
  }

  it("leaves a field on the page to the page", () => {
    const field = document.createElement("input");
    document.body.append(field);
    expect(keyboardSurfaceOf(field)).toBeNull();
    expect(keyboardSurfaceOf(null)).toBeNull();
  });

  it("picks the fixed layer that holds the modal dialog", () => {
    const { overlay, field } = dialog(document.body, "overlay");
    expect(keyboardSurfaceOf(field)).toBe(overlay);
  });

  it("picks a modal dialog that is its own fixed layer", () => {
    const { panel, field } = dialog(document.body, "dialog");
    expect(keyboardSurfaceOf(field)).toBe(panel);
  });

  it("picks only the innermost dialog, so a dialog behind it keeps its size", () => {
    const outer = dialog(document.body, "dialog");
    const inner = dialog(outer.panel, "overlay");
    expect(keyboardSurfaceOf(inner.field)).toBe(inner.overlay);
    expect(keyboardSurfaceOf(outer.field)).toBe(outer.panel);
  });

  it("falls back to the dialog when no layer is fixed", () => {
    const { panel, field } = dialog(document.body, "none");
    expect(keyboardSurfaceOf(field)).toBe(panel);
  });
});

describe("keyboardSurfaceMode", () => {
  function dialogEndingAt(bottom: number): HTMLElement {
    const dialog = document.createElement("div");
    dialog.getBoundingClientRect = () => new DOMRect(0, 0, 100, bottom);
    return dialog;
  }

  it("lifts a dialog that reaches the bottom of the viewport", () => {
    expect(keyboardSurfaceMode(dialogEndingAt(800), 800, 0)).toBe("lift");
    expect(keyboardSurfaceMode(dialogEndingAt(799.5), 800, 0)).toBe("lift");
  });

  it("lifts a dialog that ends at the bottom safe area", () => {
    expect(keyboardSurfaceMode(dialogEndingAt(752), 800, 48)).toBe("lift");
  });

  it("shifts a centered dialog or popover that ends above the bottom safe area", () => {
    expect(keyboardSurfaceMode(dialogEndingAt(500), 800, 48)).toBe("shift");
    expect(keyboardSurfaceMode(dialogEndingAt(736), 800, 48)).toBe("shift");
  });
});

describe("keyboardSurfaceShift", () => {
  const dialog = { top: 300, bottom: 500 };

  it("keeps a dialog still while the keyboard does not reach it", () => {
    expect(keyboardSurfaceShift(dialog, 800, 40, 16)).toBe(0);
    expect(keyboardSurfaceShift(dialog, 516, 40, 16)).toBe(0);
  });

  it("moves a dialog up only as far as the keyboard covers it", () => {
    expect(keyboardSurfaceShift(dialog, 450, 40, 16)).toBe(66);
  });

  it("never moves the dialog top above the limit", () => {
    expect(keyboardSurfaceShift(dialog, 100, 40, 16)).toBe(260);
    expect(keyboardSurfaceShift({ top: 20, bottom: 500 }, 300, 40, 16)).toBe(0);
  });

  it("ignores a keyboard top that is not a number", () => {
    expect(keyboardSurfaceShift(dialog, Number.NaN, 40, 16)).toBe(0);
  });
});
