// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { createBackdropPress } from "./backdrop-press";

function layer() {
  const backdrop = document.createElement("div");
  const dialog = document.createElement("div");
  const slider = document.createElement("input");
  dialog.append(slider);
  backdrop.append(dialog);
  return { backdrop, slider };
}

function eventOn(target: EventTarget, type: string): Event {
  const event = new Event(type, { bubbles: true });
  Object.defineProperty(event, "target", { value: target });
  return event;
}

describe("backdrop press", () => {
  it("counts a press that starts and ends on the backdrop", () => {
    const { backdrop } = layer();
    const press = createBackdropPress();
    press.start(eventOn(backdrop, "pointerdown"));
    expect(press.isClick(eventOn(backdrop, "click"))).toBe(true);
  });

  it("ignores a drag that starts inside the dialog and is released over the backdrop", () => {
    const { backdrop, slider } = layer();
    const press = createBackdropPress();
    press.start(eventOn(slider, "pointerdown"));
    expect(press.isClick(eventOn(backdrop, "click"))).toBe(false);
  });

  it("ignores a click with no recorded press and forgets each press after its click", () => {
    const { backdrop } = layer();
    const press = createBackdropPress();
    expect(press.isClick(eventOn(backdrop, "click"))).toBe(false);
    press.start(eventOn(backdrop, "pointerdown"));
    press.isClick(eventOn(backdrop, "click"));
    expect(press.isClick(eventOn(backdrop, "click"))).toBe(false);
  });
});
