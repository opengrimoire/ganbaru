import { describe, expect, it } from "vitest";
import { calendarZoomKeyAction, type CalendarZoomKeyInput } from "./zoom-keys";

function key(input: Partial<CalendarZoomKeyInput>): CalendarZoomKeyInput {
  return { key: "", code: "", shiftKey: false, ctrlKey: false, metaKey: false, ...input };
}

describe("calendarZoomKeyAction", () => {
  it("zooms in from the shifted plus key on US, Spanish, and German layouts", () => {
    expect(calendarZoomKeyAction(key({ key: "+", code: "Equal", shiftKey: true }))).toBe("in");
    expect(calendarZoomKeyAction(key({ key: "*", code: "BracketRight", shiftKey: true }))).toBe("in");
    expect(calendarZoomKeyAction(key({ key: "+", code: "NumpadAdd" }))).toBe("in");
  });

  it("zooms in from the unshifted Nordic plus key through its character", () => {
    expect(calendarZoomKeyAction(key({ key: "+", code: "Minus" }))).toBe("in");
  });

  it("keeps US Shift + Minus as zoom out", () => {
    expect(calendarZoomKeyAction(key({ key: "_", code: "Minus", shiftKey: true }))).toBe("out");
    expect(calendarZoomKeyAction(key({ key: "-", code: "Minus" }))).toBe("out");
  });

  it("resets from Shift + 0 by character or physical code", () => {
    expect(calendarZoomKeyAction(key({ key: ")", code: "Digit0", shiftKey: true }))).toBe("reset");
    expect(calendarZoomKeyAction(key({ key: "=", code: "Digit0", shiftKey: true }))).toBe("reset");
    expect(calendarZoomKeyAction(key({ key: "0", code: "Digit0" }))).toBeNull();
  });

  it("leaves Ctrl and Meta combinations to app-level zoom", () => {
    expect(calendarZoomKeyAction(key({ key: "+", code: "Equal", shiftKey: true, ctrlKey: true }))).toBeNull();
    expect(calendarZoomKeyAction(key({ key: "-", code: "Minus", metaKey: true }))).toBeNull();
  });

  it("ignores unrelated keys", () => {
    expect(calendarZoomKeyAction(key({ key: "=", code: "Equal" }))).toBeNull();
    expect(calendarZoomKeyAction(key({ key: "a", code: "KeyA", shiftKey: true }))).toBeNull();
  });
});
