import { describe, expect, it } from "vitest";
import type {
  ProjectsOptionalData,
  ProjectsSnapshot,
  ProjectTask,
} from "$lib/projects/types";
import {
  mergeProjectOptionalData,
  projectViewOptionalDataKinds,
} from "./optional-data";

const timestamp = "2026-01-01T00:00:00.000Z";

function task(id: string, projectId: string): ProjectTask {
  return {
    id,
    projectId,
    sectionId: `section-${projectId}`,
    statusId: `status-${projectId}`,
    title: id,
    description: "",
    priority: "normal",
    taskType: "task",
    sectionSortOrder: 0,
    statusSortOrder: 0,
    milestone: false,
    createdAt: timestamp,
    updatedAt: timestamp,
  };
}

function snapshot(): ProjectsSnapshot {
  return {
    groups: [],
    projects: [],
    sections: [],
    statuses: [],
    priorities: [],
    tasks: [task("task-a", "project-a"), task("task-b", "project-b")],
    checklistItems: [],
    tags: [
      { id: "tag-a-old", projectId: "project-a", name: "Old", sortOrder: 0, createdAt: timestamp, updatedAt: timestamp },
      { id: "tag-b", projectId: "project-b", name: "Keep", sortOrder: 0, createdAt: timestamp, updatedAt: timestamp },
    ],
    taskTagLinks: [
      { taskId: "task-a", tagId: "tag-a-old", createdAt: timestamp },
      { taskId: "task-b", tagId: "tag-b", createdAt: timestamp },
    ],
    customFields: [],
    customFieldOptions: [],
    customFieldValues: [],
    customFieldOptionValues: [],
    dependencies: [
      { id: "dependency-a", blockingTaskId: "task-a", blockedTaskId: "task-a", dependencyType: "blocks", createdAt: timestamp },
      { id: "dependency-b", blockingTaskId: "task-b", blockedTaskId: "task-b", dependencyType: "blocks", createdAt: timestamp },
    ],
    eventLinks: [
      { taskId: "task-a", eventId: "event-a", linkKind: "scheduled", createdAt: timestamp },
      { taskId: "task-b", eventId: "event-b", linkKind: "scheduled", createdAt: timestamp },
    ],
    taskChangeEvents: [],
    viewPreferences: [],
    customEmojis: [],
  };
}

function relationships(): ProjectsOptionalData {
  return {
    kind: "relationships",
    projectId: "project-a",
    checklistItems: [],
    tags: [
      { id: "tag-a-new", projectId: "project-a", name: "New", sortOrder: 0, createdAt: timestamp, updatedAt: timestamp },
    ],
    taskTagLinks: [{ taskId: "task-a", tagId: "tag-a-new", createdAt: timestamp }],
    customFields: [],
    customFieldOptions: [],
    customFieldValues: [],
    customFieldOptionValues: [],
    dependencies: [
      { id: "dependency-a-new", blockingTaskId: "task-a", blockedTaskId: "task-a", dependencyType: "blocks", createdAt: timestamp },
    ],
    eventLinks: [
      { taskId: "task-a", eventId: "event-a-new", linkKind: "reference", createdAt: timestamp },
    ],
    taskChangeEvents: [],
    viewPreferences: [],
    customEmojis: [],
  };
}

describe("project optional data", () => {
  it("loads view preferences only for List, which renders saved columns and widths", () => {
    expect(projectViewOptionalDataKinds("list")).toEqual(["saved_views"]);
    for (const view of ["kanban", "calendar", "gantt", "dashboard"] as const) {
      expect(projectViewOptionalDataKinds(view)).toEqual([]);
    }
  });

  it("replaces one project's relationships without dropping another project", () => {
    const merged = mergeProjectOptionalData(snapshot(), relationships());

    expect(merged.tags.map((tag) => tag.id)).toEqual(["tag-b", "tag-a-new"]);
    expect(merged.taskTagLinks.map((link) => link.tagId)).toEqual(["tag-b", "tag-a-new"]);
    expect(merged.dependencies.map((dependency) => dependency.id)).toEqual([
      "dependency-b",
      "dependency-a-new",
    ]);
    expect(merged.eventLinks.map((link) => link.eventId)).toEqual([
      "event-b",
      "event-a-new",
    ]);
  });
});
