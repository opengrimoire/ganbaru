import { describe, expect, it } from "vitest";
import { ProjectTaskDetailCache } from "./detail-cache";
import type { ProjectTaskDetailData } from "$lib/projects/types";

function detail(id: string, updatedAt: string, description = ""): ProjectTaskDetailData {
  return {
    task: {
      id, projectId: "project", sectionId: "section", statusId: "status", title: id,
      description, priority: "normal", taskType: "task", sectionSortOrder: 0,
      statusSortOrder: 0, milestone: false, createdAt: "2026-01-01", updatedAt,
    },
    relatedTasks: [], checklistItems: [], tags: [], taskTagLinks: [], customFields: [],
    customFieldOptions: [], customFieldValues: [], customFieldOptionValues: [],
    dependencies: [], eventLinks: [], taskChangeEvents: [],
  };
}

describe("ProjectTaskDetailCache", () => {
  it("keys details by task update version", () => {
    const cache = new ProjectTaskDetailCache(10_000);
    cache.set(detail("task", "v1"));
    expect(cache.get("task", "v1")?.task.id).toBe("task");
    expect(cache.get("task", "v2")).toBeUndefined();
  });

  it("evicts least recently used details to stay byte bounded", () => {
    const first = detail("first", "v1", "x".repeat(100));
    const second = detail("second", "v1", "y".repeat(100));
    const oneEntryBytes = new TextEncoder().encode(JSON.stringify(first)).byteLength;
    const cache = new ProjectTaskDetailCache(oneEntryBytes + 16);
    cache.set(first);
    cache.set(second);
    expect(cache.get("first", "v1")).toBeUndefined();
    expect(cache.get("second", "v1")).toBeDefined();
  });
});
