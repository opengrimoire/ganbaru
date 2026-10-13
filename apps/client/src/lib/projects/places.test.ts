import { describe, expect, it } from "vitest";
import {
  isProjectHostedPlace,
  projectHostablePlaces,
  projectPlaceShortcut,
  projectSidebarPlaces,
  resolveProjectPlace,
} from "./places";

const LINUX = "Linux x86_64";
const MAC = "MacIntel";

function keydown(init: Partial<Parameters<typeof projectPlaceShortcut>[0]>): Parameters<typeof projectPlaceShortcut>[0] {
  return { key: "", code: "", isComposing: false, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...init };
}

describe("project places", () => {
  it("opens a hosted workspace over the project view and falls back to the view without one", () => {
    expect(resolveProjectPlace("notes", "dashboard")).toBe("notes");
    expect(resolveProjectPlace("chat", "kanban")).toBe("chat");
    expect(resolveProjectPlace(null, "dashboard")).toBe("dashboard");
    expect(resolveProjectPlace(null, "gantt")).toBe("gantt");
  });

  it("hosts a feature only in a window that shows its tab", () => {
    expect(projectHostablePlaces(["calendar", "projects", "notes", "chat"])).toEqual(["notes", "chat"]);
    expect(projectHostablePlaces(["calendar", "projects", "chat"])).toEqual(["chat"]);
    expect(projectHostablePlaces(["chat", "notes"])).toEqual(["notes", "chat"]);
    expect(projectHostablePlaces(["projects"])).toEqual([]);
  });

  it("never treats the project Calendar view as the hosted Calendar tab", () => {
    expect(projectHostablePlaces(["calendar"])).toEqual([]);
    expect(isProjectHostedPlace("calendar")).toBe(false);
  });

  it("lists every project view first and keeps only the hosted places the window can open", () => {
    expect(projectSidebarPlaces(["notes", "chat"])).toEqual(["dashboard", "list", "kanban", "calendar", "gantt", "notes", "chat"]);
    expect(projectSidebarPlaces(["chat"])).toEqual(["dashboard", "list", "kanban", "calendar", "gantt", "chat"]);
    expect(projectSidebarPlaces([])).toEqual(["dashboard", "list", "kanban", "calendar", "gantt"]);
  });

  it("recognizes only hosted places", () => {
    expect(isProjectHostedPlace("notes")).toBe(true);
    expect(isProjectHostedPlace("chat")).toBe(true);
    expect(isProjectHostedPlace("list")).toBe(false);
    expect(isProjectHostedPlace("Notes")).toBe(false);
    expect(isProjectHostedPlace(undefined)).toBe(false);
  });

  it("opens sidebar places by position with the shortcut modifier and a digit", () => {
    const places = projectSidebarPlaces(["notes", "chat"]);
    expect(projectPlaceShortcut(keydown({ key: "1", code: "Digit1", ctrlKey: true }), places, LINUX)).toBe("dashboard");
    expect(projectPlaceShortcut(keydown({ key: "4", code: "Digit4", ctrlKey: true }), places, LINUX)).toBe("calendar");
    expect(projectPlaceShortcut(keydown({ key: "7", code: "Numpad7", ctrlKey: true }), places, LINUX)).toBe("chat");
    expect(projectPlaceShortcut(keydown({ key: "6", code: "Digit6", metaKey: true }), places, MAC)).toBe("notes");
  });

  it("follows the window sidebar when a hosted place is missing", () => {
    const places = projectSidebarPlaces(["chat"]);
    expect(projectPlaceShortcut(keydown({ key: "6", code: "Digit6", ctrlKey: true }), places, LINUX)).toBe("chat");
    expect(projectPlaceShortcut(keydown({ key: "7", code: "Digit7", ctrlKey: true }), places, LINUX)).toBeNull();
  });

  it("reads the physical digit on layouts whose top row types symbols", () => {
    const places = projectSidebarPlaces([]);
    expect(projectPlaceShortcut(keydown({ key: "&", code: "Digit1", ctrlKey: true }), places, LINUX)).toBe("dashboard");
    expect(projectPlaceShortcut(keydown({ key: "\u00e9", code: "Digit2", ctrlKey: true }), places, LINUX)).toBe("list");
  });

  it("ignores plain digits, other modifiers, composition, and keys outside one to nine", () => {
    const places = projectSidebarPlaces(["notes", "chat"]);
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
