import { describe, expect, it } from "vitest";
import {
  parseStoredProjectActiveViewIds,
  parseStoredProjectId,
  parseStoredProjectIds,
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

  it("restores collapsed sidebar projects once each and ignores malformed values", () => {
    expect(parseStoredProjectIds([" project-a ", "project-b", "project-a", "", 4, null])).toEqual(["project-a", "project-b"]);
    expect(parseStoredProjectIds({ "project-a": true })).toEqual([]);
    expect(parseStoredProjectIds(undefined)).toEqual([]);
  });
});
