/**
 * Timing for the welcome scene on tasks. In the Novel project's List view, a task is typed into the add row at the
 * bottom of the list and added, and its status and priority are changed from their cells in the table.
 */

import { easeInOutCubic, filmCaretOn, filmSpan, filmTap, filmTyped } from "./welcome-film";
import {
  PROJECTS_HOVER_LEAD_MS,
  type ProjectsScenePriority,
  type ProjectsSceneStatus,
  type ProjectsSceneTaskEntry,
} from "./welcome-projects-scene";

/** The cell menus the scene opens in the list. */
export type TasksSceneCell = "status" | "priority";

/** The task the scene adds, as a new task starts: the default status and priority, with no dates. */
export const TASKS_NEW_TASK: ProjectsSceneTaskEntry = { key: "draftChapter", status: "todo", priority: "normal", start: null, due: null };

/** The status and priority the added task is given from its cells. */
export const TASKS_NEW_STATUS: ProjectsSceneStatus = "inProgress";
export const TASKS_NEW_PRIORITY: ProjectsScenePriority = "high";

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const TASKS_BEATS = {
  /** A tap on the add row opens its field, a title is typed, and Enter adds the task, keeping the field open. */
  addTap: 800,
  taskTypeStart: 1000,
  taskTypeEnd: 1900,
  taskSubmit: 2300,
  /** The list scrolls the new task's status into view, a tap opens its menu and closes the add field, and In progress is picked. */
  statusScrollStart: 2600,
  statusScrollEnd: 3100,
  statusTap: 3500,
  statusPick: 4800,
  /** The list scrolls on to its priority, a tap opens its menu, and High is picked. */
  priorityScrollStart: 5100,
  priorityScrollEnd: 5600,
  priorityTap: 6000,
  priorityPick: 7300,
  end: 8600,
} as const;

const B = TASKS_BEATS;

/** How long the last frame rests before the scene starts over. */
export const TASKS_END_HOLD_MS = 900;

/** The frame shown as a still with reduced motion: the added task with its new status and priority. */
export const TASKS_STILL = B.end;

/** The tap that opens each cell menu and the tap that picks its option. */
const CELL_TAPS: Readonly<Record<TasksSceneCell, { tap: number; pick: number }>> = {
  status: { tap: B.statusTap, pick: B.statusPick },
  priority: { tap: B.priorityTap, pick: B.priorityPick },
};

/** Whether the add row's field is open: from its tap until a tap elsewhere in the list closes it. */
export function tasksAddActive(elapsed: number): boolean {
  return elapsed >= B.addTap && elapsed < B.statusTap;
}

/** As much of `title` as the add row's field holds: typed a character at a time, and cleared once the task is added. */
export function tasksDraft(elapsed: number, title: string): string {
  return elapsed >= B.taskSubmit ? "" : filmTyped(elapsed, title, B.taskTypeStart, B.taskTypeEnd);
}

/** Whether the add row's caret shows: steady while typing, then blinking while its field stays open. */
export function tasksCaretOn(elapsed: number): boolean {
  return tasksAddActive(elapsed) && filmCaretOn(elapsed, B.taskTypeEnd);
}

/** Whether the typed task has been added to the list. */
export function tasksAdded(elapsed: number): boolean {
  return elapsed >= B.taskSubmit;
}

/** The added task's status. */
export function tasksNewStatus(elapsed: number): ProjectsSceneStatus {
  return elapsed >= B.statusPick ? TASKS_NEW_STATUS : TASKS_NEW_TASK.status;
}

/** The added task's priority. */
export function tasksNewPriority(elapsed: number): ProjectsScenePriority {
  return elapsed >= B.priorityPick ? TASKS_NEW_PRIORITY : TASKS_NEW_TASK.priority;
}

/**
 * How far the list has scrolled sideways, each from 0 to 1: `status` until the added task's status cell is in view,
 * then `priority` on until its priority cell is.
 */
export function tasksListScroll(elapsed: number): { status: number; priority: number } {
  return {
    status: easeInOutCubic(filmSpan(elapsed, B.statusScrollStart, B.statusScrollEnd)),
    priority: easeInOutCubic(filmSpan(elapsed, B.priorityScrollStart, B.priorityScrollEnd)),
  };
}

/** The added task's cell menu that is open, or null while both are closed. */
export function tasksCellMenu(elapsed: number): TasksSceneCell | null {
  for (const cell of ["status", "priority"] as const) {
    if (elapsed >= CELL_TAPS[cell].tap && elapsed < CELL_TAPS[cell].pick) return cell;
  }
  return null;
}

/** Whether the added task's `cell` is highlighted: as the pointer rests on it before its tap, and while its menu is open. */
export function tasksCellHighlighted(elapsed: number, cell: TasksSceneCell): boolean {
  return elapsed >= CELL_TAPS[cell].tap - PROJECTS_HOVER_LEAD_MS && elapsed < CELL_TAPS[cell].pick;
}

/** Whether the option about to be picked in `cell`'s open menu shows as hovered, before its tap lands. */
export function tasksOptionHovered(elapsed: number, cell: TasksSceneCell): boolean {
  const { pick } = CELL_TAPS[cell];
  return elapsed >= pick - PROJECTS_HOVER_LEAD_MS && elapsed < pick;
}

/** The scene's touches, each from 0 to 1 as it shows. */
export interface TasksSceneTouches {
  add: number;
  status: number;
  statusOption: number;
  priority: number;
  priorityOption: number;
}

/** The scene's touches at `elapsed`. */
export function tasksTouches(elapsed: number): TasksSceneTouches {
  return {
    add: filmTap(elapsed, B.addTap),
    status: filmTap(elapsed, B.statusTap),
    statusOption: filmTap(elapsed, B.statusPick),
    priority: filmTap(elapsed, B.priorityTap),
    priorityOption: filmTap(elapsed, B.priorityPick),
  };
}
