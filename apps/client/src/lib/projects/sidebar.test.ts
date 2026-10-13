import { describe, expect, it } from "vitest";
import { projectSidebarActions, resolveProjectSidebarGroupId, searchProjectSidebar } from "./sidebar";

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

describe("projectSidebarActions", () => {
  const groups = ["group-routine", "work", "home"];

  it("offers rename and every other visible group for a custom project", () => {
    const actions = projectSidebarActions({ id: "project-1", groupId: "work", status: "active" }, groups);
    expect(actions.identityLocked).toBe(false);
    expect(actions.canRename).toBe(true);
    expect(actions.moveGroupIds).toEqual(["group-routine", "home"]);
    expect(actions.canMoveToGroup).toBe(true);
  });

  it("keeps the name and group of built-in Routine projects while lifecycle actions stay available", () => {
    const actions = projectSidebarActions({ id: "project-routine-sleep", groupId: "group-routine", status: "active" }, groups);
    expect(actions.identityLocked).toBe(true);
    expect(actions.canRename).toBe(false);
    expect(actions.canMoveToGroup).toBe(false);
    expect(actions.visibility).toEqual({ label: "hide", status: "hidden" });
    expect(actions.archive).toEqual({ label: "archive", status: "archived" });
  });

  it("cannot move a project when its group is the only visible one", () => {
    const actions = projectSidebarActions({ id: "project-1", groupId: "work", status: "active" }, ["work"]);
    expect(actions.moveGroupIds).toEqual([]);
    expect(actions.canMoveToGroup).toBe(false);
  });

  it("turns hide into show for a hidden project and archive into restore for an archived one", () => {
    expect(projectSidebarActions({ id: "p", groupId: "work", status: "hidden" }, groups)).toMatchObject({
      visibility: { label: "show", status: "active" },
      archive: { label: "archive", status: "archived" },
    });
    expect(projectSidebarActions({ id: "p", groupId: "work", status: "archived" }, groups)).toMatchObject({
      visibility: { label: "hide", status: "hidden" },
      archive: { label: "restore", status: "active" },
    });
  });
});
