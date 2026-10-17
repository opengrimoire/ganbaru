import { describe, expect, it } from "vitest";
import { calendarNavigationKeyAction } from "./navigation-controller";

function key(value: string, modifiers: Partial<Pick<KeyboardEvent, "ctrlKey" | "altKey" | "metaKey">> = {}) {
  return { key: value, ctrlKey: false, altKey: false, metaKey: false, ...modifiers };
}

describe("calendarNavigationKeyAction", () => {
  it("closes an open panel before considering calendar navigation", () => {
    expect(calendarNavigationKeyAction(key("Escape"), {
      blocked: true,
      sessionClosed: false,
      confirmOpen: false,
      viewMode: "week",
    })).toEqual({ type: "close" });
  });

  it("maps horizontal keys to held navigation only when the route is eligible", () => {
    const state = { blocked: false, sessionClosed: true, confirmOpen: false, viewMode: "week" as const };
    expect(calendarNavigationKeyAction(key("ArrowLeft"), state)).toEqual({
      type: "hold",
      key: "ArrowLeft",
      direction: "back",
    });
    expect(calendarNavigationKeyAction(key("ArrowRight", { ctrlKey: true }), state)).toBeNull();
    expect(calendarNavigationKeyAction(key("ArrowRight"), { ...state, confirmOpen: true })).toBeNull();
  });

  it("scrolls vertical timeline views and navigates month rows", () => {
    expect(calendarNavigationKeyAction(key("ArrowDown"), {
      blocked: false,
      sessionClosed: true,
      confirmOpen: false,
      viewMode: "day",
    })).toEqual({ type: "scroll", direction: 1 });
    expect(calendarNavigationKeyAction(key("ArrowUp"), {
      blocked: false,
      sessionClosed: true,
      confirmOpen: false,
      viewMode: "month",
    })).toEqual({ type: "hold", key: "ArrowUp", direction: "back" });
  });
});
