import {
  selectDateRangeEnd,
  selectDateRangeStart,
} from "$lib/calendar/date-range-selection";
import type { ProjectTask } from "$lib/projects/types";

export type ProjectListDateMenu = "start" | "due";
export type ProjectListTaskDatePatch = Partial<Pick<ProjectTask, "startDate" | "dueDate">>;

export interface ProjectListTaskDateEditInput {
  task: Pick<ProjectTask, "id" | "archivedAt" | "startDate" | "dueDate">;
  selectedDate: string | undefined;
  rangeDateColumnsVisible: boolean;
}

export interface ProjectListTaskDateEditPlan {
  patch?: ProjectListTaskDatePatch;
  closeMenu: ProjectListDateMenu;
  nextOpenMenu: ProjectListDateMenu | null;
}

export function projectListStartDateEditPlan(input: ProjectListTaskDateEditInput): ProjectListTaskDateEditPlan {
  const basePlan = {
    closeMenu: "start",
    nextOpenMenu: null,
  } satisfies ProjectListTaskDateEditPlan;

  if (input.task.archivedAt) return basePlan;
  if (!input.selectedDate) {
    return input.task.startDate === undefined
      ? basePlan
      : { ...basePlan, patch: { startDate: undefined } };
  }

  const nextRange = selectDateRangeStart({
    selectedDate: input.selectedDate,
    startDate: input.task.startDate,
    endDate: input.task.dueDate,
  });

  if (
    input.task.startDate === nextRange.startDate
    && input.task.dueDate === nextRange.endDate
  ) {
    return basePlan;
  }

  return {
    closeMenu: "start",
    nextOpenMenu: input.rangeDateColumnsVisible && !input.task.dueDate && !nextRange.endDate ? "due" : null,
    patch: {
      startDate: nextRange.startDate,
      dueDate: nextRange.endDate,
    },
  };
}

export function projectListDueDateEditPlan(input: ProjectListTaskDateEditInput): ProjectListTaskDateEditPlan {
  const basePlan = {
    closeMenu: "due",
    nextOpenMenu: null,
  } satisfies ProjectListTaskDateEditPlan;

  if (input.task.archivedAt) return basePlan;
  if (!input.selectedDate) {
    return input.task.dueDate === undefined
      ? basePlan
      : { ...basePlan, patch: { dueDate: undefined } };
  }

  const nextRange = selectDateRangeEnd({
    selectedDate: input.selectedDate,
    startDate: input.task.startDate,
    endDate: input.task.dueDate,
  });

  if (
    input.task.startDate === nextRange.startDate
    && input.task.dueDate === nextRange.endDate
  ) {
    return basePlan;
  }

  return {
    closeMenu: "due",
    nextOpenMenu: input.rangeDateColumnsVisible && !input.task.startDate && !nextRange.startDate ? "start" : null,
    patch: {
      startDate: nextRange.startDate,
      dueDate: nextRange.endDate,
    },
  };
}
