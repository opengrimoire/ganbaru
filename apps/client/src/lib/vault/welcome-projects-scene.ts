/**
 * Timing for the welcome scene on Projects. In the Novel project's List view, a task is typed into the add row at the
 * bottom of the list and added, and its status and priority are changed from their cells in the table. Project settings
 * then open and scroll to Event defaults, where an event name is typed, a color, a duration, and a Pomodoro are picked,
 * a playlist is picked for each phase, and the settings are saved.
 */

import { easeInOutCubic, filmCaretOn, filmSpan, filmTap, filmTyped } from "./welcome-film";
import type { MusicScenePlaylist } from "./welcome-music-scene";
import type { PomodoroPresetKey } from "$lib/pomodoro/rhythm";

/** The default task statuses, by their localized name's key, in the order a new project seeds them. */
export type ProjectsSceneStatus = "backlog" | "todo" | "inProgress" | "inReview" | "blocked" | "done";

/** The default task priorities, by their localized name's key, in the order a new project seeds them. */
export type ProjectsScenePriority = "urgent" | "high" | "normal" | "low";

/** The scene's tasks, by their localized title's key. */
export type ProjectsSceneTask = "outlinePlot" | "writeOpening" | "buildCharacters" | "researchSetting" | "draftChapter";

/** The Pomodoro phases a project picks a playlist for, in the order the settings list them. */
export type ProjectsScenePhase = "focus" | "short-break" | "long-break";

/** The cell menus the scene opens in the list. */
export type ProjectsSceneCell = "status" | "priority";

/** The Event defaults pickers the scene opens in the settings. */
export type ProjectsScenePicker = "color" | "duration" | "pomodoro" | ProjectsScenePhase;

/** One task in the Novel project's General section. Dates are days from the scene's day, or null when unset. */
export interface ProjectsSceneTaskEntry {
  key: ProjectsSceneTask;
  status: ProjectsSceneStatus;
  priority: ProjectsScenePriority;
  start: number | null;
  due: number | null;
}

/** The default statuses in the order a new project seeds them, as the status menu lists them. */
export const PROJECTS_STATUS_ORDER: readonly ProjectsSceneStatus[] = ["backlog", "todo", "inProgress", "inReview", "blocked", "done"];

/** The default priorities in the order a new project seeds them, as the priority menu lists them. */
export const PROJECTS_PRIORITY_ORDER: readonly ProjectsScenePriority[] = ["urgent", "high", "normal", "low"];

/** The palette slot of each default status, as a new project seeds them. */
export const PROJECTS_STATUS_COLORS: Readonly<Record<ProjectsSceneStatus, number>> = {
  backlog: 30,
  todo: 31,
  inProgress: 19,
  inReview: 23,
  blocked: 2,
  done: 13,
};

/** The palette slot of each default priority, as a new project seeds them. */
export const PROJECTS_PRIORITY_COLORS: Readonly<Record<ProjectsScenePriority, number>> = {
  urgent: 2,
  high: 7,
  normal: 19,
  low: 30,
};

/** The General section's tasks before the scene adds one, in list order. */
export const PROJECTS_TASKS: readonly ProjectsSceneTaskEntry[] = [
  { key: "outlinePlot", status: "done", priority: "normal", start: -10, due: -4 },
  { key: "writeOpening", status: "inProgress", priority: "high", start: -2, due: 2 },
  { key: "buildCharacters", status: "todo", priority: "normal", start: 1, due: 5 },
  { key: "researchSetting", status: "backlog", priority: "low", start: null, due: 9 },
];

/** The task the scene adds, as a new task starts: the default status and priority, with no dates. */
export const PROJECTS_NEW_TASK: ProjectsSceneTaskEntry = { key: "draftChapter", status: "todo", priority: "normal", start: null, due: null };

/** The status and priority the added task is given from its cells. */
export const PROJECTS_NEW_STATUS: ProjectsSceneStatus = "inProgress";
export const PROJECTS_NEW_PRIORITY: ProjectsScenePriority = "high";

/** The project's event color once it is picked, a palette slot. */
export const PROJECTS_COLOR = 14;

/** The project's default duration once it is picked, in minutes. */
export const PROJECTS_DEFAULT_MINUTES = 120;

/** The project's Pomodoro preset once it is picked. */
export const PROJECTS_PRESET: PomodoroPresetKey = "creative";

/** The phases in the order the settings list their playlists. */
export const PROJECTS_PHASES: readonly ProjectsScenePhase[] = ["focus", "short-break", "long-break"];

/** The playlist picked for each phase. */
export const PROJECTS_PLAYLISTS: Readonly<Record<ProjectsScenePhase, MusicScenePlaylist>> = {
  focus: "workFocus",
  "short-break": "breakCalm",
  "long-break": "breakActive",
};

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const PROJECTS_BEATS = {
  /** A tap on the add row opens its field, a title is typed, and Enter adds the task, keeping the field open. */
  addTap: 800,
  taskTypeStart: 1000,
  taskTypeEnd: 1900,
  taskSubmit: 2300,
  /** The list scrolls the new task's status into view, a tap opens its menu and closes the add field, and In progress is picked. */
  statusScrollStart: 2600,
  statusScrollEnd: 3100,
  statusTap: 3500,
  statusPick: 4300,
  /** The list scrolls on to its priority, a tap opens its menu, and High is picked. */
  priorityScrollStart: 4600,
  priorityScrollEnd: 5100,
  priorityTap: 5500,
  priorityPick: 6300,
  /** A tap on the header's settings button opens Project settings, which scroll to Event defaults. */
  settingsTap: 7100,
  defaultsScrollStart: 7500,
  defaultsScrollEnd: 8200,
  /** A tap on the event name field focuses it, and the name is typed. */
  nameTap: 8700,
  nameTypeStart: 8900,
  nameTypeEnd: 9800,
  /** A tap on the color opens the palette, a tap picks a swatch, and on desktop a tap beside the palette closes it. */
  colorTap: 10400,
  colorPick: 11200,
  colorDismiss: 11700,
  /** The Duration and Pomodoro pickers each open with a tap, and a tap picks an option. */
  durationTap: 12300,
  durationPick: 13100,
  pomodoroTap: 13700,
  pomodoroPick: 14600,
  /** The settings scroll on to the playlists, and each phase's picker opens and takes a playlist. */
  musicScrollStart: 14900,
  musicScrollEnd: 15500,
  focusTap: 16000,
  focusPick: 16800,
  shortBreakTap: 17300,
  shortBreakPick: 18100,
  longBreakTap: 18600,
  longBreakPick: 19400,
  /** Save settings saves the defaults and closes the panel. */
  saveTap: 20200,
  end: 21400,
} as const;

const B = PROJECTS_BEATS;

/** How long the last frame rests before the scene starts over. */
export const PROJECTS_END_HOLD_MS = 900;

/** The frame shown as a still with reduced motion: the settings with every event default set, before they are saved. */
export const PROJECTS_STILL = B.longBreakPick + 400;

/** How long the settings panel takes to show, and to go once Save is tapped. */
const PANEL_FADE_MS = 140;
/** How long a cell or a picked option shows as hovered before its tap lands, as the pointer rests on it. */
const HOVER_LEAD_MS = 420;

/** The tap that opens each cell menu and the tap that picks its option. */
const CELL_TAPS: Readonly<Record<ProjectsSceneCell, { tap: number; pick: number }>> = {
  status: { tap: B.statusTap, pick: B.statusPick },
  priority: { tap: B.priorityTap, pick: B.priorityPick },
};

/** The tap that opens each picker and the tap that picks its option. */
const PICKER_TAPS: Readonly<Record<ProjectsScenePicker, { tap: number; pick: number }>> = {
  color: { tap: B.colorTap, pick: B.colorPick },
  duration: { tap: B.durationTap, pick: B.durationPick },
  pomodoro: { tap: B.pomodoroTap, pick: B.pomodoroPick },
  focus: { tap: B.focusTap, pick: B.focusPick },
  "short-break": { tap: B.shortBreakTap, pick: B.shortBreakPick },
  "long-break": { tap: B.longBreakTap, pick: B.longBreakPick },
};

/** Whether the add row's field is open: from its tap until a tap elsewhere in the list closes it. */
export function projectsAddActive(elapsed: number): boolean {
  return elapsed >= B.addTap && elapsed < B.statusTap;
}

/** As much of `title` as the add row's field holds: typed a character at a time, and cleared once the task is added. */
export function projectsTaskDraft(elapsed: number, title: string): string {
  return elapsed >= B.taskSubmit ? "" : filmTyped(elapsed, title, B.taskTypeStart, B.taskTypeEnd);
}

/** Whether the add row's caret shows: steady while typing, then blinking while its field stays open. */
export function projectsTaskCaretOn(elapsed: number): boolean {
  return projectsAddActive(elapsed) && filmCaretOn(elapsed, B.taskTypeEnd);
}

/** Whether the typed task has been added to the list. */
export function projectsTaskAdded(elapsed: number): boolean {
  return elapsed >= B.taskSubmit;
}

/** The added task's status. */
export function projectsNewStatus(elapsed: number): ProjectsSceneStatus {
  return elapsed >= B.statusPick ? PROJECTS_NEW_STATUS : PROJECTS_NEW_TASK.status;
}

/** The added task's priority. */
export function projectsNewPriority(elapsed: number): ProjectsScenePriority {
  return elapsed >= B.priorityPick ? PROJECTS_NEW_PRIORITY : PROJECTS_NEW_TASK.priority;
}

/**
 * How far the list has scrolled sideways, each from 0 to 1: `status` until the added task's status cell is in view,
 * then `priority` on until its priority cell is.
 */
export function projectsListScroll(elapsed: number): { status: number; priority: number } {
  return {
    status: easeInOutCubic(filmSpan(elapsed, B.statusScrollStart, B.statusScrollEnd)),
    priority: easeInOutCubic(filmSpan(elapsed, B.priorityScrollStart, B.priorityScrollEnd)),
  };
}

/** The added task's cell menu that is open, or null while both are closed. */
export function projectsCellMenu(elapsed: number): ProjectsSceneCell | null {
  for (const cell of ["status", "priority"] as const) {
    if (elapsed >= CELL_TAPS[cell].tap && elapsed < CELL_TAPS[cell].pick) return cell;
  }
  return null;
}

/** Whether the added task's `cell` is highlighted: as the pointer rests on it before its tap, and while its menu is open. */
export function projectsCellHighlighted(elapsed: number, cell: ProjectsSceneCell): boolean {
  return elapsed >= CELL_TAPS[cell].tap - HOVER_LEAD_MS && elapsed < CELL_TAPS[cell].pick;
}

/** Whether the option about to be picked in `target`'s open menu or picker shows as hovered, before its tap lands. */
export function projectsOptionHovered(elapsed: number, target: ProjectsSceneCell | ProjectsScenePicker): boolean {
  const { pick } = target === "status" || target === "priority" ? CELL_TAPS[target] : PICKER_TAPS[target];
  return elapsed >= pick - HOVER_LEAD_MS && elapsed < pick;
}

/** How far Project settings show, from 0 to 1: they open with the settings tap and close with Save. */
export function projectsSettingsShown(elapsed: number): number {
  const shown = filmSpan(elapsed, B.settingsTap, B.settingsTap + PANEL_FADE_MS);
  const gone = filmSpan(elapsed, B.saveTap, B.saveTap + PANEL_FADE_MS);
  return shown * (1 - gone);
}

/** Whether Project settings are open, which marks the header's settings button as active. */
export function projectsSettingsOpen(elapsed: number): boolean {
  return elapsed >= B.settingsTap && elapsed < B.saveTap;
}

/**
 * How far the settings have scrolled, each from 0 to 1: `defaults` to the Event defaults heading, then `music` on until
 * the playlists are in view.
 */
export function projectsSettingsScroll(elapsed: number): { defaults: number; music: number } {
  return {
    defaults: easeInOutCubic(filmSpan(elapsed, B.defaultsScrollStart, B.defaultsScrollEnd)),
    music: easeInOutCubic(filmSpan(elapsed, B.musicScrollStart, B.musicScrollEnd)),
  };
}

/**
 * The Event defaults picker that is open, or null while all are closed. The color palette closes as its swatch is
 * picked when `colorClosesOnPick`, as on a phone, and otherwise with the tap beside it.
 */
export function projectsSettingsPicker(elapsed: number, colorClosesOnPick: boolean): ProjectsScenePicker | null {
  for (const picker of Object.keys(PICKER_TAPS) as ProjectsScenePicker[]) {
    const { tap, pick } = PICKER_TAPS[picker];
    const close = picker === "color" && !colorClosesOnPick ? B.colorDismiss : pick;
    if (elapsed >= tap && elapsed < close) return picker;
  }
  return null;
}

/** Whether the event name field has focus: from its tap until the tap on the color moves on. */
export function projectsEventNameFocused(elapsed: number): boolean {
  return elapsed >= B.nameTap && elapsed < B.colorTap;
}

/** As much of `name` as has been typed into the event name field. */
export function projectsEventName(elapsed: number, name: string): string {
  return filmTyped(elapsed, name, B.nameTypeStart, B.nameTypeEnd);
}

/** Whether the event name field's caret shows: steady while typing, then blinking while the field has focus. */
export function projectsEventNameCaretOn(elapsed: number): boolean {
  return projectsEventNameFocused(elapsed) && filmCaretOn(elapsed, B.nameTypeEnd);
}

/** The project's event color, or null before one is picked. */
export function projectsDefaultColor(elapsed: number): number | null {
  return elapsed >= B.colorPick ? PROJECTS_COLOR : null;
}

/** The project's default duration in minutes, or null before one is picked. */
export function projectsDefaultMinutes(elapsed: number): number | null {
  return elapsed >= B.durationPick ? PROJECTS_DEFAULT_MINUTES : null;
}

/** The project's Pomodoro preset, or null before one is picked. */
export function projectsDefaultPreset(elapsed: number): PomodoroPresetKey | null {
  return elapsed >= B.pomodoroPick ? PROJECTS_PRESET : null;
}

/** The project's playlist for `phase`, or null before one is picked. */
export function projectsPlaylist(elapsed: number, phase: ProjectsScenePhase): MusicScenePlaylist | null {
  return elapsed >= PICKER_TAPS[phase].pick ? PROJECTS_PLAYLISTS[phase] : null;
}

/** Whether the settings have unsaved changes, so Save settings shows as the primary action. */
export function projectsSettingsDirty(elapsed: number): boolean {
  return elapsed >= B.nameTypeStart && elapsed < B.saveTap;
}

/** The scene's touches, each from 0 to 1 as it shows. */
export interface ProjectsSceneTouches {
  add: number;
  status: number;
  statusOption: number;
  priority: number;
  priorityOption: number;
  settings: number;
  name: number;
  color: number;
  colorOption: number;
  colorDismiss: number;
  duration: number;
  durationOption: number;
  pomodoro: number;
  pomodoroOption: number;
  playlist: Record<ProjectsScenePhase, number>;
  playlistOption: Record<ProjectsScenePhase, number>;
  save: number;
}

/** The scene's touches at `elapsed`; the tap beside the color palette only shows when the palette waits for it. */
export function projectsTouches(elapsed: number, colorClosesOnPick: boolean): ProjectsSceneTouches {
  const byPhase = (beat: "tap" | "pick") =>
    Object.fromEntries(PROJECTS_PHASES.map((phase) => [phase, filmTap(elapsed, PICKER_TAPS[phase][beat])])) as Record<ProjectsScenePhase, number>;
  return {
    add: filmTap(elapsed, B.addTap),
    status: filmTap(elapsed, B.statusTap),
    statusOption: filmTap(elapsed, B.statusPick),
    priority: filmTap(elapsed, B.priorityTap),
    priorityOption: filmTap(elapsed, B.priorityPick),
    settings: filmTap(elapsed, B.settingsTap),
    name: filmTap(elapsed, B.nameTap),
    color: filmTap(elapsed, B.colorTap),
    colorOption: filmTap(elapsed, B.colorPick),
    colorDismiss: colorClosesOnPick ? 0 : filmTap(elapsed, B.colorDismiss),
    duration: filmTap(elapsed, B.durationTap),
    durationOption: filmTap(elapsed, B.durationPick),
    pomodoro: filmTap(elapsed, B.pomodoroTap),
    pomodoroOption: filmTap(elapsed, B.pomodoroPick),
    playlist: byPhase("tap"),
    playlistOption: byPhase("pick"),
    save: filmTap(elapsed, B.saveTap),
  };
}
