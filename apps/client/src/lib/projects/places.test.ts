import { describe, expect, it } from "vitest";
import {
  isProjectTaskLayoutId,
  projectPlaceForView,
  projectViewForPlace,
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

  it("recognizes only task layouts", () => {
    expect(isProjectTaskLayoutId("kanban")).toBe(true);
    expect(isProjectTaskLayoutId("dashboard")).toBe(false);
    expect(isProjectTaskLayoutId(" list")).toBe(false);
    expect(isProjectTaskLayoutId(null)).toBe(false);
  });
});
