import { describe, expect, it } from "vitest";
import {
  buildProjectLoadedDataIndex,
  createProjectLoadedDataIndexReader,
} from "./loaded-data-index";
import * as indexed from "./queries";
import { applyProjectMutation } from "./mutations";
import { TASK_LIST_COLUMNS_PREFERENCE_KEY } from "$lib/projects/tasks/list-columns";
import { createProjectStoreSelectors } from "$lib/stores/projects/selectors";
import type {
  Project,
  ProjectsSnapshot,
  ProjectTask,
} from "$lib/projects/types";

const createdAt = "2026-07-11T00:00:00.000Z";
const updatedAt = "2026-07-11T01:00:00.000Z";

function emptySnapshot(): ProjectsSnapshot {
  return {
    groups: [], projects: [], sections: [], statuses: [], priorities: [], tasks: [],
    checklistItems: [], tags: [], taskTagLinks: [], customFields: [], customFieldOptions: [],
    customFieldValues: [], customFieldOptionValues: [], dependencies: [], eventLinks: [],
    taskChangeEvents: [], viewPreferences: [], views: [], customEmojis: [],
  };
}

function project(id: string, groupId: string, status: Project["status"], sortOrder: number): Project {
  return {
    id, groupId, name: id, icon: "lucide:folder", sortOrder, status,
    defaultEventName: null, defaultEventTimeMode: "timed", defaultEventDurationMinutes: null,
    defaultPomodoroMode: "preset", defaultIdleSettingsSource: "global",
    defaultIdlePauseEnabled: true, defaultIdleThresholdMinutes: 5, createdAt, updatedAt,
  };
}

function task(
  id: string,
  projectId: string,
  sectionId: string,
  statusId: string,
  order: number,
  parentTaskId?: string,
  archivedAt?: string,
): ProjectTask {
  return {
    id, projectId, sectionId, statusId, parentTaskId, archivedAt, title: id, description: "",
    priority: "none", taskType: "task", sectionSortOrder: order, statusSortOrder: order * 2,
    milestone: false, createdAt: `${createdAt.slice(0, -5)}${String(order % 1000).padStart(3, "0")}Z`, updatedAt,
  };
}

function nestedFixture(): ProjectsSnapshot {
  return {
    groups: [
      { id: "group-b", name: "B", icon: "lucide:folder", sortOrder: 2000, collapsed: false, createdAt, updatedAt },
      { id: "group-a", name: "A", icon: "lucide:folder", sortOrder: 1000, collapsed: false, createdAt, updatedAt },
      { id: "group-hidden", name: "Hidden", icon: "lucide:folder", sortOrder: 500, collapsed: false, hiddenAt: updatedAt, createdAt, updatedAt },
    ],
    projects: [
      project("project-b", "group-a", "archived", 500),
      project("project-a", "group-a", "active", 2000),
      project("project-c", "group-b", "active", 1000),
    ],
    sections: [
      { id: "section-hidden", projectId: "project-a", name: "Hidden", sortOrder: 500, collapsed: false, hiddenAt: updatedAt, createdAt, updatedAt },
      { id: "section-b", projectId: "project-a", name: "Later", sortOrder: 2000, collapsed: false, createdAt, updatedAt },
      { id: "section-a", projectId: "project-a", name: "First", sortOrder: 1000, collapsed: false, createdAt, updatedAt },
    ],
    statuses: [
      { id: "status-done", projectId: "project-a", name: "Done", category: "done", color: 2, sortOrder: 2000, terminal: true, createdAt, updatedAt },
      { id: "status-todo", projectId: "project-a", name: "To do", category: "not_started", color: 1, sortOrder: 1000, terminal: false, createdAt, updatedAt },
    ],
    priorities: [
      { id: "priority-b", projectId: "project-a", name: "Low", color: 2, sortOrder: 2000, createdAt, updatedAt },
      { id: "priority-a", projectId: "project-a", name: "High", color: 1, sortOrder: 1000, createdAt, updatedAt },
    ],
    tasks: [
      task("task-child-b", "project-a", "section-a", "status-todo", 3000, "task-parent"),
      task("task-archived", "project-a", "section-a", "status-todo", 500, undefined, updatedAt),
      task("task-parent", "project-a", "section-a", "status-todo", 1000),
      task("task-child-a", "project-a", "section-a", "status-done", 2000, "task-parent"),
      task("task-other", "project-c", "section-other", "status-other", 1000),
    ],
    checklistItems: [
      { id: "item-b", taskId: "task-parent", title: "B", sortOrder: 2000, createdAt, updatedAt },
      { id: "item-a", taskId: "task-parent", title: "A", sortOrder: 1000, createdAt, updatedAt },
    ],
    tags: [
      { id: "tag-b", projectId: "project-a", name: "Beta", sortOrder: 2000, createdAt, updatedAt },
      { id: "tag-a", projectId: "project-a", name: "Alpha", sortOrder: 1000, createdAt, updatedAt },
    ],
    taskTagLinks: [{ taskId: "task-parent", tagId: "tag-b", createdAt }],
    customFields: [
      { id: "field-b", projectId: "project-a", name: "Text", fieldType: "text", sortOrder: 2000, icon: null, createdAt, updatedAt },
      { id: "field-a", projectId: "project-a", name: "Choice", fieldType: "select", sortOrder: 1000, icon: null, createdAt, updatedAt },
    ],
    customFieldOptions: [
      { id: "option-b", fieldId: "field-a", name: "B", sortOrder: 2000, createdAt, updatedAt },
      { id: "option-a", fieldId: "field-a", name: "A", sortOrder: 1000, createdAt, updatedAt },
    ],
    customFieldValues: [{ taskId: "task-parent", fieldId: "field-b", textValue: "value", updatedAt }],
    customFieldOptionValues: [{ taskId: "task-parent", fieldId: "field-a", optionId: "option-b", createdAt }],
    dependencies: [{ id: "dependency-a", blockingTaskId: "task-child-a", blockedTaskId: "task-parent", dependencyType: "blocks", createdAt }],
    eventLinks: [{ taskId: "task-parent", eventId: "event-a", linkKind: "scheduled", createdAt }],
    taskChangeEvents: [
      { id: "change-old", taskId: "task-parent", eventType: "updated", occurredAt: createdAt },
      { id: "change-new", taskId: "task-parent", eventType: "updated", occurredAt: updatedAt },
    ],
    viewPreferences: [{
      projectId: "project-a", viewId: "view-a-list", preferenceKey: TASK_LIST_COLUMNS_PREFERENCE_KEY,
      preferenceValue: JSON.stringify(["status", "custom:field-a"]), updatedAt,
    }],
    views: [{ id: "view-a-list", projectId: "project-a", viewType: "list", sortOrder: 1000, createdAt, updatedAt }],
    customEmojis: [{ id: "emoji-a", name: "Focus", assetPath: "project-icons/a.png", sortOrder: 3000, createdAt, updatedAt }],
  };
}

function archivedFixture(): ProjectsSnapshot {
  const fixture = nestedFixture();
  return {
    ...fixture,
    projects: fixture.projects.map((value) => ({ ...value, status: "archived" })),
    tasks: fixture.tasks.map((value) => ({ ...value, archivedAt: updatedAt })),
  };
}

function paginatedFixture(): ProjectsSnapshot {
  const fixture = nestedFixture();
  const visibleTaskIds = new Set(["task-parent", "task-child-b"]);
  return {
    ...fixture,
    tasks: fixture.tasks.filter((value) => visibleTaskIds.has(value.id)),
    checklistItems: [],
    taskChangeEvents: [],
  };
}

function denseFixture(count = 2_000): ProjectsSnapshot {
  const fixture = nestedFixture();
  return {
    ...fixture,
    tasks: Array.from({ length: count }, (_, index) =>
      task(`dense-${index}`, "project-a", index % 2 === 0 ? "section-a" : "section-b", index % 3 === 0 ? "status-done" : "status-todo", index + 1)
    ),
    checklistItems: [], taskTagLinks: [], customFieldValues: [], customFieldOptionValues: [],
    dependencies: [], eventLinks: [], taskChangeEvents: [],
  };
}

const byOrderAndName = <T extends { sortOrder: number; name: string }>(a: T, b: T) =>
  a.sortOrder - b.sortOrder || a.name.localeCompare(b.name);
const bySectionOrder = (a: ProjectTask, b: ProjectTask) =>
  a.sectionSortOrder - b.sectionSortOrder || a.createdAt.localeCompare(b.createdAt);
const byStatusOrder = (a: ProjectTask, b: ProjectTask) =>
  a.statusSortOrder - b.statusSortOrder || a.createdAt.localeCompare(b.createdAt);

function ids(values: readonly { id: string }[]): string[] {
  return values.map((value) => value.id);
}

function maxOrder(values: readonly number[]): number {
  return Math.max(0, ...values) + 1000;
}

function compareEverySelector(snapshot: ProjectsSnapshot): void {
  const projectId = "project-a";
  const groupId = "group-a";
  const taskId = "task-parent";
  const fieldId = "field-a";
  const activeProjects = snapshot.projects.filter((value) => value.status === "active").sort(byOrderAndName);
  const activeSections = snapshot.sections.filter((value) => value.projectId === projectId && !value.archivedAt && !value.hiddenAt).sort(byOrderAndName);
  const statuses = snapshot.statuses.filter((value) => value.projectId === projectId).sort(byOrderAndName);
  const priorities = snapshot.priorities.filter((value) => value.projectId === projectId).sort(byOrderAndName);
  const activeTasks = snapshot.tasks.filter((value) => value.projectId === projectId && !value.archivedAt).sort(bySectionOrder);
  const allTasks = snapshot.tasks.filter((value) => value.projectId === projectId).sort(bySectionOrder);
  const sectionTasks = snapshot.tasks.filter((value) => value.projectId === projectId && value.sectionId === "section-a" && !value.parentTaskId && !value.archivedAt).sort(bySectionOrder);
  const statusTasks = snapshot.tasks.filter((value) => value.projectId === projectId && value.statusId === "status-todo" && !value.parentTaskId && !value.archivedAt).sort(byStatusOrder);
  const subtasks = snapshot.tasks.filter((value) => value.parentTaskId === taskId && !value.archivedAt).sort(bySectionOrder);
  const allSubtasks = snapshot.tasks.filter((value) => value.parentTaskId === taskId).sort(bySectionOrder);
  const projects = snapshot.projects.filter((value) => value.groupId === groupId).sort(byOrderAndName);
  const projectTags = snapshot.tags.filter((value) => value.projectId === projectId).sort(byOrderAndName);
  const taskLinks = snapshot.taskTagLinks.filter((value) => value.taskId === taskId);
  const linkedTagIds = new Set(taskLinks.map((value) => value.tagId));
  const fields = snapshot.customFields.filter((value) => value.projectId === projectId).sort(byOrderAndName);
  const options = snapshot.customFieldOptions.filter((value) => value.fieldId === fieldId).sort(byOrderAndName);
  const selectedOptionIds = new Set(snapshot.customFieldOptionValues.filter((value) => value.taskId === taskId && value.fieldId === fieldId).map((value) => value.optionId));
  const history = snapshot.taskChangeEvents.filter((value) => value.taskId === taskId).sort((a, b) => b.occurredAt.localeCompare(a.occurredAt));
  const projectTaskIds = new Set(snapshot.tasks.filter((value) => value.projectId === projectId).map((value) => value.id));
  const projectHistory = snapshot.taskChangeEvents.filter((value) => projectTaskIds.has(value.taskId)).sort((a, b) => b.occurredAt.localeCompare(a.occurredAt));

  expect(ids(indexed.activeProjects(snapshot))).toEqual(ids(activeProjects));
  expect(indexed.firstProjectId(snapshot)).toBe(activeProjects[0]?.id ?? snapshot.projects[0]?.id ?? null);
  expect(indexed.projectById(snapshot, projectId)).toBe(snapshot.projects.find((value) => value.id === projectId));
  expect(indexed.groupById(snapshot, groupId)).toBe(snapshot.groups.find((value) => value.id === groupId));
  expect(ids(indexed.sectionsForProject(snapshot, projectId))).toEqual(ids(activeSections));
  expect(ids(indexed.sectionsForProjectIncludingInactive(snapshot, projectId))).toEqual(ids(snapshot.sections.filter((value) => value.projectId === projectId).sort(byOrderAndName)));
  expect(ids(indexed.statusesForProject(snapshot, projectId))).toEqual(ids(statuses));
  expect(ids(indexed.prioritiesForProject(snapshot, projectId))).toEqual(ids(priorities));
  expect(ids(indexed.tasksForProject(snapshot, projectId))).toEqual(ids(activeTasks));
  expect(ids(indexed.tasksForProjectIncludingArchived(snapshot, projectId))).toEqual(ids(allTasks));
  expect(ids(indexed.topLevelTasksForSection(snapshot, projectId, "section-a"))).toEqual(ids(sectionTasks));
  expect(ids(indexed.topLevelTasksForStatus(snapshot, projectId, "status-todo"))).toEqual(ids(statusTasks));
  expect(ids(indexed.subtasksForTask(snapshot, taskId))).toEqual(ids(subtasks));
  expect(ids(indexed.subtasksForTaskIncludingArchived(snapshot, taskId))).toEqual(ids(allSubtasks));
  expect(ids(indexed.projectsForGroup(snapshot, groupId))).toEqual(ids(projects.filter((value) => value.status === "active")));
  expect(ids(indexed.projectsForGroupIncludingInactive(snapshot, groupId))).toEqual(ids(projects));
  expect(ids(indexed.visibleGroups(snapshot))).toEqual(ids(snapshot.groups.filter((value) => !value.hiddenAt && !value.archivedAt).sort(byOrderAndName)));
  expect(indexed.defaultSection(snapshot, projectId)).toBe(activeSections[0]);
  expect(indexed.defaultStatus(snapshot, projectId)).toBe(statuses.find((value) => value.name.toLowerCase() === "to do") ?? statuses.find((value) => value.category === "not_started") ?? statuses[0]);
  expect(indexed.doneStatus(snapshot, projectId)).toBe(statuses.find((value) => value.terminal));
  expect(indexed.reopenStatus(snapshot, projectId)).toBe(statuses.find((value) => value.name.toLowerCase() === "to do") ?? statuses.find((value) => !value.terminal) ?? statuses[0]);
  expect(indexed.statusById(snapshot, "status-todo")).toBe(snapshot.statuses.find((value) => value.id === "status-todo"));
  expect(indexed.taskById(snapshot, taskId)).toBe(snapshot.tasks.find((value) => value.id === taskId));
  expect(indexed.eventLinksForTask(snapshot, taskId)).toEqual(snapshot.eventLinks.filter((value) => value.taskId === taskId));
  expect(indexed.eventLinksForEvent(snapshot, "event-a")).toEqual(snapshot.eventLinks.filter((value) => value.eventId === "event-a"));
  expect(indexed.taskChangeEventsForTask(snapshot, taskId)).toEqual(history);
  expect(indexed.recentTaskChangeEventsForProject(snapshot, projectId, 1)).toEqual(projectHistory.slice(0, 1));
  expect(ids(indexed.checklistItemsForTask(snapshot, taskId))).toEqual(ids(snapshot.checklistItems.filter((value) => value.taskId === taskId).sort((a, b) => a.sortOrder - b.sortOrder || a.createdAt.localeCompare(b.createdAt))));
  expect(ids(indexed.tagsForProject(snapshot, projectId))).toEqual(ids(projectTags));
  expect(indexed.tagById(snapshot, "tag-a")).toBe(snapshot.tags.find((value) => value.id === "tag-a"));
  expect(indexed.taskTagLinksForTask(snapshot, taskId)).toEqual(taskLinks);
  expect(ids(indexed.tagsForTask(snapshot, taskId))).toEqual(ids(snapshot.tags.filter((value) => linkedTagIds.has(value.id)).sort(byOrderAndName)));
  const selectedTask = snapshot.tasks.find((value) => value.id === taskId);
  expect(ids(indexed.unlinkedTagsForTask(snapshot, selectedTask))).toEqual(
    selectedTask ? ids(projectTags.filter((value) => !linkedTagIds.has(value.id))) : [],
  );
  expect(indexed.projectTagByName(snapshot, projectId, " alpha ")).toBe(snapshot.tags.find((value) => value.projectId === projectId && value.name === "Alpha"));
  expect(ids(indexed.customFieldsForProject(snapshot, projectId))).toEqual(ids(fields));
  expect(indexed.customFieldById(snapshot, fieldId)).toBe(snapshot.customFields.find((value) => value.id === fieldId));
  expect(indexed.customFieldByName(snapshot, projectId, " choice ")).toBe(snapshot.customFields.find((value) => value.id === fieldId));
  expect(ids(indexed.customFieldOptionsForField(snapshot, fieldId))).toEqual(ids(options));
  expect(indexed.customFieldOptionByName(snapshot, fieldId, " a ")).toBe(snapshot.customFieldOptions.find((value) => value.id === "option-a"));
  expect(indexed.customFieldValueForTask(snapshot, taskId, "field-b")).toBe(snapshot.customFieldValues.find((value) => value.taskId === taskId && value.fieldId === "field-b"));
  expect(ids(indexed.customFieldOptionValuesForTask(snapshot, taskId, fieldId))).toEqual(ids(options.filter((value) => selectedOptionIds.has(value.id))));
  expect(indexed.dependenciesBlockingTask(snapshot, taskId)).toEqual(snapshot.dependencies.filter((value) => value.blockedTaskId === taskId));
  expect(indexed.dependenciesBlockedByTask(snapshot, "task-child-a")).toEqual(snapshot.dependencies.filter((value) => value.blockingTaskId === "task-child-a"));
  expect(indexed.nextGroupSortOrder(snapshot)).toBe(maxOrder(snapshot.groups.map((value) => value.sortOrder)));
  expect(indexed.nextProjectSortOrder(snapshot, groupId)).toBe(maxOrder(projects.map((value) => value.sortOrder)));
  expect(indexed.nextSectionSortOrder(snapshot, projectId)).toBe(maxOrder(activeSections.map((value) => value.sortOrder)));
  expect(indexed.nextStatusSortOrder(snapshot, projectId)).toBe(maxOrder(statuses.map((value) => value.sortOrder)));
  expect(indexed.nextPrioritySortOrder(snapshot, projectId)).toBe(maxOrder(priorities.map((value) => value.sortOrder)));
  expect(indexed.nextTaskSectionSortOrder(snapshot, projectId, "section-a")).toBe(maxOrder(sectionTasks.map((value) => value.sectionSortOrder)));
  expect(indexed.nextSubtaskSortOrder(snapshot, taskId)).toBe(maxOrder(allSubtasks.map((value) => value.sectionSortOrder)));
  expect(indexed.nextChecklistSortOrder(snapshot, taskId)).toBe(maxOrder(snapshot.checklistItems.filter((value) => value.taskId === taskId).map((value) => value.sortOrder)));
  expect(indexed.nextTagSortOrder(snapshot, projectId)).toBe(maxOrder(projectTags.map((value) => value.sortOrder)));
  expect(indexed.nextCustomFieldSortOrder(snapshot, projectId)).toBe(maxOrder(fields.map((value) => value.sortOrder)));
  expect(indexed.nextCustomFieldOptionSortOrder(snapshot, fieldId)).toBe(maxOrder(options.map((value) => value.sortOrder)));
  expect(indexed.nextCustomEmojiSortOrder(snapshot)).toBe(maxOrder(snapshot.customEmojis.map((value) => value.sortOrder)));
  expect(indexed.nextTaskStatusSortOrder(snapshot, projectId, "status-todo")).toBe(maxOrder(statusTasks.map((value) => value.statusSortOrder)));
}

describe("ProjectLoadedDataIndex", () => {
  for (const [name, fixture] of [
    ["empty", emptySnapshot],
    ["archived", archivedFixture],
    ["nested", nestedFixture],
    ["paginated", paginatedFixture],
    ["dense", denseFixture],
  ] as const) {
    it(`matches reference selector behavior for the ${name} fixture`, () => {
      compareEverySelector(fixture());
    });
  }

  it("returns defensive arrays that cannot corrupt cached ordering", () => {
    const snapshot = nestedFixture();
    const sections = indexed.sectionsForProject(snapshot, "project-a");
    sections.reverse();
    sections.pop();

    expect(ids(indexed.sectionsForProject(snapshot, "project-a"))).toEqual(["section-a", "section-b"]);
  });

  it("indexes identities for every loaded core and optional collection", () => {
    const snapshot = nestedFixture();
    const index = buildProjectLoadedDataIndex(snapshot);

    expect(index.groupById.get("group-a")).toBe(snapshot.groups[1]);
    expect(index.projectById.get("project-a")).toBe(snapshot.projects[1]);
    expect(index.sectionById.get("section-a")?.projectId).toBe("project-a");
    expect(index.statusById.get("status-todo")?.projectId).toBe("project-a");
    expect(index.priorityById.get("priority-a")?.projectId).toBe("project-a");
    expect(index.taskById.get("task-parent")?.projectId).toBe("project-a");
    expect(index.checklistItemById.get("item-a")?.taskId).toBe("task-parent");
    expect(index.tagById.get("tag-a")?.projectId).toBe("project-a");
    expect(index.taskTagLinkByKey.size).toBe(snapshot.taskTagLinks.length);
    expect(index.customFieldById.get("field-a")?.projectId).toBe("project-a");
    expect(index.customFieldOptionById.get("option-a")?.fieldId).toBe("field-a");
    expect(index.customFieldValueByTaskAndField.size).toBe(snapshot.customFieldValues.length);
    expect(index.customFieldOptionValueByKey.size).toBe(snapshot.customFieldOptionValues.length);
    expect(index.dependencyById.get("dependency-a")?.blockedTaskId).toBe("task-parent");
    expect(index.eventLinkByKey.size).toBe(snapshot.eventLinks.length);
    expect(index.taskChangeEventById.get("change-new")?.taskId).toBe("task-parent");
    expect(index.viewPreferenceByKey.size).toBe(snapshot.viewPreferences.length);
    expect(index.customEmojiById.get("emoji-a")?.name).toBe("Focus");
  });

  it("builds once per snapshot identity, including one rebuild per committed patch", () => {
    let snapshot = nestedFixture();
    let builds = 0;
    const reader = createProjectLoadedDataIndexReader(() => { builds += 1; });
    const selectors = createProjectStoreSelectors(() => snapshot, reader);

    selectors.tasksForProject("project-a");
    selectors.taskById("task-parent");
    selectors.tagsForTask("task-parent");
    selectors.customFieldsForProject("project-a");
    expect(builds).toBe(1);

    snapshot = applyProjectMutation(snapshot, {
      changed: { ...emptySnapshot(), tasks: [{ ...snapshot.tasks[2]!, title: "Patched" }] },
      removals: [],
      calendarEventProjectAssignments: [],
    });
    selectors.taskById("task-parent");
    selectors.tasksForProject("project-a");
    expect(builds).toBe(2);
  });
});
