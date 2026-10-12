import { describe, expect, it } from "vitest";
import {
  isProjectHostedPlace,
  isProjectTaskLayoutId,
  projectHostablePlaces,
  projectPlaceForView,
  projectSidebarPlaces,
  projectViewForPlace,
  resolveProjectPlace,
} from "./places";

describe("project places", () => {
  it("shows the dashboard as the overview and every other view as tasks", () => {
    expect(projectPlaceForView("dashboard")).toBe("overview");
    expect(projectPlaceForView("list")).toBe("tasks");
    expect(projectPlaceForView("kanban")).toBe("tasks");
    expect(projectPlaceForView("calendar")).toBe("tasks");
    expect(projectPlaceForView("gantt")).toBe("tasks");
  });

  it("reopens the last task layout for Tasks and the dashboard for the overview", () => {
    expect(projectViewForPlace("tasks", "gantt")).toBe("gantt");
    expect(projectViewForPlace("overview", "gantt")).toBe("dashboard");
  });

  it("opens a hosted workspace over the project view and falls back to the view's place without one", () => {
    expect(resolveProjectPlace("notes", "dashboard")).toBe("notes");
    expect(resolveProjectPlace("notes", "kanban")).toBe("notes");
    expect(resolveProjectPlace(null, "dashboard")).toBe("overview");
    expect(resolveProjectPlace(null, "kanban")).toBe("tasks");
  });

  it("recognizes only task layouts", () => {
    expect(isProjectTaskLayoutId("kanban")).toBe(true);
    expect(isProjectTaskLayoutId("dashboard")).toBe(false);
    expect(isProjectTaskLayoutId(" list")).toBe(false);
    expect(isProjectTaskLayoutId(null)).toBe(false);
  });

  it("hosts a feature only in a window that shows its tab", () => {
    expect(projectHostablePlaces(["calendar", "projects", "notes", "chat"])).toEqual(["notes", "chat"]);
    expect(projectHostablePlaces(["calendar", "projects", "chat"])).toEqual(["chat"]);
    expect(projectHostablePlaces(["chat", "notes"])).toEqual(["notes", "chat"]);
    expect(projectHostablePlaces(["projects"])).toEqual([]);
  });

  it("lists view places first and keeps only the hosted places the window can open", () => {
    expect(projectSidebarPlaces(["notes", "chat"])).toEqual(["overview", "tasks", "notes", "chat"]);
    expect(projectSidebarPlaces(["chat"])).toEqual(["overview", "tasks", "chat"]);
    expect(projectSidebarPlaces([])).toEqual(["overview", "tasks"]);
  });

  it("recognizes only hosted places", () => {
    expect(isProjectHostedPlace("notes")).toBe(true);
    expect(isProjectHostedPlace("chat")).toBe(true);
    expect(isProjectHostedPlace("tasks")).toBe(false);
    expect(isProjectHostedPlace("Notes")).toBe(false);
    expect(isProjectHostedPlace(undefined)).toBe(false);
  });
});
