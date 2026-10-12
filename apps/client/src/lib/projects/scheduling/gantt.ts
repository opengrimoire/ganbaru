import { Temporal } from "@js-temporal/polyfill";
import type { ProjectStatus, ProjectTask, ProjectTaskDependency } from "$lib/projects/types";

export interface ProjectGanttInput {
  tasks: readonly ProjectTask[];
  statuses: readonly ProjectStatus[];
  dependencies: readonly ProjectTaskDependency[];
  dependencyBlockedTaskIds: ReadonlySet<string>;
  today: string;
}

export interface ProjectGanttTick {
  date: string;
  leftPercent: number;
}

export interface ProjectGanttRow {
  task: ProjectTask;
  startDate: string;
  endDate: string;
  leftPercent: number;
  widthPercent: number;
  markerPercent: number;
  milestone: boolean;
  overdue: boolean;
  blocked: boolean;
  done: boolean;
  blockedByCount: number;
  blocksCount: number;
}

export interface ProjectGanttDependencyEdge {
  id: string;
  blockingTaskId: string;
  blockedTaskId: string;
  blockingTitle: string;
  blockedTitle: string;
  fromPercent: number;
  toPercent: number;
  lineStartPercent: number;
  lineWidthPercent: number;
  lagDays: number;
  violated: boolean;
}

export interface ProjectGanttTimeline {
  startDate?: string;
  endDate?: string;
  totalDays: number;
  ticks: ProjectGanttTick[];
  todayPercent?: number;
  rows: ProjectGanttRow[];
  dependencyEdges: ProjectGanttDependencyEdge[];
}

export type ProjectGanttDateInteraction = "move" | "resize-start" | "resize-end";

export interface ProjectGanttDatePatch {
  startDate?: string;
  dueDate?: string;
  targetEndDate?: string;
}

interface TaskDateRange {
  startDate: string;
  endDate: string;
  milestone: boolean;
}

const MAX_TICKS = 8;

function plainDate(date: string): Temporal.PlainDate {
  return Temporal.PlainDate.from(date);
}

function addDays(date: string, days: number): string {
  return plainDate(date).add({ days }).toString();
}

function daysBetween(startDate: string, endDate: string): number {
  return plainDate(startDate).until(plainDate(endDate)).total({ unit: "days" });
}

function minDate(a: string, b: string): string {
  return a <= b ? a : b;
}

function maxDate(a: string, b: string): string {
  return a >= b ? a : b;
}

function clampPercent(value: number): number {
  return Math.min(100, Math.max(0, value));
}

function statusById(statuses: readonly ProjectStatus[]): ReadonlyMap<string, ProjectStatus> {
  return new Map(statuses.map((status) => [status.id, status]));
}

function taskDateRange(task: ProjectTask): TaskDateRange | undefined {
  const milestone = task.milestone || task.taskType === "milestone";
  if (milestone) {
    const date = task.targetEndDate ?? task.dueDate ?? task.startDate;
    return date ? { startDate: date, endDate: date, milestone: true } : undefined;
  }
  const firstDate = task.startDate ?? task.targetEndDate ?? task.dueDate;
  const secondDate = task.targetEndDate ?? task.dueDate ?? task.startDate;
  if (!firstDate || !secondDate) return undefined;
  return {
    startDate: minDate(firstDate, secondDate),
    endDate: maxDate(firstDate, secondDate),
    milestone: false,
  };
}

function shiftOptionalDate(date: string | undefined, days: number): string | undefined {
  return date ? addDays(date, days) : undefined;
}

function patchChanged(task: ProjectTask, patch: ProjectGanttDatePatch): boolean {
  return task.startDate !== patch.startDate
    || task.dueDate !== patch.dueDate
    || task.targetEndDate !== patch.targetEndDate;
}

function rangeStartPatch(task: ProjectTask, startDate: string): ProjectGanttDatePatch {
  return {
    startDate,
    dueDate: task.dueDate,
    targetEndDate: task.targetEndDate,
  };
}

function rangeEndPatch(task: ProjectTask, endDate: string): ProjectGanttDatePatch {
  return {
    startDate: task.startDate,
    dueDate: task.dueDate && !task.targetEndDate ? endDate : task.dueDate,
    targetEndDate: task.targetEndDate || !task.dueDate ? endDate : task.targetEndDate,
  };
}

export function buildProjectGanttDatePatch(
  task: ProjectTask,
  interaction: ProjectGanttDateInteraction,
  dayDelta: number,
): ProjectGanttDatePatch | undefined {
  if (dayDelta === 0) return undefined;
  const range = taskDateRange(task);
  if (!range) return undefined;
  let patch: ProjectGanttDatePatch | undefined;
  if (interaction === "move" || range.milestone) {
    patch = {
      startDate: shiftOptionalDate(task.startDate, dayDelta),
      dueDate: shiftOptionalDate(task.dueDate, dayDelta),
      targetEndDate: shiftOptionalDate(task.targetEndDate, dayDelta),
    };
  } else if (interaction === "resize-start") {
    const nextStart = minDate(addDays(range.startDate, dayDelta), range.endDate);
    patch = rangeStartPatch(task, nextStart);
  } else {
    const nextEnd = maxDate(addDays(range.endDate, dayDelta), range.startDate);
    patch = rangeEndPatch(task, nextEnd);
  }
  return patchChanged(task, patch) ? patch : undefined;
}

function buildTicks(startDate: string, totalDays: number): ProjectGanttTick[] {
  const interval = Math.max(1, Math.ceil(totalDays / MAX_TICKS));
  const ticks: ProjectGanttTick[] = [];
  for (let offset = 0; offset < totalDays; offset += interval) {
    ticks.push({
      date: addDays(startDate, offset),
      leftPercent: clampPercent((offset / totalDays) * 100),
    });
  }
  const endOffset = totalDays - 1;
  const endDate = addDays(startDate, endOffset);
  if (ticks.at(-1)?.date !== endDate) {
    ticks.push({
      date: endDate,
      leftPercent: clampPercent((endOffset / totalDays) * 100),
    });
  }
  return ticks;
}

export function buildProjectGanttTimeline(input: ProjectGanttInput): ProjectGanttTimeline {
  const datedTasks = input.tasks
    .map((task) => ({ task, range: taskDateRange(task) }))
    .filter((entry): entry is { task: ProjectTask; range: TaskDateRange } => entry.range !== undefined);

  if (datedTasks.length === 0) {
    return { totalDays: 0, ticks: [], rows: [], dependencyEdges: [] };
  }

  const timelineStart = datedTasks.reduce(
    (current, entry) => minDate(current, entry.range.startDate),
    datedTasks[0].range.startDate,
  );
  const timelineEnd = datedTasks.reduce(
    (current, entry) => maxDate(current, entry.range.endDate),
    datedTasks[0].range.endDate,
  );
  const totalDays = Math.max(1, daysBetween(timelineStart, timelineEnd) + 1);
  const statusesById = statusById(input.statuses);
  const blockedByCountByTaskId = new Map<string, number>();
  const blocksCountByTaskId = new Map<string, number>();
  for (const dependency of input.dependencies) {
    blockedByCountByTaskId.set(
      dependency.blockedTaskId,
      (blockedByCountByTaskId.get(dependency.blockedTaskId) ?? 0) + 1,
    );
    blocksCountByTaskId.set(
      dependency.blockingTaskId,
      (blocksCountByTaskId.get(dependency.blockingTaskId) ?? 0) + 1,
    );
  }

  const rows = datedTasks
    .map(({ task, range }) => {
      const startOffset = daysBetween(timelineStart, range.startDate);
      const endOffset = daysBetween(timelineStart, range.endDate);
      const status = statusesById.get(task.statusId);
      const done = status?.terminal === true;
      const blocked = status?.category === "blocked"
        || Boolean(task.blockerReason?.trim())
        || input.dependencyBlockedTaskIds.has(task.id);
      return {
        task,
        startDate: range.startDate,
        endDate: range.endDate,
        leftPercent: clampPercent((startOffset / totalDays) * 100),
        widthPercent: clampPercent(((endOffset - startOffset + 1) / totalDays) * 100),
        markerPercent: clampPercent(((startOffset + 0.5) / totalDays) * 100),
        milestone: range.milestone,
        overdue: !done && range.endDate < input.today,
        blocked,
        done,
        blockedByCount: blockedByCountByTaskId.get(task.id) ?? 0,
        blocksCount: blocksCountByTaskId.get(task.id) ?? 0,
      } satisfies ProjectGanttRow;
    })
    .sort((a, b) =>
      a.startDate.localeCompare(b.startDate)
      || a.endDate.localeCompare(b.endDate)
      || a.task.sectionSortOrder - b.task.sectionSortOrder
      || a.task.title.localeCompare(b.task.title)
    );
  const rowsByTaskId = new Map(rows.map((row) => [row.task.id, row]));
  const dependencyEdges = input.dependencies
    .map((dependency) => {
      const blockingRow = rowsByTaskId.get(dependency.blockingTaskId);
      const blockedRow = rowsByTaskId.get(dependency.blockedTaskId);
      if (!blockingRow || !blockedRow) return undefined;
      const fromOffset = daysBetween(timelineStart, blockingRow.endDate) + 1;
      const toOffset = daysBetween(timelineStart, blockedRow.startDate);
      const fromPercent = clampPercent((fromOffset / totalDays) * 100);
      const toPercent = clampPercent((toOffset / totalDays) * 100);
      const lineStartPercent = Math.min(fromPercent, toPercent);
      const lineWidthPercent = Math.abs(toPercent - fromPercent);
      const lagDays = daysBetween(blockingRow.endDate, blockedRow.startDate);
      return {
        id: dependency.id,
        blockingTaskId: dependency.blockingTaskId,
        blockedTaskId: dependency.blockedTaskId,
        blockingTitle: blockingRow.task.title,
        blockedTitle: blockedRow.task.title,
        fromPercent,
        toPercent,
        lineStartPercent,
        lineWidthPercent,
        lagDays,
        violated: lagDays <= 0,
      } satisfies ProjectGanttDependencyEdge;
    })
    .filter((edge): edge is ProjectGanttDependencyEdge => edge !== undefined);

  const todayOffset = input.today >= timelineStart && input.today <= timelineEnd
    ? daysBetween(timelineStart, input.today)
    : undefined;

  return {
    startDate: timelineStart,
    endDate: timelineEnd,
    totalDays,
    ticks: buildTicks(timelineStart, totalDays),
    todayPercent: todayOffset === undefined
      ? undefined
      : clampPercent(((todayOffset + 0.5) / totalDays) * 100),
    rows,
    dependencyEdges,
  };
}
