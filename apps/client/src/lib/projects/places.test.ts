import { describe, expect, it } from "vitest";
import { isProjectHostedPlace, projectHostablePlaces, projectSidebarPlaces, resolveProjectPlace } from "./places";

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
});
