// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import { isTextEntryElement, revealScrollDelta } from "./keyboard";

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
