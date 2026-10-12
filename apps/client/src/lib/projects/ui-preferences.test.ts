import { describe, expect, it } from "vitest";
import {
  DEFAULT_PROJECT_VIEW_ID,
  isProjectViewId,
  parseStoredProjectId,
  parseStoredProjectViewId,
} from "./ui-preferences";

describe("project ui preferences", () => {
  it("accepts supported project view ids only", () => {
    expect(isProjectViewId("dashboard")).toBe(true);
    expect(isProjectViewId("list")).toBe(true);
    expect(isProjectViewId("kanban")).toBe(true);
    expect(isProjectViewId("calendar")).toBe(true);
    expect(isProjectViewId("gantt")).toBe(true);
    expect(isProjectViewId("board")).toBe(false);
    expect(isProjectViewId("summary")).toBe(false);
    expect(isProjectViewId("timeline")).toBe(false);
    expect(isProjectViewId(undefined)).toBe(false);
  });

  it("falls back to the list view for invalid stored view ids", () => {
    expect(parseStoredProjectViewId("board")).toBe(DEFAULT_PROJECT_VIEW_ID);
    expect(parseStoredProjectViewId("summary")).toBe(DEFAULT_PROJECT_VIEW_ID);
    expect(parseStoredProjectViewId("timeline")).toBe(DEFAULT_PROJECT_VIEW_ID);
    expect(parseStoredProjectViewId(null)).toBe(DEFAULT_PROJECT_VIEW_ID);
  });

  it("trims saved project ids and rejects blank values", () => {
    expect(parseStoredProjectId(" project-a ")).toBe("project-a");
    expect(parseStoredProjectId("   ")).toBeNull();
    expect(parseStoredProjectId(null)).toBeNull();
  });
});
