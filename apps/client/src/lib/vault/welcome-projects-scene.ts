/**
 * Timing for the welcome scene on Projects. The Thesis project's List view shows its tasks, and its settings open and
 * scroll to Event defaults, where the duration is set to one hour and the Pomodoro to Creative, beside the project's
 * color and playlists that are already set, and the settings are saved. Back in the list, three tasks are checked, and
 * the selection bar's Schedule form, whose minutes already read the project's hour, schedules them back to back. The
 * Calendar then shows three events, one per task, each with the project's color, rhythm, and music, sharing one Focus
 * rail, and the first one's Focus starts on its own.
 */

import {
  easeInOutCubic,
  filmDayRails,
  filmSpan,
  filmTap,
  monotoneCurve,
  type FilmKeyframe,
  type FilmRail,
  type FilmSpan,
} from "./welcome-film";
import type { PomodoroPresetKey } from "$lib/pomodoro/rhythm";

/** The default task statuses the scene shows, by their localized name's key, with their palette slots. */
export type ProjectsSceneStatus = "backlog" | "todo" | "inProgress" | "done";

/** The default task priorities the scene shows, by their localized name's key. */
export type ProjectsScenePriority = "urgent" | "high" | "normal" | "low";

/** The scene's tasks, by their localized title's key. */
export type ProjectsSceneTask = "defineQuestion" | "writeIntroduction" | "analyzeSurvey" | "draftDiscussion" | "formatReferences";

/** One task in the Thesis project's General section. Dates are days from the scene's day, or null when unset. */
export interface ProjectsSceneTaskEntry {
  key: ProjectsSceneTask;
  status: ProjectsSceneStatus;
  priority: ProjectsScenePriority;
  start: number | null;
  due: number | null;
}

/** The palette slot of each default status, as a new project seeds them. */
export const PROJECTS_STATUS_COLORS: Readonly<Record<ProjectsSceneStatus, number>> = {
  backlog: 30,
  todo: 31,
  inProgress: 19,
  done: 13,
};

/** The palette slot of each default priority, as a new project seeds them. */
export const PROJECTS_PRIORITY_COLORS: Readonly<Record<ProjectsScenePriority, number>> = {
  urgent: 2,
  high: 7,
  normal: 19,
  low: 30,
};

/** The General section's tasks, in list order. */
export const PROJECTS_TASKS: readonly ProjectsSceneTaskEntry[] = [
  { key: "defineQuestion", status: "done", priority: "normal", start: -14, due: -7 },
  { key: "writeIntroduction", status: "inProgress", priority: "high", start: -2, due: 3 },
  { key: "analyzeSurvey", status: "todo", priority: "urgent", start: 0, due: 2 },
  { key: "draftDiscussion", status: "todo", priority: "normal", start: 1, due: 6 },
  { key: "formatReferences", status: "backlog", priority: "low", start: null, due: 10 },
];

/** The rows checked for scheduling, in the order they are tapped. */
export const PROJECTS_SELECTED_ROWS: readonly number[] = [1, 2, 3];

/** The project's default duration once it is set, in minutes, which the Schedule form's minutes start from. */
export const PROJECTS_DEFAULT_MINUTES = 60;

/** The project's Pomodoro preset once it is set. */
export const PROJECTS_PRESET: PomodoroPresetKey = "creative";

/** Where the Schedule form starts the first event, in minutes of the day. */
export const PROJECTS_SCHEDULE_START = 10 * 60;

/** The scheduled events, back to back, one per checked task in list order, in minutes of the day. */
export const PROJECTS_EVENTS: readonly (FilmSpan & { row: number })[] = PROJECTS_SELECTED_ROWS.map((row, index) => ({
  row,
  start: PROJECTS_SCHEDULE_START + index * PROJECTS_DEFAULT_MINUTES,
  end: PROJECTS_SCHEDULE_START + (index + 1) * PROJECTS_DEFAULT_MINUTES,
}));

/** The rail the events share once they are all on the day, planned by the project's rhythm. */
export const PROJECTS_RAIL: FilmRail = railFor(PROJECTS_EVENTS.length);

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const PROJECTS_BEATS = {
  /** A tap on the header's settings button opens Project settings, which scroll to Event defaults. */
  settingsTap: 1300,
  defaultsScrollStart: 1800,
  defaultsScrollEnd: 2600,
  /** A tap on the Duration trigger opens its options, and a tap on one hour picks it. */
  durationTap: 3500,
  durationPick: 4700,
  /** A tap on the Pomodoro trigger opens its presets, and a tap on Creative picks it. */
  pomodoroTap: 5700,
  pomodoroPick: 7100,
  /** The settings scroll on to the project's playlists, already set. */
  musicScrollStart: 7500,
  musicScrollEnd: 8100,
  /** Save settings saves the defaults and closes the panel. */
  saveTap: 9100,
  /** Three taps check three tasks, and the selection bar shows from the first. */
  selectFirstTap: 10100,
  selectSecondTap: 10700,
  selectThirdTap: 11300,
  /** A tap on Schedule opens its form, whose minutes already read the project's hour. */
  scheduleTap: 12400,
  minutesBumpStart: 12800,
  minutesBumpEnd: 13900,
  /** Schedule selected schedules the three tasks back to back. */
  scheduleSubmitTap: 14700,
  /** The list gives way to the Calendar. */
  listOut: 15100,
  calendarIn: 15500,
  /** The three events land on the day one after another. */
  firstEventIn: 15800,
  secondEventIn: 16200,
  thirdEventIn: 16600,
  /** The first event's Focus starts on its own. */
  focusStart: 18600,
  end: 21000,
} as const;

const B = PROJECTS_BEATS;

/** How long the last frame rests before the scene starts over: just long enough to read the running Focus. */
export const PROJECTS_END_HOLD_MS = 600;

/** The frame shown as a still with reduced motion: the three events on the day with the first one's Focus running. */
export const PROJECTS_STILL = B.focusStart + 1200;

/** The moments the selected tasks are checked, in row order. */
const SELECT_TAPS: readonly number[] = [B.selectFirstTap, B.selectSecondTap, B.selectThirdTap];

/** The moments the events land, in time order. */
const EVENT_INS: readonly number[] = [B.firstEventIn, B.secondEventIn, B.thirdEventIn];

const SCREEN_FADE_MS = 300;
/** How long the settings panel takes to show and to go once Save is tapped. */
const PANEL_FADE_MS = 140;
/** How long a scheduled event takes to land on the day. */
const EVENT_IN_MS = 320;
/** How long a checkbox's touch shows before it lands, so the checkbox shows under the pointer as on hover. */
const HOVER_LEAD_MS = 360;

/** The current time against scene time, in minutes of the day: it reaches the first event and walks into it. */
const CLOCK: readonly FilmKeyframe[] = [
  [B.calendarIn, PROJECTS_SCHEDULE_START - 8],
  [B.focusStart, PROJECTS_SCHEDULE_START],
  [B.end, PROJECTS_SCHEDULE_START + 3],
];

/** The rail shared by the first `count` events, planned as the Calendar plans back to back events of one rhythm. */
function railFor(count: number): FilmRail {
  return filmDayRails(PROJECTS_EVENTS.slice(0, count).map(({ start, end }) => ({ start, end, preset: PROJECTS_PRESET })))[0];
}

/** How far the List view shows, from 0 to 1. */
export function projectsListShown(elapsed: number): number {
  return 1 - easeInOutCubic(filmSpan(elapsed, B.listOut, B.listOut + SCREEN_FADE_MS));
}

/** How far the Calendar shows, from 0 to 1. */
export function projectsCalendarShown(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, B.calendarIn - SCREEN_FADE_MS, B.calendarIn));
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
 * How far the settings have scrolled, each from 0 to 1: `defaults` to the Event defaults heading, then `music` on to
 * the project's playlists below it.
 */
export function projectsSettingsScroll(elapsed: number): { defaults: number; music: number } {
  return {
    defaults: easeInOutCubic(filmSpan(elapsed, B.defaultsScrollStart, B.defaultsScrollEnd)),
    music: easeInOutCubic(filmSpan(elapsed, B.musicScrollStart, B.musicScrollEnd)),
  };
}

/** The Event defaults picker that is open, or null while both are closed. */
export function projectsSettingsPicker(elapsed: number): "duration" | "pomodoro" | null {
  if (elapsed >= B.durationTap && elapsed < B.durationPick) return "duration";
  if (elapsed >= B.pomodoroTap && elapsed < B.pomodoroPick) return "pomodoro";
  return null;
}

/** The project's default duration in minutes, or null before one is picked. */
export function projectsDefaultMinutes(elapsed: number): number | null {
  return elapsed >= B.durationPick ? PROJECTS_DEFAULT_MINUTES : null;
}

/** The project's Pomodoro preset, or null before one is picked. */
export function projectsDefaultPreset(elapsed: number): PomodoroPresetKey | null {
  return elapsed >= B.pomodoroPick ? PROJECTS_PRESET : null;
}

/** Whether the settings have unsaved changes, so Save settings shows as the primary action. */
export function projectsSettingsDirty(elapsed: number): boolean {
  return elapsed >= B.durationPick && elapsed < B.saveTap;
}

/** The settings' touches, each from 0 to 1 as it shows. */
export function projectsSettingsTouches(elapsed: number): {
  settings: number;
  duration: number;
  durationOption: number;
  pomodoro: number;
  pomodoroOption: number;
  save: number;
} {
  return {
    settings: filmTap(elapsed, B.settingsTap),
    duration: filmTap(elapsed, B.durationTap),
    durationOption: filmTap(elapsed, B.durationPick),
    pomodoro: filmTap(elapsed, B.pomodoroTap),
    pomodoroOption: filmTap(elapsed, B.pomodoroPick),
    save: filmTap(elapsed, B.saveTap),
  };
}

/** Whether the task at `row` is checked. */
export function projectsRowChecked(elapsed: number, row: number): boolean {
  const index = PROJECTS_SELECTED_ROWS.indexOf(row);
  return index >= 0 && elapsed >= SELECT_TAPS[index];
}

/** Whether the task at `row` shows its checkbox: once checked, or while the pointer rests on it before the tap. */
export function projectsRowCheckboxShown(elapsed: number, row: number): boolean {
  const index = PROJECTS_SELECTED_ROWS.indexOf(row);
  return index >= 0 && elapsed >= SELECT_TAPS[index] - HOVER_LEAD_MS;
}

/** The touch on the checkbox of the task at `row`, from 0 to 1 as it shows. */
export function projectsRowTouch(elapsed: number, row: number): number {
  const index = PROJECTS_SELECTED_ROWS.indexOf(row);
  return index >= 0 ? filmTap(elapsed, SELECT_TAPS[index]) : 0;
}

/** How many tasks are checked. */
export function projectsSelectedCount(elapsed: number): number {
  return SELECT_TAPS.filter((tap) => elapsed >= tap).length;
}

/** Whether the Schedule form is open: from the tap on Schedule until the list gives way. */
export function projectsScheduleOpen(elapsed: number): boolean {
  return elapsed >= B.scheduleTap && elapsed < B.listOut;
}

/** The Schedule form's touches, each from 0 to 1 as it shows. */
export function projectsScheduleTouches(elapsed: number): { schedule: number; submit: number } {
  return { schedule: filmTap(elapsed, B.scheduleTap), submit: filmTap(elapsed, B.scheduleSubmitTap) };
}

/**
 * How far the Schedule form's minutes lean toward the primary color, from 0 to 1 and back, to tie the value to the
 * project's default duration; 0 outside that moment.
 */
export function projectsMinutesBump(elapsed: number): number {
  const progress = filmSpan(elapsed, B.minutesBumpStart, B.minutesBumpEnd);
  return progress > 0 && progress < 1 ? Math.sin(Math.PI * progress) : 0;
}

/** How far the scheduled event at `index` has landed on the day, from 0 to 1. */
export function projectsEventIn(elapsed: number, index: number): number {
  const landing = EVENT_INS[index];
  return landing === undefined ? 0 : easeInOutCubic(filmSpan(elapsed, landing, landing + EVENT_IN_MS));
}

/** How many scheduled events are on the day, counting one from the moment it starts to land. */
export function projectsEventsShown(elapsed: number): number {
  return EVENT_INS.filter((landing) => elapsed >= landing).length;
}

/** The rail of the events on the day, or null before the first lands; it grows as each event joins it. */
export function projectsEventRail(elapsed: number): FilmRail | null {
  const count = projectsEventsShown(elapsed);
  return count === 0 ? null : count === PROJECTS_EVENTS.length ? PROJECTS_RAIL : railFor(count);
}

/** The current time, in minutes of the day. */
export function projectsClock(elapsed: number): number {
  return monotoneCurve(CLOCK, elapsed);
}

/** Whether the first event's Focus runs. */
export function projectsFocusing(elapsed: number): boolean {
  return elapsed >= B.focusStart;
}

/** The first focus phase's remaining and total seconds, as the Focus ring counts them; zeros before it starts. */
export function projectsPhaseSeconds(elapsed: number): { remaining: number; total: number } {
  const span = PROJECTS_RAIL.focus[0];
  if (!projectsFocusing(elapsed) || !span) return { remaining: 0, total: 0 };
  const remaining = Math.max(0, Math.round((span.end - projectsClock(elapsed)) * 60));
  return { remaining, total: Math.round((span.end - span.start) * 60) };
}

/** The focus the rail has recorded up to the current time, in minutes of the day, once the first Focus runs. */
export function projectsRailRecorded(elapsed: number): FilmSpan[] {
  if (!projectsFocusing(elapsed)) return [];
  const now = Math.min(projectsClock(elapsed), PROJECTS_RAIL.end);
  return PROJECTS_RAIL.focus.filter((span) => span.start < now).map((span) => ({ start: span.start, end: Math.min(span.end, now) }));
}
