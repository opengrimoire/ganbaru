import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyProjectDependencyCascade, type ProjectDependencyCascadePreview } from "$lib/api/project-cascade";
import { applyProjectTaskBulk, createProjectCustomField, createProjectPriority, createProjectStatus, createProjectTag, deleteProjectCustomField, linkProjectTaskEvent, reorderProjectItem, updateProjectTask } from "$lib/api/projects";
import { createProjectStoreActions } from "./actions";
import { createProjectStoreSelectors } from "./selectors";
import actionSource from "./actions.ts?raw";
import type {
  ProjectMutation,
  ProjectsSnapshot,
  ProjectTask,
  ProjectTaskUpdate,
} from "$lib/projects/types";

vi.mock("$lib/api/projects", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/projects")>(),
  updateProjectTask: vi.fn(),
  applyProjectTaskBulk: vi.fn(),
  createProjectCustomField: vi.fn(),
  reorderProjectItem: vi.fn(),
  deleteProjectCustomField: vi.fn(),
  linkProjectTaskEvent: vi.fn(),
  createProjectStatus: vi.fn(),
  createProjectPriority: vi.fn(),
  createProjectTag: vi.fn(),
}));
vi.mock("$lib/api/project-cascade", () => ({ applyProjectDependencyCascade: vi.fn() }));

function emptySnapshot(): ProjectsSnapshot {
  return {
    groups: [], projects: [], sections: [], statuses: [], priorities: [], tasks: [],
    checklistItems: [], tags: [], taskTagLinks: [], customFields: [], customFieldOptions: [],
    customFieldValues: [], customFieldOptionValues: [], dependencies: [], eventLinks: [],
    taskChangeEvents: [], viewPreferences: [], customEmojis: [],
  };
}

const task: ProjectTask = {
  id: "task-1", projectId: "project-1", sectionId: "section-1", statusId: "status-1",
  title: "Before", description: "", priority: "none", taskType: "task",
  sectionSortOrder: 1000, statusSortOrder: 1000, milestone: false,
  createdAt: "created", updatedAt: "before", detailLoaded: true,
};

function taskMutation(changedTask: ProjectTask): ProjectMutation {
  return {
    changed: { ...emptySnapshot(), tasks: [changedTask] },
    removals: [],
    calendarEventProjectAssignments: [],
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

describe("createProjectStoreActions", () => {
  beforeEach(() => vi.clearAllMocks());

  it("contains no ordinary mutation path that follows success with a snapshot reload", () => {
    expect(actionSource).not.toMatch(/\breload\s*\(/u);
  });

  function setup() {
    let snapshot = { ...emptySnapshot(), tasks: [task] };
    let loadGeneration = 0;
    const reloaded = { ...emptySnapshot(), tasks: [{ ...task, title: "Forced", updatedAt: "forced" }] };
    const reload = vi.fn(async (_projectId?: string | null) => {
      loadGeneration += 1;
      snapshot = reloaded;
    });
    const applyCalendarEventProjectAssignments = vi.fn(async () => undefined);
    const actions = createProjectStoreActions({
      selectors: createProjectStoreSelectors(() => snapshot),
      readSnapshot: () => snapshot,
      readLoadGeneration: () => loadGeneration,
      updateSnapshot: (updater) => { snapshot = updater(snapshot); },
      applyCalendarEventProjectAssignments,
      readSelectedProjectId: () => "project-1",
      setSelectedProjectId: vi.fn(),
      reload,
      ensureProjectData: vi.fn(async () => undefined),
      ensureTaskDetailData: vi.fn(async (_projectId: string, taskId: string) => {
        snapshot = {
          ...snapshot,
          tasks: snapshot.tasks.map((task) => task.id === taskId ? { ...task, detailLoaded: true } : task),
        };
      }),
    });
    return { actions, readSnapshot: () => snapshot, reload, applyCalendarEventProjectAssignments };
  }

  it("preserves settings draft identities and ranks when creating collection rows", async () => {
    const dates = { createdAt: "2026-10-01T00:00:00Z", updatedAt: "2026-10-01T00:00:00Z" };
    vi.mocked(createProjectStatus).mockImplementation(async (entry) => ({ changed: { ...emptySnapshot(), statuses: [{ ...entry, ...dates }] }, removals: [], calendarEventProjectAssignments: [] }));
    vi.mocked(createProjectPriority).mockImplementation(async (entry) => ({ changed: { ...emptySnapshot(), priorities: [{ ...entry, ...dates }] }, removals: [], calendarEventProjectAssignments: [] }));
    vi.mocked(createProjectTag).mockImplementation(async (entry) => ({ changed: { ...emptySnapshot(), tags: [{ ...entry, ...dates, color: 8 as const }] }, removals: [], calendarEventProjectAssignments: [] }));
    const { actions, readSnapshot } = setup();
    await actions.addStatus("project-1", "Ready", "active", 8, { id: "draft-status", sortOrder: 4 });
    await actions.addPriority("project-1", "Medium", 8, { id: "draft-priority", sortOrder: 5 });
    await actions.addTag("project-1", "Work", 8, { id: "draft-tag", sortOrder: 6 });
    expect(createProjectStatus).toHaveBeenCalledWith(expect.objectContaining({ id: "draft-status", sortOrder: 4 }));
    expect(createProjectPriority).toHaveBeenCalledWith(expect.objectContaining({ id: "draft-priority", sortOrder: 5 }));
    expect(createProjectTag).toHaveBeenCalledWith(expect.objectContaining({ id: "draft-tag", sortOrder: 6 }));
    expect(readSnapshot().tags[0].id).toBe("draft-tag");
    await expect(actions.addTag("project-1", "Work", 8, { id: "another-draft", sortOrder: 7 })).rejects.toThrow("A tag with this name already exists");
    expect(createProjectTag).toHaveBeenCalledOnce();
    expect((await actions.addTag("project-1", "Work"))?.id).toBe("draft-tag");
  });

  it("applies the reviewed cascade once and reconciles tasks absent from the visible cache", async () => {
    const context = setup();
    const hidden = { ...task, id: "unloaded", startDate: "2026-06-13", revision: 3 };
    vi.mocked(applyProjectDependencyCascade).mockResolvedValue(taskMutation(hidden));
    const preview: ProjectDependencyCascadePreview = { projectId: task.projectId, digest: "1".repeat(64), items: [], conflicts: [] };
    await context.actions.applyDependencyCascade(preview);
    expect(applyProjectDependencyCascade).toHaveBeenCalledExactlyOnceWith({ operationId: expect.any(String), projectId: task.projectId, reviewedDigest: preview.digest });
    expect(context.readSnapshot().tasks.find((row) => row.id === hidden.id)).toEqual(hidden);
    expect(updateProjectTask).not.toHaveBeenCalled();
  });

  it("retains cascade retry identity while preserving a newer task revision", async () => {
    const context = setup();
    const preview: ProjectDependencyCascadePreview = { projectId: task.projectId, digest: "1".repeat(64), items: [], conflicts: [] };
    vi.mocked(applyProjectDependencyCascade).mockRejectedValueOnce(new Error("lost response"));
    await expect(context.actions.applyDependencyCascade(preview)).rejects.toThrow("lost response");
    vi.mocked(updateProjectTask).mockResolvedValue(taskMutation({ ...task, title: "Newer", revision: 5 }));
    await context.actions.updateTask(task, { title: "Newer" });
    vi.mocked(applyProjectDependencyCascade).mockResolvedValue(taskMutation({ ...task, title: "Older receipt", revision: 3 }));
    await context.actions.applyDependencyCascade(preview);
    expect(vi.mocked(applyProjectDependencyCascade).mock.calls[0]).toEqual(vi.mocked(applyProjectDependencyCascade).mock.calls[1]);
    expect(context.readSnapshot().tasks[0].title).toBe("Newer");
  });

  it("rejects a cascade when Project context changes while its command client loads", async () => {
    const context = setup();
    const preview: ProjectDependencyCascadePreview = { projectId: task.projectId, digest: "1".repeat(64), items: [], conflicts: [] };
    const pending = context.actions.applyDependencyCascade(preview);
    const failure = expect(pending).rejects.toThrow("Project context changed");
    await context.reload();
    await failure;
    expect(applyProjectDependencyCascade).not.toHaveBeenCalled();
    expect(context.readSnapshot().tasks[0].title).toBe("Forced");
  });

  it("patches an authoritative task result with no follow-up snapshot load", async () => {
    vi.mocked(updateProjectTask).mockImplementation(async (update: ProjectTaskUpdate) =>
      taskMutation({ ...task, title: update.title, updatedAt: "server" }));
    const context = setup();

    await context.actions.updateTask(task, { title: "After" });

    expect(context.readSnapshot().tasks[0]).toMatchObject({ title: "After", updatedAt: "server" });
    expect(context.reload).not.toHaveBeenCalled();
  });

  it("reorders from one loaded task and reconciles an unloaded native sibling", async () => {
    const sibling = { ...task, id: "task-hidden", sectionSortOrder: 1000, revision: 2 };
    vi.mocked(reorderProjectItem).mockResolvedValue({ ...taskMutation(task), changed: {
      ...emptySnapshot(), tasks: [{ ...task, sectionSortOrder: 2000, revision: 1 }, sibling],
    } });
    const context = setup();
    await context.actions.moveTaskInSection(task, 1);
    expect(reorderProjectItem).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
      projectId: task.projectId, direction: 1,
      item: { kind: "task", id: task.id, axis: "section", groupId: task.sectionId, parentTaskId: null, expectedOrder: 1000 },
    }));
    expect(context.readSnapshot().tasks.map((task) => task.id)).toEqual([task.id, sibling.id]);
    expect(updateProjectTask).not.toHaveBeenCalled();
  });

  it("retains reorder identity after failure and does not resurrect a later deleted field", async () => {
    const field = { id: "field-1", projectId: "project-1", name: "Field", fieldType: "text" as const,
      sortOrder: 1000, icon: null, revision: 0, createdAt: "created", updatedAt: "updated" };
    const context = setup();
    vi.mocked(reorderProjectItem).mockRejectedValueOnce(new Error("lost response"));
    await expect(context.actions.moveCustomField(field, 1)).rejects.toThrow("lost response");
    const first = vi.mocked(reorderProjectItem).mock.calls[0][0];
    vi.mocked(deleteProjectCustomField).mockResolvedValue({ changed: emptySnapshot(), removals: [{ kind: "custom_field", id: field.id }], calendarEventProjectAssignments: [] });
    await context.actions.removeCustomField(field.id);
    vi.mocked(reorderProjectItem).mockResolvedValue({ changed: { ...emptySnapshot(), customFields: [{ ...field, sortOrder: 2000, revision: 1 }] }, removals: [], calendarEventProjectAssignments: [] });
    await context.actions.moveCustomField(field, 1);
    expect(vi.mocked(reorderProjectItem).mock.calls[1][0]).toEqual(first);
    expect(context.readSnapshot().customFields).toEqual([]);
  });

  it("skips duplicating a property when the new name is blank", async () => {
    const field = { id: "field-1", projectId: "project-1", name: "Field", fieldType: "text" as const,
      sortOrder: 1000, icon: null, revision: 0, createdAt: "created", updatedAt: "updated" };
    const context = setup();
    await expect(context.actions.duplicateCustomField(field, "   ")).resolves.toBeUndefined();
    expect(createProjectCustomField).not.toHaveBeenCalled();
    expect(context.readSnapshot().customFields).toEqual([]);
  });

  it("keeps the snapshot unchanged when the command transaction fails", async () => {
    vi.mocked(updateProjectTask).mockRejectedValue(new Error("transaction rolled back"));
    const context = setup();

    await expect(context.actions.updateTask(task, { title: "After" })).rejects.toThrow("transaction rolled back");

    expect(context.readSnapshot().tasks[0]).toEqual(task);
    expect(context.reload).not.toHaveBeenCalled();
  });

  it("archives the selected roots once and reconciles native descendants outside the loaded page", async () => {
    const context = setup();
    const child = { ...task, id: "hidden-child", parentTaskId: task.id, archivedAt: "native-time" };
    vi.mocked(applyProjectTaskBulk).mockResolvedValue({
      ...taskMutation({ ...task, archivedAt: "native-time" }),
      changed: { ...emptySnapshot(), tasks: [{ ...task, archivedAt: "native-time" }, child] },
    });

    await context.actions.archiveTasks([task]);

    expect(applyProjectTaskBulk).toHaveBeenCalledExactlyOnceWith({
      operationId: expect.any(String),
      projectId: task.projectId,
      change: { kind: "archive", archived: true },
      tasks: [{ id: task.id, value: null }],
    });
    expect(context.readSnapshot().tasks).toContainEqual(child);
    expect(updateProjectTask).not.toHaveBeenCalled();
  });

  it("retains bulk retry identity after a lost response without publishing unconfirmed state", async () => {
    const context = setup();
    vi.mocked(applyProjectTaskBulk)
      .mockRejectedValueOnce(new Error("response lost"))
      .mockResolvedValueOnce(taskMutation({ ...task, priority: "high" }));

    await expect(context.actions.setTasksPriority([task], "high")).rejects.toThrow("response lost");
    expect(context.readSnapshot().tasks[0]).toEqual(task);
    await context.actions.setTasksPriority([task], "high");

    const calls = vi.mocked(applyProjectTaskBulk).mock.calls;
    expect(calls[0][0]).toEqual(calls[1][0]);
    expect(calls[0][0]).toMatchObject({
      change: { kind: "priority", priority: "high" },
      tasks: [{ id: task.id, value: task.priority }],
    });
    expect(context.readSnapshot().tasks[0].priority).toBe("high");
    expect(updateProjectTask).not.toHaveBeenCalled();
  });

  it("rejects a mixed project bulk selection before invoking persistence", async () => {
    const context = setup();
    await expect(context.actions.setTasksPriority([
      task, { ...task, id: "other", projectId: "other-project" },
    ], "high")).rejects.toThrow("same project");
    expect(applyProjectTaskBulk).not.toHaveBeenCalled();
  });

  it("ignores an older response for the same entity", async () => {
    const first = deferred<ProjectMutation>();
    const second = deferred<ProjectMutation>();
    vi.mocked(updateProjectTask)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const context = setup();
    const staleRequest = context.actions.updateTask(task, { title: "Old response" });
    const latestRequest = context.actions.updateTask(task, { title: "New response" });

    second.resolve(taskMutation({ ...task, title: "New response", updatedAt: "new" }));
    await latestRequest;
    first.resolve(taskMutation({ ...task, title: "Old response", updatedAt: "old" }));
    await staleRequest;

    expect(context.readSnapshot().tasks[0]).toMatchObject({ title: "New response", updatedAt: "new" });
  });

  it("allows an explicit forced reload to reconcile a local patch", async () => {
    vi.mocked(updateProjectTask).mockResolvedValue(
      taskMutation({ ...task, title: "Local patch", updatedAt: "local" }),
    );
    const context = setup();
    await context.actions.updateTask(task, { title: "Local patch" });

    await context.reload("project-1");

    expect(context.readSnapshot().tasks[0]).toMatchObject({ title: "Forced", updatedAt: "forced" });
    expect(context.reload).toHaveBeenCalledOnce();
  });

  it("does not apply a mutation response that predates a forced reload", async () => {
    const pending = deferred<ProjectMutation>();
    vi.mocked(updateProjectTask).mockReturnValue(pending.promise);
    const context = setup();
    const request = context.actions.updateTask(task, { title: "Stale" });

    await context.reload("project-1");
    pending.resolve(taskMutation({ ...task, title: "Stale", updatedAt: "stale" }));
    await request;

    expect(context.readSnapshot().tasks[0]).toMatchObject({ title: "Forced", updatedAt: "forced" });
  });

  it("patches event links and forwards a server-derived calendar assignment", async () => {
    vi.mocked(linkProjectTaskEvent).mockResolvedValue({
      changed: {
        ...emptySnapshot(),
        eventLinks: [{ taskId: "task-1", eventId: "event-1", linkKind: "scheduled", createdAt: "server" }],
      },
      removals: [],
      calendarEventProjectAssignments: [{ eventId: "event-1", projectId: "project-1" }],
    });
    const context = setup();

    await context.actions.linkTaskEvent("task-1", "event-1");

    expect(context.readSnapshot().eventLinks).toHaveLength(1);
    expect(context.applyCalendarEventProjectAssignments).toHaveBeenCalledWith([
      { eventId: "event-1", projectId: "project-1" },
    ]);
    expect(context.reload).not.toHaveBeenCalled();
  });
});
