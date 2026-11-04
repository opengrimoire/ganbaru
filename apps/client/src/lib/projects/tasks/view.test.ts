import { describe, expect, it } from "vitest";
import { customFieldReference } from "./list-columns";
import type {
  ProjectCustomField,
  ProjectCustomFieldOption,
  ProjectCustomFieldValue,
  ProjectPriorityConfig,
  ProjectStatus,
  ProjectTask,
} from "$lib/projects/types";
import {
  buildProjectTaskListGroups,
  buildProjectTaskView,
  manualStatusCompare,
  projectTaskCustomFieldKey,
} from "./view";

const statuses: ProjectStatus[] = [
  status("backlog", "Backlog", "not_started", 1000, false),
  status("todo", "To do", "not_started", 2000, false),
  status("doing", "In progress", "active", 3000, false),
  status("review", "In review", "active", 4000, false),
  status("blocked", "Blocked", "blocked", 5000, false),
  status("done", "Done", "done", 6000, true),
];

const priorities: ProjectPriorityConfig[] = [
  priority("urgent", "Urgent", 2, 1000),
  priority("high", "High", 7, 2000),
  priority("normal", "Normal", 19, 3000),
  priority("low", "Low", 30, 4000),
];

function status(
  id: string,
  name: string,
  category: ProjectStatus["category"],
  rank: number,
  terminal: boolean,
): ProjectStatus {
  return {
    id,
    projectId: "project",
    name,
    category,
    color: 0,
    orderKey: String(rank),
    terminal,
    createdAt: "2026-06-01T00:00:00Z",
    updatedAt: "2026-06-01T00:00:00Z",
  };
}

function priority(
  id: string,
  name: string,
  color: ProjectPriorityConfig["color"],
  rank: number,
): ProjectPriorityConfig {
  return {
    id,
    projectId: "project",
    name,
    color,
    orderKey: String(rank),
    createdAt: "2026-06-01T00:00:00Z",
    updatedAt: "2026-06-01T00:00:00Z",
  };
}

function task(input: Partial<ProjectTask> & Pick<ProjectTask, "id" | "title">): ProjectTask {
  return {
    id: input.id,
    projectId: "project",
    sectionId: input.sectionId ?? "section-a",
    statusId: input.statusId ?? "backlog",
    parentTaskId: input.parentTaskId,
    title: input.title,
    description: input.description ?? "",
    priority: input.priority ?? "normal",
    taskType: input.taskType ?? "task",
    sectionOrderKey: input.sectionOrderKey ?? "1000",
    statusOrderKey: input.statusOrderKey ?? "1000",
    estimateMinutes: input.estimateMinutes,
    dueDate: input.dueDate,
    dueTime: input.dueTime,
    startDate: input.startDate,
    startTime: input.startTime,
    targetEndDate: input.targetEndDate,
    completedAt: input.completedAt,
    archivedAt: input.archivedAt,
    blockerReason: input.blockerReason,
    milestone: input.milestone ?? false,
    createdAt: input.createdAt ?? "2026-06-01T00:00:00Z",
    updatedAt: input.updatedAt ?? "2026-06-01T00:00:00Z",
  };
}

function customField(input: Pick<ProjectCustomField, "id" | "name" | "fieldType">): ProjectCustomField {
  return {
    ...input,
    projectId: "project",
    orderKey: "1000",
    icon: null,
    createdAt: "2026-06-01T00:00:00Z",
    updatedAt: "2026-06-01T00:00:00Z",
  };
}

function customFieldOption(
  input: Pick<ProjectCustomFieldOption, "id" | "fieldId" | "name" | "orderKey">,
): ProjectCustomFieldOption {
  return {
    ...input,
    createdAt: "2026-06-01T00:00:00Z",
    updatedAt: "2026-06-01T00:00:00Z",
  };
}

function customFieldValue(
  input: Omit<ProjectCustomFieldValue, "updatedAt">,
): ProjectCustomFieldValue {
  return {
    ...input,
    updatedAt: "2026-06-01T00:00:00Z",
  };
}

function view(overrides: Partial<Parameters<typeof buildProjectTaskView>[0]>) {
  return buildProjectTaskView({
    tasks: [],
    statuses,
    priorities,
    customFields: [],
    customFieldOptions: [],
    customFieldValuesByTaskField: new Map(),
    customFieldOptionIdsByTaskField: new Map(),
    scheduledTaskIds: new Set(),
    nextScheduledStartByTaskId: new Map(),
    taskTagIdsByTaskId: new Map(),
    dependencyBlockedTaskIds: new Set(),
    dependencyBlockingTaskIds: new Set(),
    today: "2026-06-12",
    weekEnd: "2026-06-19",
    search: "",
    statusFilter: "all",
    sectionFilter: "all",
    priorityFilter: "all",
    dueFilter: "all",
    scheduleFilter: "all",
    dependencyFilter: "all",
    tagFilter: "all",
    customFieldFilters: [],
    groupBy: "section",
    sortMode: "manual",
    sortDirection: "asc",
    ...overrides,
  });
}

describe("buildProjectTaskView", () => {
  it("keeps a parent visible when a matching subtask is visible", () => {
    const result = view({
      tasks: [
        task({ id: "parent", title: "Build project manager" }),
        task({ id: "subtask", title: "Write filter tests", parentTaskId: "parent" }),
      ],
      search: "filter",
    });

    expect(result.tasks.map((entry) => entry.id)).toEqual(["parent", "subtask"]);
    expect([...result.matchedTaskIds]).toEqual(["subtask"]);
  });

  it("filters by scheduled state without duplicating task data", () => {
    const result = view({
      tasks: [
        task({ id: "scheduled", title: "Scheduled task" }),
        task({ id: "unscheduled", title: "Unscheduled task" }),
      ],
      scheduledTaskIds: new Set(["scheduled"]),
      scheduleFilter: "scheduled",
    });

    expect(result.tasks.map((entry) => entry.id)).toEqual(["scheduled"]);
  });

  it("sorts by next scheduled block with unscheduled tasks last", () => {
    const result = view({
      tasks: [
        task({ id: "unscheduled", title: "Unscheduled task" }),
        task({ id: "later", title: "Later task" }),
        task({ id: "earlier", title: "Earlier task" }),
      ],
      scheduledTaskIds: new Set(["later", "earlier"]),
      nextScheduledStartByTaskId: new Map([
        ["later", "2026-06-13 09:00"],
        ["earlier", "2026-06-12 15:00"],
      ]),
      sortMode: "scheduled",
    });

    expect(result.tasks.map((entry) => entry.id)).toEqual(["earlier", "later", "unscheduled"]);
  });

  it("sorts due dates by due time before manual fallback", () => {
    const result = view({
      tasks: [
        task({ id: "late", title: "Late", dueDate: "2026-06-13", dueTime: "16:00", sectionOrderKey: "1000" }),
        task({ id: "early", title: "Early", dueDate: "2026-06-13", dueTime: "09:00", sectionOrderKey: "2000" }),
        task({ id: "no-time-a", title: "No time A", dueDate: "2026-06-14", sectionOrderKey: "4000" }),
        task({ id: "no-time-b", title: "No time B", dueDate: "2026-06-14", sectionOrderKey: "3000" }),
      ],
      sortMode: "due",
    });

    expect(result.tasks.map((entry) => entry.id)).toEqual(["early", "late", "no-time-b", "no-time-a"]);
  });

  it("filters overdue tasks without treating completed tasks as overdue", () => {
    const result = view({
      tasks: [
        task({ id: "late", title: "Late task", dueDate: "2026-06-10" }),
        task({ id: "done-late", title: "Done late task", statusId: "done", dueDate: "2026-06-10" }),
        task({ id: "future", title: "Future task", dueDate: "2026-06-15" }),
      ],
      dueFilter: "overdue",
    });

    expect(result.tasks.map((entry) => entry.id)).toEqual(["late"]);
  });

  it("filters tasks by due date range", () => {
    const tasks = [
      task({ id: "before", title: "Before", dueDate: "2026-06-10" }),
      task({ id: "inside", title: "Inside", dueDate: "2026-06-14" }),
      task({ id: "after", title: "After", dueDate: "2026-06-20" }),
      task({ id: "none", title: "No due date" }),
    ];

    expect(view({
      tasks,
      dueFilter: "range",
      dueRangeStart: "2026-06-12",
      dueRangeEnd: "2026-06-18",
    }).tasks.map((entry) => entry.id)).toEqual(["inside"]);

    expect(view({
      tasks,
      dueFilter: "range",
      dueRangeStart: "2026-06-12",
    }).tasks.map((entry) => entry.id)).toEqual(["after", "inside"]);
  });

  it("treats dependency-blocked tasks as blocked", () => {
    const result = view({
      tasks: [
        task({ id: "blocked-by-dependency", title: "Blocked by dependency" }),
        task({ id: "open", title: "Open task" }),
      ],
      dependencyBlockedTaskIds: new Set(["blocked-by-dependency"]),
      statusFilter: "blocked",
    });

    expect(result.tasks.map((entry) => entry.id)).toEqual(["blocked-by-dependency"]);
  });

  it("filters tasks by dependency relationship", () => {
    const tasks = [
      task({ id: "blocks", title: "Blocks another task" }),
      task({ id: "blocked", title: "Blocked by another task" }),
      task({ id: "both", title: "Blocks and is blocked" }),
      task({ id: "independent", title: "Independent task" }),
    ];

    expect(view({
      tasks,
      dependencyBlockingTaskIds: new Set(["blocks", "both"]),
      dependencyBlockedTaskIds: new Set(["blocked", "both"]),
      dependencyFilter: "linked",
    }).tasks.map((entry) => entry.id)).toEqual(["blocked", "blocks", "both"]);

    expect(view({
      tasks,
      dependencyBlockingTaskIds: new Set(["blocks", "both"]),
      dependencyBlockedTaskIds: new Set(["blocked", "both"]),
      dependencyFilter: "blocked_by",
    }).tasks.map((entry) => entry.id)).toEqual(["blocked", "both"]);

    expect(view({
      tasks,
      dependencyBlockingTaskIds: new Set(["blocks", "both"]),
      dependencyBlockedTaskIds: new Set(["blocked", "both"]),
      dependencyFilter: "none",
    }).tasks.map((entry) => entry.id)).toEqual(["independent"]);
  });

  it("filters tasks by project tags", () => {
    const tasks = [
      task({ id: "backend", title: "Backend task" }),
      task({ id: "frontend", title: "Frontend task" }),
      task({ id: "untagged", title: "Untagged task" }),
    ];
    const taskTagIdsByTaskId = new Map([
      ["backend", new Set(["tag-backend"])],
      ["frontend", new Set(["tag-frontend"])],
    ]);

    expect(view({
      tasks,
      taskTagIdsByTaskId,
      tagFilter: "tag-backend",
    }).tasks.map((entry) => entry.id)).toEqual(["backend"]);

    expect(view({
      tasks,
      taskTagIdsByTaskId,
      tagFilter: "none",
    }).tasks.map((entry) => entry.id)).toEqual(["untagged"]);
  });

  it("filters tasks by typed custom fields", () => {
    const riskField = customField({ id: "field-risk", name: "Risk", fieldType: "number" });
    const reviewedField = customField({ id: "field-reviewed", name: "Reviewed", fieldType: "checkbox" });
    const stageField = customField({ id: "field-stage", name: "Stage", fieldType: "select" });
    const buildOption = customFieldOption({
      id: "option-build",
      fieldId: stageField.id,
      name: "Build",
      orderKey: "1000",
    });
    const tasks = [
      task({ id: "scored", title: "Scored" }),
      task({ id: "reviewed", title: "Reviewed" }),
      task({ id: "staged", title: "Staged" }),
      task({ id: "empty", title: "Empty" }),
    ];
    const customFieldValuesByTaskField = new Map([
      [
        projectTaskCustomFieldKey("scored", riskField.id),
        customFieldValue({ taskId: "scored", fieldId: riskField.id, numberValue: "4" }),
      ],
      [
        projectTaskCustomFieldKey("reviewed", reviewedField.id),
        customFieldValue({ taskId: "reviewed", fieldId: reviewedField.id, checkboxValue: true }),
      ],
    ]);
    const customFieldOptionIdsByTaskField = new Map([
      [projectTaskCustomFieldKey("staged", stageField.id), new Set([buildOption.id])],
    ]);
    const input = {
      tasks,
      customFields: [riskField, reviewedField, stageField],
      customFieldOptions: [buildOption],
      customFieldValuesByTaskField,
      customFieldOptionIdsByTaskField,
    };

    expect(view({
      ...input,
      customFieldFilters: [{ fieldId: riskField.id, mode: "filled" }],
    }).tasks.map((entry) => entry.id)).toEqual(["scored"]);

    expect(view({
      ...input,
      customFieldFilters: [{ fieldId: reviewedField.id, mode: "checkbox", checked: true }],
    }).tasks.map((entry) => entry.id)).toEqual(["reviewed"]);

    expect(view({
      ...input,
      customFieldFilters: [{ fieldId: stageField.id, mode: "option", optionId: buildOption.id }],
    }).tasks.map((entry) => entry.id)).toEqual(["staged"]);

    expect(view({
      ...input,
      customFieldFilters: [{ fieldId: riskField.id, mode: "empty" }],
    }).tasks.map((entry) => entry.id)).toEqual(["empty", "reviewed", "staged"]);
  });

  it("sorts by custom field values and keeps empty values last", () => {
    const scoreField = customField({ id: "field-score", name: "Score", fieldType: "number" });
    const tasks = [
      task({ id: "empty", title: "Empty" }),
      task({ id: "low", title: "Low" }),
      task({ id: "high", title: "High" }),
    ];
    const customFieldValuesByTaskField = new Map([
      [
        projectTaskCustomFieldKey("low", scoreField.id),
        customFieldValue({ taskId: "low", fieldId: scoreField.id, numberValue: "2" }),
      ],
      [
        projectTaskCustomFieldKey("high", scoreField.id),
        customFieldValue({ taskId: "high", fieldId: scoreField.id, numberValue: "10.5" }),
      ],
    ]);

    expect(view({
      tasks,
      customFields: [scoreField],
      customFieldValuesByTaskField,
      sortMode: customFieldReference(scoreField.id),
      sortDirection: "desc",
    }).tasks.map((entry) => entry.id)).toEqual(["high", "low", "empty"]);
  });

  it("sorts priority with explicit direction and keeps manual fallback stable", () => {
    const result = view({
      tasks: [
        task({ id: "normal", title: "Normal", priority: "normal", sectionOrderKey: "1000" }),
        task({ id: "urgent-a", title: "Urgent A", priority: "urgent", sectionOrderKey: "3000" }),
        task({ id: "urgent-b", title: "Urgent B", priority: "urgent", sectionOrderKey: "2000" }),
        task({ id: "low", title: "Low", priority: "low", sectionOrderKey: "4000" }),
      ],
      sortMode: "priority",
      sortDirection: "desc",
    });

    expect(result.tasks.map((entry) => entry.id)).toEqual(["urgent-b", "urgent-a", "normal", "low"]);
  });
});

describe("buildProjectTaskListGroups", () => {
  it("groups top-level tasks by status in workflow order", () => {
    const groups = buildProjectTaskListGroups({
      tasks: [
        task({ id: "child", title: "Child", statusId: "done", parentTaskId: "parent" }),
        task({ id: "parent", title: "Parent", statusId: "doing" }),
        task({ id: "later", title: "Later", statusId: "backlog" }),
      ],
      statuses,
      priorities,
      scheduledTaskIds: new Set(),
      today: "2026-06-12",
      weekEnd: "2026-06-19",
      groupBy: "status",
    });

    expect(groups.map((group) => [group.value, group.tasks.map((entry) => entry.id)])).toEqual([
      ["backlog", ["later"]],
      ["todo", []],
      ["doing", ["parent"]],
      ["review", []],
      ["blocked", []],
      ["done", []],
    ]);
  });

  it("groups due dates into useful planning buckets", () => {
    const groups = buildProjectTaskListGroups({
      tasks: [
        task({ id: "none", title: "No due date" }),
        task({ id: "later", title: "Later", dueDate: "2026-06-30" }),
        task({ id: "today", title: "Today", dueDate: "2026-06-12" }),
        task({ id: "overdue", title: "Overdue", dueDate: "2026-06-10" }),
        task({ id: "earlier", title: "Earlier done", statusId: "done", dueDate: "2026-06-10" }),
        task({ id: "week", title: "Week", dueDate: "2026-06-15" }),
      ],
      statuses,
      priorities,
      scheduledTaskIds: new Set(),
      today: "2026-06-12",
      weekEnd: "2026-06-19",
      groupBy: "due",
    });

    expect(groups.map((group) => [group.value, group.tasks.map((entry) => entry.id)])).toEqual([
      ["overdue", ["overdue"]],
      ["today", ["today"]],
      ["week", ["week"]],
      ["later", ["later"]],
      ["earlier", ["earlier"]],
      ["none", ["none"]],
    ]);
  });

  it("keeps empty priority groups visible", () => {
    const groups = buildProjectTaskListGroups({
      tasks: [
        task({ id: "normal", title: "Normal priority", priority: "normal" }),
      ],
      statuses,
      priorities,
      scheduledTaskIds: new Set(),
      today: "2026-06-12",
      weekEnd: "2026-06-19",
      groupBy: "priority",
    });

    expect(groups.map((group) => [group.value, group.tasks.map((entry) => entry.id)])).toEqual([
      ["urgent", []],
      ["high", []],
      ["normal", ["normal"]],
      ["low", []],
    ]);
  });

  it("groups scheduled tasks before unscheduled tasks", () => {
    const groups = buildProjectTaskListGroups({
      tasks: [
        task({ id: "scheduled", title: "Scheduled" }),
      ],
      statuses,
      priorities,
      scheduledTaskIds: new Set(["scheduled"]),
      today: "2026-06-12",
      weekEnd: "2026-06-19",
      groupBy: "scheduled",
    });

    expect(groups.map((group) => [group.value, group.tasks.map((entry) => entry.id)])).toEqual([
      ["scheduled", ["scheduled"]],
      ["unscheduled", []],
    ]);
  });
});

describe("manualStatusCompare", () => {
  it("uses status order before section order for kanban columns", () => {
    const first = task({ id: "first", title: "First", statusOrderKey: "1000", sectionOrderKey: "2000" });
    const second = task({ id: "second", title: "Second", statusOrderKey: "2000", sectionOrderKey: "1000" });

    expect([second, first].sort(manualStatusCompare).map((entry) => entry.id)).toEqual(["first", "second"]);
  });
});
