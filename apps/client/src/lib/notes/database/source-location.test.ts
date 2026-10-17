import { describe, expect, it } from "vitest";
import { compactSourceLocation } from "./source-location";

const title = (text: string, last = false) => ({ kind: "title", text, last });

describe("compactSourceLocation", () => {
  it("returns no pieces for an empty path", () => {
    expect(compactSourceLocation([])).toEqual([]);
  });

  it("keeps short paths whole and marks the final title", () => {
    expect(compactSourceLocation(["Group", "Project", "Tasks"])).toEqual([title("Group"), title("Project"), title("Tasks", true)]);
    expect(compactSourceLocation(["Tasks"])).toEqual([title("Tasks", true)]);
  });

  it("keeps a path that exactly fits the limit", () => {
    expect(compactSourceLocation(["A", "B", "C", "D"], 4)).toHaveLength(4);
  });

  it("collapses the middle of long paths but keeps the outermost and innermost segments", () => {
    expect(compactSourceLocation(["Group", "Project", "Folder", "Notes", "Page"], 4)).toEqual([
      title("Group"),
      { kind: "gap" },
      title("Notes"),
      title("Page", true),
    ]);
  });

  it("never shows fewer than one segment on each side of the gap", () => {
    expect(compactSourceLocation(["A", "B", "C", "D"], 1)).toEqual([title("A"), { kind: "gap" }, title("D", true)]);
  });

  it("keeps duplicate titles as separate pieces", () => {
    expect(compactSourceLocation(["Notes", "Notes"])).toEqual([title("Notes"), title("Notes", true)]);
  });
});
