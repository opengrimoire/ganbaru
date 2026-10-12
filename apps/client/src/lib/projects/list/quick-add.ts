import type { ProjectTaskListGroup } from "$lib/projects/tasks/view";
import type {
  ProjectPriorityConfig,
  ProjectStatus,
  ProjectTask,
  ProjectTaskGroupMode,
} from "$lib/projects/types";

export type ProjectListTaskCreateTarget = `section:${string}` | `group:${string}` | `subtask:${string}`;
export type ProjectListQuickAddPatch = Partial<Pick<ProjectTask, "priority" | "dueDate">>;

export interface ProjectListGroupQuickAddInput {
  groupBy: ProjectTaskGroupMode;
  group: Pick<ProjectTaskListGroup, "value">;
  statuses: readonly ProjectStatus[];
  priorities: readonly ProjectPriorityConfig[];
  today?: Temporal.PlainDate;
}

export interface ProjectListGroupQuickAddPlan {
  enabled: boolean;
  statusId?: string;
  patch: ProjectListQuickAddPatch;
}

export function projectListSectionTaskCreateTarget(sectionId: string): ProjectListTaskCreateTarget {
  return `section:${sectionId}`;
}

export function projectListSubtaskCreateTarget(parentTaskId: string): ProjectListTaskCreateTarget {
  return `subtask:${parentTaskId}`;
}

export function projectListGroupTaskDraftKey(
  groupBy: ProjectTaskGroupMode,
  group: Pick<ProjectTaskListGroup, "value">,
): string {
  return `${groupBy}:${group.value}`;
}

export function projectListGroupTaskCreateTarget(groupKey: string): ProjectListTaskCreateTarget {
  return `group:${groupKey}`;
}

export function projectListDueDateForGroup(
  value: string,
  today: Temporal.PlainDate = Temporal.Now.plainDateISO(),
): string | undefined {
  if (value === "today") return today.toString();
  if (value === "week") return today.add({ days: 7 }).toString();
  if (value === "later") return today.add({ days: 8 }).toString();
  if (value === "overdue" || value === "earlier") return today.subtract({ days: 1 }).toString();
  return undefined;
}

function projectListTerminalTaskStatus(statuses: readonly ProjectStatus[]): ProjectStatus | undefined {
  return statuses.find((status) => status.terminal);
}

export function projectListGroupQuickAddPlan(
  input: ProjectListGroupQuickAddInput,
): ProjectListGroupQuickAddPlan {
  if (input.groupBy === "status") {
    const status = input.statuses.find((entry) => entry.id === input.group.value);
    return {
      enabled: status !== undefined,
      statusId: status?.id,
      patch: {},
    };
  }

  if (input.groupBy === "priority") {
    const priority = input.priorities.find((entry) => entry.id === input.group.value);
    return {
      enabled: priority !== undefined,
      patch: priority ? { priority: priority.id } : {},
    };
  }

  if (input.groupBy === "due") {
    const terminalStatus = projectListTerminalTaskStatus(input.statuses);
    return {
      enabled: input.group.value !== "earlier" || terminalStatus !== undefined,
      statusId: input.group.value === "earlier" ? terminalStatus?.id : undefined,
      patch: {
        dueDate: projectListDueDateForGroup(input.group.value, input.today),
      },
    };
  }

  if (input.groupBy === "scheduled") {
    return {
      enabled: input.group.value === "unscheduled",
      patch: {},
    };
  }

  return {
    enabled: false,
    patch: {},
  };
}
