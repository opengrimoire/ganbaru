import { describe, expect, it } from "vitest";
import { buildProjectLists, projectListGroupId, projectListKey, UNGROUPED_PROJECT_LIST_KEY } from "./lists";
import type { ProjectGroup } from "./types";

function group(id: string): ProjectGroup {
  return { id, name: id, icon: "lucide:folder", orderKey: "a0", collapsed: false, createdAt: "c", updatedAt: "u" };
}

describe("project list keys", () => {
  it("round-trips group ids and the ungrouped list without collisions", () => {
    expect(projectListKey("work")).toBe("group:work");
    expect(projectListKey(null)).toBe(UNGROUPED_PROJECT_LIST_KEY);
    expect(projectListGroupId(projectListKey("work"))).toBe("work");
    expect(projectListGroupId(projectListKey("ungrouped"))).toBe("ungrouped");
    expect(projectListKey("ungrouped")).not.toBe(UNGROUPED_PROJECT_LIST_KEY);
    expect(projectListGroupId(UNGROUPED_PROJECT_LIST_KEY)).toBeNull();
  });
});

describe("buildProjectLists", () => {
  const projectsByGroup = new Map<string | null, string[]>([
    ["work", ["report"]],
    [null, ["loose-a", "loose-b"]],
  ]);
  const projectsFor = (groupId: string | null) => projectsByGroup.get(groupId) ?? [];

  it("lists groups in order, keeps empty groups, and ends with the ungrouped projects", () => {
    expect(buildProjectLists([group("work"), group("home")], projectsFor)).toEqual([
      { key: "group:work", group: group("work"), projects: ["report"] },
      { key: "group:home", group: group("home"), projects: [] },
      { key: UNGROUPED_PROJECT_LIST_KEY, group: null, projects: ["loose-a", "loose-b"] },
    ]);
  });

  it("omits the ungrouped list when no project is outside every group", () => {
    const lists = buildProjectLists([group("work")], (groupId) => groupId === null ? [] : ["report"]);
    expect(lists.map((list) => list.key)).toEqual(["group:work"]);
  });

  it("lists only ungrouped projects when there are no groups", () => {
    expect(buildProjectLists([], projectsFor)).toEqual([
      { key: UNGROUPED_PROJECT_LIST_KEY, group: null, projects: ["loose-a", "loose-b"] },
    ]);
  });
});
