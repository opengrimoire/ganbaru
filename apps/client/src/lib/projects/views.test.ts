import { describe, expect, it } from "vitest";
import type { ProjectView, ProjectViewType } from "./types";
import {
  canAddProjectViewType,
  isProjectHostedViewType,
  isProjectTaskViewType,
  isProjectViewType,
  projectViewForType,
  projectViewsFor,
  resolveActiveProjectView,
} from "./views";

function view(id: string, viewType: ProjectViewType, sortOrder: number, projectId = "project-a"): ProjectView {
  return { id, projectId, viewType, sortOrder, createdAt: "", updatedAt: "" };
}

const VIEWS = [
  view("chat", "chat", 4000),
  view("list", "list", 2000),
  view("other-list", "list", 1000, "project-b"),
  view("overview", "dashboard", 1000),
  view("notes", "notes", 3000),
  view("week", "list", 3000),
];

describe("project views", () => {
  it("recognizes task, hosted, and unknown view types", () => {
    expect(isProjectViewType("gantt")).toBe(true);
    expect(isProjectViewType("web")).toBe(false);
    expect(isProjectViewType(undefined)).toBe(false);
    expect(isProjectTaskViewType("dashboard")).toBe(true);
    expect(isProjectTaskViewType("notes")).toBe(false);
    expect(isProjectHostedViewType("chat")).toBe(true);
    expect(isProjectHostedViewType("calendar")).toBe(false);
  });

  it("lists one project's views by order, breaking ties by id", () => {
    expect(projectViewsFor(VIEWS, "project-a", ["notes", "chat"]).map((entry) => entry.id))
      .toEqual(["overview", "list", "notes", "week", "chat"]);
    expect(projectViewsFor(VIEWS, null, ["notes", "chat"])).toEqual([]);
  });

  it("keeps hosted views only where the window can host their feature", () => {
    expect(projectViewsFor(VIEWS, "project-a", ["chat"]).map((entry) => entry.id))
      .toEqual(["overview", "list", "week", "chat"]);
    expect(projectViewsFor(VIEWS, "project-a", []).map((entry) => entry.id))
      .toEqual(["overview", "list", "week"]);
  });

  it("opens the remembered view, then the first view, then nothing", () => {
    const projectViews = projectViewsFor(VIEWS, "project-a", ["notes", "chat"]);
    expect(resolveActiveProjectView(projectViews, "week")?.id).toBe("week");
    expect(resolveActiveProjectView(projectViews, "removed")?.id).toBe("overview");
    expect(resolveActiveProjectView(projectViews, null)?.id).toBe("overview");
    expect(resolveActiveProjectView(projectViews, "other-list")?.id).toBe("overview");
    expect(resolveActiveProjectView([], "week")).toBeUndefined();
  });

  it("keeps the active view when it already has the requested type", () => {
    const projectViews = projectViewsFor(VIEWS, "project-a", []);
    const week = projectViews.find((entry) => entry.id === "week");
    expect(projectViewForType(projectViews, week, "list")?.id).toBe("week");
    expect(projectViewForType(projectViews, week, "dashboard")?.id).toBe("overview");
    expect(projectViewForType(projectViews, undefined, "list")?.id).toBe("list");
    expect(projectViewForType(projectViews, week, "calendar")).toBeUndefined();
  });

  it("allows repeated task views and one view per hosted type", () => {
    const projectViews = projectViewsFor(VIEWS, "project-a", ["notes", "chat"]);
    expect(canAddProjectViewType(projectViews, "list")).toBe(true);
    expect(canAddProjectViewType(projectViews, "gantt")).toBe(true);
    expect(canAddProjectViewType(projectViews, "notes")).toBe(false);
    expect(canAddProjectViewType(projectViews, "chat")).toBe(false);
    expect(canAddProjectViewType([], "notes")).toBe(true);
  });
});
