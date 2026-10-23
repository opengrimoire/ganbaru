import { describe, expect, it } from "vitest";
import type { ProjectConflicts, ProjectDependencyConflict, ProjectFieldConflict } from "$lib/api/project-conflicts";
import {
  canRestoreDependency,
  dependencyConflictsForTask,
  findFieldConflict,
  previewVersionText,
} from "./conflicts";

const device = { deviceId: "device-a", deviceLabel: null, ownDevice: true };

function field(overrides: Partial<ProjectFieldConflict>): ProjectFieldConflict {
  return {
    target: "task",
    id: "task-1",
    field: "title",
    versions: [{ version: "v1", device, editedAtMs: 1, displayed: true, text: "A" }],
    ...overrides,
  };
}

function dependency(overrides: Partial<ProjectDependencyConflict>): ProjectDependencyConflict {
  return {
    key: "key",
    blockingTaskId: "task-1",
    blockedTaskId: "task-2",
    blockingTaskTitle: "One",
    blockedTaskTitle: "Two",
    wouldCycle: false,
    device,
    addedAtMs: 1,
    ...overrides,
  };
}

function conflicts(fields: ProjectFieldConflict[], dependencies: ProjectDependencyConflict[] = []): ProjectConflicts {
  return { projectId: "project-1", fields, dependencies };
}

describe("project conflict lookup", () => {
  it("finds a conflict by row kind, id, and field", () => {
    const title = field({});
    const description = field({ field: "description" });
    const other = field({ id: "task-2" });
    const read = conflicts([other, description, title]);
    expect(findFieldConflict(read, "task", "task-1", "title")).toBe(title);
    expect(findFieldConflict(read, "task", "task-1", "description")).toBe(description);
  });

  it("does not match another row kind with the same id", () => {
    const read = conflicts([field({ target: "checklist_item", id: "shared" })]);
    expect(findFieldConflict(read, "task", "shared", "title")).toBeUndefined();
  });

  it("ignores a conflict without versions and a missing read", () => {
    expect(findFieldConflict(conflicts([field({ versions: [] })]), "task", "task-1", "title")).toBeUndefined();
    expect(findFieldConflict(undefined, "task", "task-1", "title")).toBeUndefined();
  });

  it("lists hidden dependencies on either side of a task", () => {
    const blocking = dependency({ key: "a", blockingTaskId: "task-1", blockedTaskId: "task-2" });
    const blocked = dependency({ key: "b", blockingTaskId: "task-3", blockedTaskId: "task-1" });
    const unrelated = dependency({ key: "c", blockingTaskId: "task-3", blockedTaskId: "task-4" });
    const read = conflicts([], [blocking, unrelated, blocked]);
    expect(dependencyConflictsForTask(read, "task-1")).toEqual([blocking, blocked]);
    expect(dependencyConflictsForTask(undefined, "task-1")).toEqual([]);
  });
});

describe("hidden dependency restore", () => {
  it("is available only while adding it again would not recreate the cycle", () => {
    expect(canRestoreDependency({ wouldCycle: false })).toBe(true);
    expect(canRestoreDependency({ wouldCycle: true })).toBe(false);
  });
});

describe("version preview", () => {
  it("keeps short text whole", () => {
    expect(previewVersionText("Short", 10)).toBe("Short");
    expect(previewVersionText("1234567890", 10)).toBe("1234567890");
  });

  it("cuts long text on characters rather than code units", () => {
    expect(previewVersionText("ab😀cd", 3)).toBe("ab😀…");
  });

  it("drops trailing whitespace before the ellipsis", () => {
    expect(previewVersionText("abc   def", 6)).toBe("abc…");
  });
});
