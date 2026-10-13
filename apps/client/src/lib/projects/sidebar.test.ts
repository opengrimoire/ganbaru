import { describe, expect, it } from "vitest";
import { resolveProjectSidebarGroupId, searchProjectSidebar } from "./sidebar";

describe("resolveProjectSidebarGroupId", () => {
  const groups = ["health", "work", "home"];

  it("lists the selected project's group", () => {
    expect(resolveProjectSidebarGroupId("work", groups)).toBe("work");
  });

  it("uses the first visible group when the selected group is not visible", () => {
    expect(resolveProjectSidebarGroupId("hidden", groups)).toBe("health");
    expect(resolveProjectSidebarGroupId(null, groups)).toBe("health");
  });

  it("returns null when there are no visible groups", () => {
    expect(resolveProjectSidebarGroupId("work", [])).toBeNull();
  });
});

describe("searchProjectSidebar", () => {
  const groups = [
    { name: "Work", projects: [{ name: "Quarterly report" }, { name: "Hiring" }] },
    { name: "Personal", projects: [{ name: "Mudanza a León" }, { name: "Reading list" }] },
  ];
  const names = (query: string) => searchProjectSidebar(query, groups).map((result) => result.project.name);

  it("matches project names across every group, ignoring case", () => {
    expect(names("REPORT")).toEqual(["Quarterly report"]);
    expect(names("in")).toEqual(["Hiring", "Reading list"]);
  });

  it("ignores diacritics in both the query and the names", () => {
    expect(names("leon")).toEqual(["Mudanza a León"]);
    expect(names("LEÓN")).toEqual(["Mudanza a León"]);
  });

  it("lists every project of a group whose name matches", () => {
    expect(names("pers")).toEqual(["Mudanza a León", "Reading list"]);
  });

  it("reports the group of each result", () => {
    expect(searchProjectSidebar("hiring", groups)).toEqual([{ project: { name: "Hiring" }, groupName: "Work" }]);
  });

  it("matches nothing for a blank query or a missing name", () => {
    expect(names("   ")).toEqual([]);
    expect(names("garden")).toEqual([]);
  });

  it("collapses repeated whitespace in the query", () => {
    expect(names("  reading   list ")).toEqual(["Reading list"]);
  });
});
