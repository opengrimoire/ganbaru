import { describe, expect, it } from "vitest";
import { isProjectNameTaken, projectNameKey } from "./text";

const projects = [
  { id: "thesis", groupId: "school", name: "Thesis" },
  { id: "draft", groupId: "school", name: "  Draft notes " },
  { id: "other-thesis", groupId: "work", name: "thesis" },
];

describe("projectNameKey", () => {
  it("ignores surrounding whitespace and case", () => {
    expect(projectNameKey("  Ünïcode Thesis ")).toBe("ünïcode thesis");
  });
});

describe("isProjectNameTaken", () => {
  it("matches a name in the same group regardless of case and surrounding whitespace", () => {
    expect(isProjectNameTaken(projects, "school", " THESIS ")).toBe(true);
    expect(isProjectNameTaken(projects, "school", "draft notes")).toBe(true);
  });

  it("allows the same name in another group", () => {
    expect(isProjectNameTaken(projects, "personal", "Thesis")).toBe(false);
  });

  it("lets a project keep or recase its own name", () => {
    expect(isProjectNameTaken(projects, "school", "thesis", "thesis")).toBe(false);
    expect(isProjectNameTaken(projects, "work", "Thesis", "other-thesis")).toBe(false);
  });

  it("refuses a move into a group where the name is used", () => {
    expect(isProjectNameTaken(projects, "work", "Thesis", "thesis")).toBe(true);
  });

  it("treats a blank name as not taken so the required check reports it", () => {
    expect(isProjectNameTaken(projects, "school", "   ")).toBe(false);
  });

  it("keeps inner whitespace significant", () => {
    expect(isProjectNameTaken(projects, "school", "Draft  notes")).toBe(false);
  });
});
