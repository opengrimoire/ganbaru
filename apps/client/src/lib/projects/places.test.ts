import { describe, expect, it } from "vitest";
import { projectHostablePlaces, projectPlaceShortcut } from "./places";

const LINUX = "Linux x86_64";
const MAC = "MacIntel";

function keydown(init: Partial<Parameters<typeof projectPlaceShortcut>[0]>): Parameters<typeof projectPlaceShortcut>[0] {
  return { key: "", code: "", isComposing: false, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...init };
}

describe("project places", () => {
  it("hosts a feature only in a window that shows its tab", () => {
    expect(projectHostablePlaces(["calendar", "projects", "notes", "chat"])).toEqual(["notes", "chat"]);
    expect(projectHostablePlaces(["calendar", "projects", "chat"])).toEqual(["chat"]);
    expect(projectHostablePlaces(["chat", "notes"])).toEqual(["notes", "chat"]);
    expect(projectHostablePlaces(["projects"])).toEqual([]);
  });

  it("never treats the project Calendar view as the hosted Calendar tab", () => {
    expect(projectHostablePlaces(["calendar"])).toEqual([]);
  });

  it("opens sidebar entries by position with the shortcut modifier and a digit", () => {
    const places = ["overview", "list", "board", "notes", "chat", "week", "chat-2"];
    expect(projectPlaceShortcut(keydown({ key: "1", code: "Digit1", ctrlKey: true }), places, LINUX)).toBe("overview");
    expect(projectPlaceShortcut(keydown({ key: "4", code: "Digit4", ctrlKey: true }), places, LINUX)).toBe("notes");
    expect(projectPlaceShortcut(keydown({ key: "7", code: "Numpad7", ctrlKey: true }), places, LINUX)).toBe("chat-2");
    expect(projectPlaceShortcut(keydown({ key: "6", code: "Digit6", metaKey: true }), places, MAC)).toBe("week");
  });

  it("returns nothing past the end of the list", () => {
    expect(projectPlaceShortcut(keydown({ key: "3", code: "Digit3", ctrlKey: true }), ["a", "b"], LINUX)).toBeNull();
    expect(projectPlaceShortcut(keydown({ key: "1", code: "Digit1", ctrlKey: true }), [], LINUX)).toBeNull();
  });

  it("reads the physical digit on layouts whose top row types symbols", () => {
    const places = ["overview", "list"];
    expect(projectPlaceShortcut(keydown({ key: "&", code: "Digit1", ctrlKey: true }), places, LINUX)).toBe("overview");
    expect(projectPlaceShortcut(keydown({ key: "é", code: "Digit2", ctrlKey: true }), places, LINUX)).toBe("list");
  });

  it("ignores plain digits, other modifiers, composition, and keys outside one to nine", () => {
    const places = ["overview", "list", "notes"];
    expect(projectPlaceShortcut(keydown({ key: "2", code: "Digit2" }), places, LINUX)).toBeNull();
    expect(projectPlaceShortcut(keydown({ key: "2", code: "Digit2", altKey: true }), places, LINUX)).toBeNull();
    expect(projectPlaceShortcut(keydown({ key: "2", code: "Digit2", ctrlKey: true, shiftKey: true }), places, LINUX)).toBeNull();
    expect(projectPlaceShortcut(keydown({ key: "2", code: "Digit2", ctrlKey: true, altKey: true }), places, LINUX)).toBeNull();
    expect(projectPlaceShortcut(keydown({ key: "2", code: "Digit2", metaKey: true }), places, LINUX)).toBeNull();
    expect(projectPlaceShortcut(keydown({ key: "2", code: "Digit2", ctrlKey: true, isComposing: true }), places, LINUX)).toBeNull();
    expect(projectPlaceShortcut(keydown({ key: "0", code: "Digit0", ctrlKey: true }), places, LINUX)).toBeNull();
    expect(projectPlaceShortcut(keydown({ key: "a", code: "KeyA", ctrlKey: true }), places, LINUX)).toBeNull();
  });
});
