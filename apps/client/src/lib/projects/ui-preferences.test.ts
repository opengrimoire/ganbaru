import { describe, expect, it } from "vitest";
import {
  parseStoredProjectActiveViewIds,
  parseStoredProjectId,
  pruneProjectActiveViewIds,
} from "./ui-preferences";

describe("project ui preferences", () => {
  it("trims saved project ids and rejects blank values", () => {
    expect(parseStoredProjectId(" project-a ")).toBe("project-a");
    expect(parseStoredProjectId("   ")).toBeNull();
    expect(parseStoredProjectId(null)).toBeNull();
  });

  it("restores remembered views per project and drops malformed entries", () => {
    expect(parseStoredProjectActiveViewIds({
      "project-a": "view-a",
      " project-b ": " view-b ",
      "project-c": "",
      "project-d": 4,
      "  ": "view-e",
    })).toEqual({ "project-a": "view-a", "project-b": "view-b" });
    expect(parseStoredProjectActiveViewIds(["view-a"])).toEqual({});
    expect(parseStoredProjectActiveViewIds("view-a")).toEqual({});
    expect(parseStoredProjectActiveViewIds(null)).toEqual({});
  });

  it("forgets remembered views of projects that no longer exist", () => {
    expect(pruneProjectActiveViewIds(
      { "project-a": "view-a", "project-gone": "view-b" },
      new Set(["project-a", "project-c"]),
    )).toEqual({ "project-a": "view-a" });
  });
});
