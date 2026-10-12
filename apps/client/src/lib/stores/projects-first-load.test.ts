// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import { loadProjectTaskView, updateProjectTask } from "$lib/api/projects";
import { PROJECT_TASK_FILTER_DEFAULTS } from "$lib/projects/list/view";
import type {
  ProjectOptionalDataKind,
  ProjectsOptionalData,
  ProjectsSnapshot,
  ProjectsWorkspaceSnapshot,
  ProjectTaskDetailData,
  ProjectTaskViewPage,
  ProjectMutation,
} from "$lib/projects/types";

const backend = vi.hoisted(() => {
  let resolveWorkspace: ((value: unknown) => void) | undefined;
  const workspace = new Promise<unknown>((resolve) => {
    resolveWorkspace = resolve;
  });
  const optionalResolvers = new Map<string, (value: unknown) => void>();
  const optionalCalls: string[] = [];
  return {
    workspace,
    workspaceCalls: 0,
    optionalCalls,
    detailCalls: 0,
    resolveWorkspace(value: unknown) {
      resolveWorkspace?.(value);
    },
    optionalRequest(kind: string, projectId: string | null) {
      const key = `${projectId ?? "global"}:${kind}`;
      optionalCalls.push(key);
      return new Promise<unknown>((resolve) => {
        optionalResolvers.set(key, resolve);
      });
    },
    resolveOptional(kind: string, projectId: string | null, value: unknown) {
      optionalResolvers.get(`${projectId ?? "global"}:${kind}`)?.(value);
    },
    loadDetail() {
      this.detailCalls += 1;
      const task = emptySnapshot().tasks[0];
      return Promise.resolve({
        task,
        relatedTasks: [], checklistItems: [], tags: [], taskTagLinks: [], customFields: [],
        customFieldOptions: [], customFieldValues: [], customFieldOptionValues: [],
        dependencies: [], eventLinks: [], taskChangeEvents: [],
      } satisfies ProjectTaskDetailData);
    },
  };
});

vi.mock("$lib/api/projects", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/projects")>();
  return {
    ...actual,
    loadProjectsWorkspace: () => {
      backend.workspaceCalls += 1;
      return backend.workspace;
    },
    refreshProjectsWorkspace: vi.fn(),
    loadProjectsOptionalData: (kind: string, projectId: string | null) =>
      backend.optionalRequest(kind, projectId),
    loadProjectTaskDetail: () => backend.loadDetail(),
    loadProjectTaskView: vi.fn(),
    updateProjectTask: vi.fn(),
  };
});

function emptySnapshot(): ProjectsSnapshot {
  return {
    groups: [{
      id: "group-1",
      name: "Group",
      icon: "lucide:folder",
      sortOrder: 0,
      collapsed: false,
      createdAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z",
    }],
    projects: [{
      id: "project-1",
      groupId: "group-1",
      name: "Project",
      icon: "lucide:folder",
      color: 8,
      sortOrder: 0,
      status: "active",
      defaultEventName: null,
      defaultEventTimeMode: "timed",
      defaultEventDurationMinutes: 60,
      defaultPomodoroMode: "preset",
      defaultPomodoroPresetKey: "adaptive",
      defaultIdleSettingsSource: "global",
      defaultIdlePauseEnabled: true,
      defaultIdleThresholdMinutes: 5,
      createdAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z",
    }],
    sections: [],
    statuses: [],
    priorities: [],
    tasks: [{
      id: "task-1", projectId: "project-1", sectionId: "section-1", statusId: "status-1",
      title: "Task", description: "", priority: "normal", taskType: "task",
      sectionSortOrder: 0, statusSortOrder: 0, milestone: false,
      createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z",
      detailLoaded: false,
    }],
    checklistItems: [],
    tags: [],
    taskTagLinks: [],
    customFields: [],
    customFieldOptions: [],
    customFieldValues: [],
    customFieldOptionValues: [],
    dependencies: [],
    eventLinks: [],
    taskChangeEvents: [],
    viewPreferences: [],
    customEmojis: [],
  };
}

function optionalData(
  kind: ProjectOptionalDataKind,
  projectId: string | null = "project-1",
): ProjectsOptionalData {
  return {
    kind,
    projectId,
    checklistItems: [],
    tags: [],
    taskTagLinks: [],
    customFields: [],
    customFieldOptions: [],
    customFieldValues: [],
    customFieldOptionValues: [],
    dependencies: [],
    eventLinks: [],
    taskChangeEvents: [],
    viewPreferences: [],
    customEmojis: [],
  };
}

describe("Projects initial loading", () => {
  it("uses one workspace request and single-flights view and panel data", async () => {
    const { getProjects } = await import("./projects.svelte");
    const projects = getProjects();
    const firstLoad = projects.ensureLoaded();
    const secondLoad = projects.ensureLoaded();

    expect(backend.workspaceCalls).toBe(1);
    backend.resolveWorkspace({
      resolvedProjectId: "project-1",
      activeView: "list",
      snapshot: emptySnapshot(),
    } satisfies ProjectsWorkspaceSnapshot);
    await Promise.all([firstLoad, secondLoad]);
    expect(projects.selectedProjectId).toBe("project-1");
    expect(projects.isProjectDataLoaded("project-1")).toBe(true);
    expect(backend.workspaceCalls).toBe(1);

    await projects.ensureProjectViewData("project-1", "kanban");
    expect(backend.optionalCalls).toEqual([]);
    expect(projects.isProjectViewDataLoaded("project-1", "kanban")).toBe(true);

    const list = projects.ensureProjectViewData("project-1", "list");
    const toolbar = projects.ensureProjectToolbarData("project-1");
    expect(backend.optionalCalls).toEqual(["project-1:saved_views"]);
    expect(projects.isProjectViewDataLoaded("project-1", "list")).toBe(false);
    backend.resolveOptional("saved_views", "project-1", optionalData("saved_views"));
    await Promise.all([list, toolbar]);
    expect(projects.isProjectViewDataLoaded("project-1", "list")).toBe(true);

    await Promise.all([
      projects.ensureTaskDetailData("project-1", "task-1"),
      projects.ensureTaskDetailData("project-1", "task-1"),
    ]);
    expect(backend.detailCalls).toBe(1);
    expect(backend.optionalCalls).toEqual(["project-1:saved_views"]);
  });

  it("advances query revisions for applied writes but keeps page merges, failed writes, and stale responses neutral", async () => {
    const { getProjects } = await import("./projects.svelte");
    const projects = getProjects();
    backend.resolveWorkspace({resolvedProjectId: "project-1", activeView: "list", snapshot: emptySnapshot()} satisfies ProjectsWorkspaceSnapshot);
    await projects.ensureLoaded();
    const task = {...emptySnapshot().tasks[0], detailLoaded: true};
    const page: ProjectTaskViewPage = {
      projectId: "project-1", view: "list", tasks: [task], totalCount: 1, matchedCount: 1, archivedCount: 0,
      columnCounts: [], matchedEventIds: [], taskTagLinks: [], customFieldValues: [], customFieldOptionValues: [],
      dependencies: [], eventLinks: [], tags: [], customFields: [], customFieldOptions: [],
    };
    vi.mocked(loadProjectTaskView).mockResolvedValue(page);
    const initialRevision = projects.taskMutationRevision;
    await projects.loadTaskView({...PROJECT_TASK_FILTER_DEFAULTS, projectId: "project-1", view: "list", pageSize: 100, columnCursors: {}, showArchived: false,
      visibleSectionIds: [], today: "2026-07-11", weekEnd: "2026-07-18", candidateEventIds: []});
    expect(projects.taskMutationRevision).toBe(initialRevision);
    const mutation = (title: string): ProjectMutation => ({changed: {...emptySnapshot(), groups: [], projects: [], tasks: [{...task, title}]}, removals: [], calendarEventProjectAssignments: []});
    vi.mocked(updateProjectTask).mockResolvedValueOnce(mutation("Updated"));
    await projects.updateTask(task, {title: "Updated"});
    expect(projects.taskMutationRevision).toBe(initialRevision + 1);
    vi.mocked(updateProjectTask).mockRejectedValueOnce(new Error("Transaction failed"));
    await expect(projects.updateTask(task, {title: "Failed"})).rejects.toThrow("Transaction failed");
    expect(projects.taskMutationRevision).toBe(initialRevision + 1);
    let finishOlder!: (value: ProjectMutation) => void;
    vi.mocked(updateProjectTask).mockImplementationOnce(() => new Promise<ProjectMutation>((resolve) => { finishOlder = resolve; }))
      .mockResolvedValueOnce(mutation("Newest"));
    const older = projects.updateTask(task, {title: "Older"});
    await projects.updateTask(task, {title: "Newest"});
    finishOlder(mutation("Older"));
    await older;
    expect(projects.taskById(task.id)?.title).toBe("Newest");
    expect(projects.taskMutationRevision).toBe(initialRevision + 2);
  });
});
