/**
 * Timing for the welcome scene on tasks. In the Novel project's List view, a task is typed into the add row at the
 * bottom of the list and added, its row is expanded and two subtasks are typed into its add-subtask row, and then the
 * task's status, its first subtask's status, and the task's priority are changed from their cells in the table.
 */

import { cursorClick, cursorRest, WELCOME_CURSOR_SCROLL_OVERLAP_MS, type WelcomeCursorScript } from "./welcome-cursor";
import { easeInOutCubic, filmCaretOn, filmSpan, filmTap, filmTyped } from "./welcome-film";
import {
  PROJECTS_CURSOR_REST_ANCHOR,
  PROJECTS_HOVER_LEAD_MS,
  type ProjectsScenePriority,
  type ProjectsSceneStatus,
  type ProjectsSceneTaskEntry,
} from "./welcome-projects-scene";

/** The cell menus the scene opens in the list: the added task's status and priority, and its first subtask's status. */
export type TasksSceneCell = "status" | "subtaskStatus" | "priority";

/** The `data-cursor` name of each cell the scene opens. */
export const TASKS_CELL_TARGETS: Readonly<Record<TasksSceneCell, string>> = {
  status: "tasks-status",
  subtaskStatus: "tasks-subtask-status",
  priority: "tasks-priority",
};

/** The task the scene adds, as a new task starts: the default status and priority, with no dates. */
export const TASKS_NEW_TASK: ProjectsSceneTaskEntry = { key: "draftChapter", status: "todo", priority: "normal", start: null, due: null };

/** The subtasks the scene adds under the new task, in the order they are typed, each as a new subtask starts. */
export const TASKS_SUBTASKS: readonly ProjectsSceneTaskEntry[] = [
  { key: "outlineScenes", status: "todo", priority: "normal", start: null, due: null },
  { key: "firstDraft", status: "todo", priority: "normal", start: null, due: null },
];

/** The status and priority the added task is given from its cells; its first subtask is given the same status. */
export const TASKS_NEW_STATUS: ProjectsSceneStatus = "inProgress";
export const TASKS_NEW_PRIORITY: ProjectsScenePriority = "high";

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const TASKS_BEATS = {
  /** A tap on the add row opens its field, a title is typed, and Enter adds the task, keeping the field open. */
  addTap: 800,
  taskTypeStart: 1000,
  taskTypeEnd: 1800,
  taskSubmit: 2200,
  /** A tap on the new task's chevron closes the add field and expands the task, showing its add-subtask row. */
  expandTap: 3100,
  /** A tap on the add-subtask row opens its field, and two subtasks are typed and added with Enter, one after the other. */
  subtaskAddTap: 3900,
  firstSubtaskTypeStart: 4100,
  firstSubtaskTypeEnd: 4900,
  firstSubtaskSubmit: 5300,
  secondSubtaskTypeStart: 5500,
  secondSubtaskTypeEnd: 6300,
  secondSubtaskSubmit: 6700,
  /** The list scrolls the new task's status into view, a tap opens its menu and closes the subtask field, and In progress is picked. */
  statusScrollStart: 7000,
  statusScrollEnd: 7500,
  statusTap: 7900,
  statusPick: 9200,
  /** A tap on the first subtask's status, in the same column, opens its menu, and In progress is picked. */
  subtaskStatusTap: 10100,
  subtaskStatusPick: 11400,
  /** The list scrolls on to the task's priority, a tap opens its menu, and High is picked. */
  priorityScrollStart: 11700,
  priorityScrollEnd: 12200,
  priorityTap: 12600,
  priorityPick: 13900,
  end: 15200,
} as const;

const B = TASKS_BEATS;

/** How long the last frame rests before the scene starts over. */
export const TASKS_END_HOLD_MS = 900;

/** The frame shown as a still with reduced motion: the added task and its subtasks, with their new statuses and the task's new priority. */
export const TASKS_STILL = B.end;

/** The tap that opens each cell menu and the tap that picks its option. */
const CELL_TAPS: Readonly<Record<TasksSceneCell, { tap: number; pick: number }>> = {
  status: { tap: B.statusTap, pick: B.statusPick },
  subtaskStatus: { tap: B.subtaskStatusTap, pick: B.subtaskStatusPick },
  priority: { tap: B.priorityTap, pick: B.priorityPick },
};

const CELLS = ["status", "subtaskStatus", "priority"] as const satisfies readonly TasksSceneCell[];

/** When each subtask is typed and added, in the order of {@link TASKS_SUBTASKS}. */
const SUBTASK_TYPING = [
  { start: B.firstSubtaskTypeStart, end: B.firstSubtaskTypeEnd, submit: B.firstSubtaskSubmit },
  { start: B.secondSubtaskTypeStart, end: B.secondSubtaskTypeEnd, submit: B.secondSubtaskSubmit },
] as const;

/** Whether the add row's field is open: from its tap until the tap on the new task's chevron closes it. */
export function tasksAddActive(elapsed: number): boolean {
  return elapsed >= B.addTap && elapsed < B.expandTap;
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

/** Whether the new task is expanded, showing its subtasks and its add-subtask row. */
export function tasksExpanded(elapsed: number): boolean {
  return elapsed >= B.expandTap;
}

/** Whether the add-subtask row's field is open: from its tap until the tap on the new task's status closes it. */
export function tasksSubtaskAddActive(elapsed: number): boolean {
  return elapsed >= B.subtaskAddTap && elapsed < B.statusTap;
}

/** How many of {@link TASKS_SUBTASKS} have been added under the new task. */
export function tasksSubtaskCount(elapsed: number): number {
  return SUBTASK_TYPING.filter((typing) => elapsed >= typing.submit).length;
}

/**
 * As much of the subtask being typed as the add-subtask row's field holds, where `titles` are the subtasks' titles in
 * order: each typed a character at a time, and cleared once it is added.
 */
export function tasksSubtaskDraft(elapsed: number, titles: readonly string[]): string {
  const index = tasksSubtaskCount(elapsed);
  const typing = SUBTASK_TYPING[index];
  const title = titles[index];
  return typing && title !== undefined ? filmTyped(elapsed, title, typing.start, typing.end) : "";
}

/** Whether the add-subtask row's caret shows: steady while each subtask is typed, then blinking while its field stays open. */
export function tasksSubtaskCaretOn(elapsed: number): boolean {
  const typing = elapsed < B.secondSubtaskTypeStart ? SUBTASK_TYPING[0] : SUBTASK_TYPING[1];
  return tasksSubtaskAddActive(elapsed) && filmCaretOn(elapsed, typing.end);
}

/** The added task's status. */
export function tasksNewStatus(elapsed: number): ProjectsSceneStatus {
  return elapsed >= B.statusPick ? TASKS_NEW_STATUS : TASKS_NEW_TASK.status;
}

/** The status of the subtask at `index` in {@link TASKS_SUBTASKS}: the first one's changes from its cell. */
export function tasksSubtaskStatus(elapsed: number, index: number): ProjectsSceneStatus {
  const start = TASKS_SUBTASKS[index]?.status ?? TASKS_NEW_TASK.status;
  return index === 0 && elapsed >= B.subtaskStatusPick ? TASKS_NEW_STATUS : start;
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

/** The cell menu that is open, or null while every one is closed. */
export function tasksCellMenu(elapsed: number): TasksSceneCell | null {
  for (const cell of CELLS) {
    if (elapsed >= CELL_TAPS[cell].tap && elapsed < CELL_TAPS[cell].pick) return cell;
  }
  return null;
}

/** Whether `cell` is highlighted: as the pointer rests on it before its tap, and while its menu is open. */
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
  expand: number;
  subtaskAdd: number;
  status: number;
  statusOption: number;
  subtaskStatus: number;
  subtaskStatusOption: number;
  priority: number;
  priorityOption: number;
}

/** The scene's touches at `elapsed`. */
export function tasksTouches(elapsed: number): TasksSceneTouches {
  return {
    add: filmTap(elapsed, B.addTap),
    expand: filmTap(elapsed, B.expandTap),
    subtaskAdd: filmTap(elapsed, B.subtaskAddTap),
    status: filmTap(elapsed, B.statusTap),
    statusOption: filmTap(elapsed, B.statusPick),
    subtaskStatus: filmTap(elapsed, B.subtaskStatusTap),
    subtaskStatusOption: filmTap(elapsed, B.subtaskStatusPick),
    priority: filmTap(elapsed, B.priorityTap),
    priorityOption: filmTap(elapsed, B.priorityPick),
  };
}

/**
 * The desktop pointer's path through the scene: it clicks into the add row and stays there while the task is typed,
 * expands the new task once it is added and clicks into its add-subtask row while the subtasks are typed, then opens
 * each cell's menu once the list has scrolled to it and picks its option. Both status menus are the same menu, so their
 * option is one place.
 */
export const TASKS_CURSOR: WelcomeCursorScript = [
  cursorRest("projects-list", 0, { anchor: PROJECTS_CURSOR_REST_ANCHOR }),
  cursorClick("tasks-add", B.addTap, { glyph: "text", after: 0 }),
  cursorClick("tasks-expand", B.expandTap, { glyph: "pointer", after: B.taskSubmit }),
  cursorClick("tasks-subtask-add", B.subtaskAddTap, {
    glyph: "text",
    after: B.expandTap,
    busy: [{ start: B.statusScrollStart, end: B.statusScrollEnd }],
  }),
  cursorClick(TASKS_CELL_TARGETS.status, B.statusTap, { glyph: "pointer", after: B.statusScrollEnd - WELCOME_CURSOR_SCROLL_OVERLAP_MS }),
  cursorClick("tasks-status-option", B.statusPick),
  cursorClick(TASKS_CELL_TARGETS.subtaskStatus, B.subtaskStatusTap, { glyph: "pointer", after: B.statusPick }),
  cursorClick("tasks-status-option", B.subtaskStatusPick, { busy: [{ start: B.priorityScrollStart, end: B.priorityScrollEnd }] }),
  cursorClick(TASKS_CELL_TARGETS.priority, B.priorityTap, { glyph: "pointer", after: B.priorityScrollEnd - WELCOME_CURSOR_SCROLL_OVERLAP_MS }),
  cursorClick("tasks-priority-option", B.priorityPick),
];
