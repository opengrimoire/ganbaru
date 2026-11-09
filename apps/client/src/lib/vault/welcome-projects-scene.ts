/**
 * Timing for the welcome scene on Projects, and the Novel project the welcome scenes on Projects and on tasks share. From
 * the project's List view, Project settings open on Event defaults, where an event name is typed, a color, a duration,
 * and a Pomodoro are picked, a playlist is picked for each phase, and the settings are saved and closed, the settings
 * scrolling each control into view before its tap. Then in the Calendar, an event is dragged out and its panel opens,
 * where picking the Novel project gives the event every default just set: its name, color, length, Pomodoro, and
 * playlists. The Music section opens on them, and the event is saved.
 */

import {
  cursorClick,
  cursorRest,
  WELCOME_CURSOR_REST_MS,
  WELCOME_CURSOR_SCROLL_OVERLAP_MS,
  type WelcomeCursorScript,
  type WelcomeCursorStop,
} from "./welcome-cursor";
import type { WelcomeEventPanelFrame } from "./welcome-event-panel";
import {
  easeInOutCubic,
  FILM_DEFAULT_PRESET,
  FILM_SHEET_RISE_MS,
  filmCaretOn,
  filmDayRails,
  filmPress,
  filmSpan,
  filmTap,
  filmTyped,
  type FilmRail,
} from "./welcome-film";
import type { MusicScenePlaylist } from "./welcome-music-scene";
import type { PomodoroPresetKey } from "$lib/pomodoro/rhythm";

/** The default task statuses, by their localized name's key, in the order a new project seeds them. */
export type ProjectsSceneStatus = "backlog" | "todo" | "inProgress" | "inReview" | "blocked" | "done";

/** The default task priorities, by their localized name's key, in the order a new project seeds them. */
export type ProjectsScenePriority = "urgent" | "high" | "normal" | "low";

/** The scene's tasks, by their localized title's key. */
export type ProjectsSceneTask =
  | "outlinePlot"
  | "writeOpening"
  | "buildCharacters"
  | "researchSetting"
  | "draftChapter"
  | "outlineScenes"
  | "firstDraft";

/** The Pomodoro phases a project picks a playlist for, in the order the settings list them. */
export type ProjectsScenePhase = "focus" | "short-break" | "long-break";

/** The Event defaults pickers the scene opens in the settings. */
export type ProjectsScenePicker = "color" | "duration" | "pomodoro" | ProjectsScenePhase;

/** The Event defaults controls the settings scroll into view, one at a time, before their taps. */
export type ProjectsSettingsTarget = "name" | ProjectsScenePicker;

/** The settings controls in the order the scene uses them, which is their order in Event defaults. */
export const PROJECTS_SETTINGS_TARGETS: readonly ProjectsSettingsTarget[] = [
  "name",
  "color",
  "duration",
  "pomodoro",
  "focus",
  "short-break",
  "long-break",
];

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

/** The General section's tasks before the scene on tasks adds one, in list order. */
export const PROJECTS_TASKS: readonly ProjectsSceneTaskEntry[] = [
  { key: "outlinePlot", status: "done", priority: "normal", start: -10, due: -4 },
  { key: "writeOpening", status: "inProgress", priority: "high", start: -2, due: 2 },
  { key: "buildCharacters", status: "todo", priority: "normal", start: 1, due: 5 },
  { key: "researchSetting", status: "backlog", priority: "low", start: null, due: 9 },
];

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
  /** A tap on the header's settings button opens Project settings, which scroll to Event defaults. */
  settingsTap: 800,
  nameScrollStart: 1600,
  nameScrollEnd: 2300,
  /** A tap on the event name field focuses it, and the name is typed. */
  nameTap: 2800,
  nameTypeStart: 3000,
  nameTypeEnd: 3900,
  /**
   * The settings scroll the color into view with room for its palette, a tap opens the palette, a tap picks a swatch,
   * which closes the palette.
   */
  colorScrollStart: 4100,
  colorScrollEnd: 4500,
  colorTap: 4900,
  colorPick: 6200,
  /** Each of the other pickers is scrolled into view, opens with a tap, and a tap picks an option. */
  durationScrollStart: 6900,
  durationScrollEnd: 7300,
  durationTap: 7700,
  durationPick: 8900,
  pomodoroScrollStart: 9100,
  pomodoroScrollEnd: 9500,
  pomodoroTap: 9900,
  pomodoroPick: 11100,
  focusScrollStart: 11400,
  focusScrollEnd: 11900,
  focusTap: 12300,
  focusPick: 13600,
  shortBreakScrollStart: 13800,
  shortBreakScrollEnd: 14200,
  shortBreakTap: 14600,
  shortBreakPick: 15800,
  longBreakScrollStart: 16000,
  longBreakScrollEnd: 16400,
  longBreakTap: 16800,
  longBreakPick: 18000,
  /** Save settings saves the defaults and turns back to its idle color, and the close button then closes the panel. */
  settingsSaveTap: 18800,
  settingsCloseTap: 20200,
  /** A click on the Calendar's tab, or a tap on it in the phone's tab bar, and the list gives way to the Calendar. */
  calendarTap: 21300,
  calendarIn: 21600,
  /** A press drags an event out over the empty hour, and letting go opens the event panel on it. */
  createStart: 22500,
  createEnd: 23200,
  panelOpen: 23350,
  /** The project trigger, then the Personal group, then Novel, which gives the event the project's defaults. */
  projectTap: 24350,
  groupTap: 25450,
  projectPick: 26550,
  /** On the phone, a drag on the sheet's grabber takes it from its peek to its full height. */
  sheetDragStart: 27500,
  sheetDragEnd: 28050,
  /** A tap on the Music section's header opens it on the project's playlists. */
  musicTap: 28800,
  /** Save saves the event and closes the panel. */
  eventSaveTap: 30500,
  end: 31900,
} as const;

const B = PROJECTS_BEATS;

/** How long the last frame rests before the scene starts over. */
export const PROJECTS_END_HOLD_MS = 900;

/** The frame shown as a still with reduced motion: the event with every project default, its Music section open. */
export const PROJECTS_STILL = B.musicTap + 1200;

/** The Calendar event, in minutes of the day: the length it is dragged out to, before the project's default replaces it. */
export const PROJECTS_EVENT = { start: 10 * 60, drawnEnd: 11 * 60 } as const;

/** The event's Focus rail before and after the project is picked, planned by the Calendar's own rhythm. */
export const PROJECTS_DEFAULT_RAIL: FilmRail = filmDayRails([
  { start: PROJECTS_EVENT.start, end: PROJECTS_EVENT.drawnEnd, preset: FILM_DEFAULT_PRESET },
])[0];
export const PROJECTS_RAIL: FilmRail = filmDayRails([
  { start: PROJECTS_EVENT.start, end: PROJECTS_EVENT.start + PROJECTS_DEFAULT_MINUTES, preset: PROJECTS_PRESET },
])[0];

/** The current time in the Calendar, in minutes of the day, shortly before the event. */
export const PROJECTS_CLOCK = PROJECTS_EVENT.start - 20;
/** How long the settings panel takes to show, and to go once Save is tapped. */
const PANEL_FADE_MS = 140;
/** How long a list cell or a picked option shows as hovered before its tap lands, as the pointer rests on it. */
export const PROJECTS_HOVER_LEAD_MS = 420;
/**
 * How long the Music section takes to open, as the event sheet slides a section open. The panel's body scrolls it into
 * view over the same time and curve, so the two read as one motion.
 */
const SECTION_OPEN_MS = 360;
/** How long the finger rests on the grabber before the sheet follows it. */
const DRAG_LAND_MS = 160;

/** The scroll that brings each settings control into view, in the order the scene uses them. */
const SETTINGS_SCROLLS: readonly { target: ProjectsSettingsTarget; start: number; end: number }[] = [
  { target: "name", start: B.nameScrollStart, end: B.nameScrollEnd },
  { target: "color", start: B.colorScrollStart, end: B.colorScrollEnd },
  { target: "duration", start: B.durationScrollStart, end: B.durationScrollEnd },
  { target: "pomodoro", start: B.pomodoroScrollStart, end: B.pomodoroScrollEnd },
  { target: "focus", start: B.focusScrollStart, end: B.focusScrollEnd },
  { target: "short-break", start: B.shortBreakScrollStart, end: B.shortBreakScrollEnd },
  { target: "long-break", start: B.longBreakScrollStart, end: B.longBreakScrollEnd },
];

/** The tap that opens each picker and the tap that picks its option. */
const PICKER_TAPS: Readonly<Record<ProjectsScenePicker, { tap: number; pick: number }>> = {
  color: { tap: B.colorTap, pick: B.colorPick },
  duration: { tap: B.durationTap, pick: B.durationPick },
  pomodoro: { tap: B.pomodoroTap, pick: B.pomodoroPick },
  focus: { tap: B.focusTap, pick: B.focusPick },
  "short-break": { tap: B.shortBreakTap, pick: B.shortBreakPick },
  "long-break": { tap: B.longBreakTap, pick: B.longBreakPick },
};

/** Whether the option about to be picked in `picker` shows as hovered, before its tap lands. */
export function projectsOptionHovered(elapsed: number, picker: ProjectsScenePicker): boolean {
  const { pick } = PICKER_TAPS[picker];
  return elapsed >= pick - PROJECTS_HOVER_LEAD_MS && elapsed < pick;
}

/** How far Project settings show, from 0 to 1: they open with the settings tap and close with the close button. */
export function projectsSettingsShown(elapsed: number): number {
  const shown = filmSpan(elapsed, B.settingsTap, B.settingsTap + PANEL_FADE_MS);
  const gone = filmSpan(elapsed, B.settingsCloseTap, B.settingsCloseTap + PANEL_FADE_MS);
  return shown * (1 - gone);
}

/** Whether Project settings are open, which marks the header's settings button as active. */
export function projectsSettingsOpen(elapsed: number): boolean {
  return elapsed >= B.settingsTap && elapsed < B.settingsCloseTap;
}

/**
 * Where the settings are scrolling: from the control the last scroll brought into view, or from the top when `from` is
 * null, to the control the current scroll brings into view, `progress` of the way from 0 to 1. Both are null before the
 * first scroll starts. The scene measures each control to turn this into pixels.
 */
export function projectsSettingsScroll(elapsed: number): {
  from: ProjectsSettingsTarget | null;
  to: ProjectsSettingsTarget | null;
  progress: number;
} {
  const started = SETTINGS_SCROLLS.filter((scroll) => elapsed >= scroll.start);
  const current = started.at(-1);
  if (!current) return { from: null, to: null, progress: 0 };
  return {
    from: started.at(-2)?.target ?? null,
    to: current.target,
    progress: easeInOutCubic(filmSpan(elapsed, current.start, current.end)),
  };
}

/**
 * The Event defaults picker that is open, or null while all are closed. Each picker, the color palette included,
 * closes as its option is picked.
 */
export function projectsSettingsPicker(elapsed: number): ProjectsScenePicker | null {
  for (const picker of Object.keys(PICKER_TAPS) as ProjectsScenePicker[]) {
    const { tap, pick } = PICKER_TAPS[picker];
    if (elapsed >= tap && elapsed < pick) return picker;
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
  return elapsed >= B.nameTypeStart && elapsed < B.settingsSaveTap;
}

/** How far the list shows, from 0 to 1, as the Calendar fades in over it once its tab is picked. */
export function projectsListShown(elapsed: number): number {
  return 1 - projectsCalendarShown(elapsed);
}

/** How far the Calendar shows, from 0 to 1, fading in from the tap on its tab. */
export function projectsCalendarShown(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, B.calendarTap, B.calendarIn));
}

/** Whether the Calendar's tab is the one showing: from the tap on it. */
export function projectsCalendarTabActive(elapsed: number): boolean {
  return elapsed >= B.calendarTap;
}

/** Whether the event is on the day: from the press that starts drawing it. */
export function projectsEventDrawnIn(elapsed: number): boolean {
  return elapsed >= B.createStart;
}

/** How far the event has been dragged out to its drawn length, from 0 to 1, easing as the press moves. */
export function projectsEventDrawn(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, B.createStart, B.createEnd));
}

/** Whether the event is still being created: from the press that draws it until Save. */
export function projectsEventCreating(elapsed: number): boolean {
  return elapsed >= B.createStart && elapsed < B.eventSaveTap;
}

/** Whether the event panel is open: from letting go of the drawn event until Save is tapped. */
export function projectsPanelOpen(elapsed: number): boolean {
  return elapsed >= B.panelOpen && elapsed < B.eventSaveTap;
}

/** Whether the event has the Novel project, and with it every default the settings saved. */
export function projectsEventPicked(elapsed: number): boolean {
  return elapsed >= B.projectPick;
}

/**
 * The event's length in minutes: it grows with the press up to its drawn length, then takes the project's default
 * duration at once when the project is picked, as the Calendar applies it.
 */
export function projectsEventMinutes(elapsed: number): number {
  if (projectsEventPicked(elapsed)) return PROJECTS_DEFAULT_MINUTES;
  return (PROJECTS_EVENT.drawnEnd - PROJECTS_EVENT.start) * projectsEventDrawn(elapsed);
}

/**
 * The event's rail at this moment: none while it is still being drawn, then its default rhythm once the panel opens on
 * it, then the project's once the project is picked.
 */
export function projectsEventRail(elapsed: number): FilmRail | null {
  if (elapsed < B.panelOpen) return null;
  return projectsEventPicked(elapsed) ? PROJECTS_RAIL : PROJECTS_DEFAULT_RAIL;
}

/** The project picker's open pane: the groups after the project trigger is tapped, then the group's projects. */
function projectPane(elapsed: number): "groups" | "projects" | null {
  if (elapsed >= B.groupTap && elapsed < B.projectPick) return "projects";
  if (elapsed >= B.projectTap && elapsed < B.groupTap) return "groups";
  return null;
}

/**
 * The event panel at `elapsed`, with the project's event name, which fills the empty title as the project is picked,
 * and `playlist`'s icon and localized name for each phase's project playlist.
 */
export function projectsEventPanelFrame(
  elapsed: number,
  eventName: string,
  playlist: (key: MusicScenePlaylist) => { icon: string; name: string },
): WelcomeEventPanelFrame {
  const picked = projectsEventPicked(elapsed);
  const opened = easeInOutCubic(filmSpan(elapsed, B.musicTap, B.musicTap + SECTION_OPEN_MS));
  return {
    sheetRise: 1 - (1 - filmSpan(elapsed, B.panelOpen, B.panelOpen + FILM_SHEET_RISE_MS)) ** 3,
    sheetExpand: easeInOutCubic(filmSpan(elapsed, B.sheetDragStart, B.sheetDragEnd)),
    pane: projectPane(elapsed),
    picked,
    typed: picked ? eventName : "",
    typing: false,
    caretOn: false,
    touches: {
      project: filmTap(elapsed, B.projectTap),
      group: filmTap(elapsed, B.groupTap),
      projectPick: filmTap(elapsed, B.projectPick),
      title: 0,
      sheetDrag: filmPress(elapsed, B.sheetDragStart - DRAG_LAND_MS, B.sheetDragEnd),
      music: filmTap(elapsed, B.musicTap),
      save: filmTap(elapsed, B.eventSaveTap),
    },
    preset: picked ? PROJECTS_PRESET : FILM_DEFAULT_PRESET,
    music: {
      enabled: picked,
      expanded: opened,
      rows: PROJECTS_PHASES.map((phase) => {
        const { icon, name } = playlist(PROJECTS_PLAYLISTS[phase]);
        return { phase, playlistIcon: icon, playlistName: name, fill: picked ? 1 : 0, touch: 0 };
      }),
      picker: null,
    },
    scroll: opened,
  };
}

/** The scene's touches, each from 0 to 1 as it shows. */
export interface ProjectsSceneTouches {
  settings: number;
  name: number;
  color: number;
  colorOption: number;
  duration: number;
  durationOption: number;
  pomodoro: number;
  pomodoroOption: number;
  playlist: Record<ProjectsScenePhase, number>;
  playlistOption: Record<ProjectsScenePhase, number>;
  save: number;
  close: number;
  calendarTab: number;
}

/** The scene's touches at `elapsed`. */
export function projectsTouches(elapsed: number): ProjectsSceneTouches {
  const byPhase = (beat: "tap" | "pick") =>
    Object.fromEntries(PROJECTS_PHASES.map((phase) => [phase, filmTap(elapsed, PICKER_TAPS[phase][beat])])) as Record<ProjectsScenePhase, number>;
  return {
    settings: filmTap(elapsed, B.settingsTap),
    name: filmTap(elapsed, B.nameTap),
    color: filmTap(elapsed, B.colorTap),
    colorOption: filmTap(elapsed, B.colorPick),
    duration: filmTap(elapsed, B.durationTap),
    durationOption: filmTap(elapsed, B.durationPick),
    pomodoro: filmTap(elapsed, B.pomodoroTap),
    pomodoroOption: filmTap(elapsed, B.pomodoroPick),
    playlist: byPhase("tap"),
    playlistOption: byPhase("pick"),
    save: filmTap(elapsed, B.settingsSaveTap),
    close: filmTap(elapsed, B.settingsCloseTap),
    calendarTab: filmTap(elapsed, B.calendarTap),
  };
}

/** How long before the drag starts the press that draws the event comes down, for a finger or the mouse button. */
export const PROJECTS_CREATE_PRESS_MS = 140;

/** The name the desktop pointer finds the Calendar's tab by. */
export const PROJECTS_CALENDAR_TAB_TARGET = "projects-calendar-tab";

/** Where the desktop pointer rests on the list as the welcome scenes on Projects and on tasks start. */
export const PROJECTS_CURSOR_REST_ANCHOR = [0.6, 0.3] as const;

/** The desktop pointer opening `picker`'s control once its scroll brings it into view, then picking its option. */
function pickerClicks(picker: ProjectsScenePicker): WelcomeCursorStop[] {
  const scroll = SETTINGS_SCROLLS.find((entry) => entry.target === picker);
  const after = scroll ? scroll.end - WELCOME_CURSOR_SCROLL_OVERLAP_MS : undefined;
  return [cursorClick(`projects-${picker}`, PICKER_TAPS[picker].tap, { after }), cursorClick(`projects-${picker}-option`, PICKER_TAPS[picker].pick)];
}

/**
 * The desktop pointer's path through the scene: it opens the settings and moves into them before they scroll, clicks
 * into the event name, opens each picker and picks its option, saves and closes the settings, clicks the Calendar's tab, drags the event out
 * in the Calendar, picks Novel in its panel, hovering Personal to show its projects as the desktop picker does, opens its
 * Music section, and saves it.
 */
export const PROJECTS_CURSOR: WelcomeCursorScript = [
  cursorRest("projects-list", 0, { anchor: PROJECTS_CURSOR_REST_ANCHOR }),
  cursorClick("projects-settings", B.settingsTap, { after: 0 }),
  // A wheel scrolls what is under the pointer, so it moves into the settings before they scroll.
  cursorRest("projects-settings-scroll", B.nameScrollStart - WELCOME_CURSOR_REST_MS, {
    anchor: [0.6, 0.5],
    busy: [{ start: B.nameScrollStart, end: B.nameScrollEnd }],
  }),
  cursorClick("projects-name", B.nameTap, { glyph: "text", after: B.nameScrollEnd - WELCOME_CURSOR_SCROLL_OVERLAP_MS }),
  ...(["color", "duration", "pomodoro", ...PROJECTS_PHASES] as const).flatMap(pickerClicks),
  cursorClick("projects-save", B.settingsSaveTap, { after: B.longBreakPick }),
  cursorClick("projects-close", B.settingsCloseTap),
  cursorClick(PROJECTS_CALENDAR_TAB_TARGET, B.calendarTap),
  {
    target: "projects-drag",
    anchor: [0.6, 1],
    depart: B.calendarIn,
    arrive: B.createStart - PROJECTS_CREATE_PRESS_MS - WELCOME_CURSOR_REST_MS,
  },
  {
    target: "projects-drag",
    anchor: [0.6, 1],
    depart: B.createStart,
    arrive: B.createEnd,
    press: { start: B.createStart - PROJECTS_CREATE_PRESS_MS, end: B.createEnd },
    follow: true,
  },
  cursorClick("panel-project", B.projectTap, { after: B.panelOpen }),
  cursorRest("panel-group", B.groupTap),
  cursorClick("panel-project-pick", B.projectPick),
  cursorClick("panel-music", B.musicTap, { anchor: [0.25, 0.5] }),
  cursorClick("panel-save", B.eventSaveTap),
];
