import { describe, expect, it } from "vitest";
import { FILM_TAP_MS } from "./welcome-film";
import {
  PROJECTS_BEATS,
  PROJECTS_COLOR,
  PROJECTS_DEFAULT_MINUTES,
  PROJECTS_NEW_PRIORITY,
  PROJECTS_NEW_STATUS,
  PROJECTS_NEW_TASK,
  PROJECTS_PHASES,
  PROJECTS_PLAYLISTS,
  PROJECTS_PRESET,
  PROJECTS_PRIORITY_COLORS,
  PROJECTS_PRIORITY_ORDER,
  PROJECTS_STATUS_COLORS,
  PROJECTS_STATUS_ORDER,
  PROJECTS_STILL,
  projectsAddActive,
  projectsCellHighlighted,
  projectsCellMenu,
  projectsDefaultColor,
  projectsDefaultMinutes,
  projectsDefaultPreset,
  projectsEventName,
  projectsEventNameCaretOn,
  projectsEventNameFocused,
  projectsListScroll,
  projectsNewPriority,
  projectsNewStatus,
  projectsOptionHovered,
  projectsPlaylist,
  projectsSettingsDirty,
  projectsSettingsOpen,
  projectsSettingsPicker,
  projectsSettingsScroll,
  projectsSettingsShown,
  projectsTaskAdded,
  projectsTaskCaretOn,
  projectsTaskDraft,
  projectsTouches,
  type ProjectsScenePhase,
} from "./welcome-projects-scene";

const B = PROJECTS_BEATS;
const TITLE = "Draft chapter 2";
const NAME = "Writing session";
const PHASE_TAPS: Readonly<Record<ProjectsScenePhase, { tap: number; pick: number }>> = {
  focus: { tap: B.focusTap, pick: B.focusPick },
  "short-break": { tap: B.shortBreakTap, pick: B.shortBreakPick },
  "long-break": { tap: B.longBreakTap, pick: B.longBreakPick },
};

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(B.end / step) + 1 }, (_, index) => index * step);
}

/** Every touch of a frame as one flat list. */
function touchValues(elapsed: number, colorClosesOnPick: boolean): number[] {
  const { playlist, playlistOption, ...rest } = projectsTouches(elapsed, colorClosesOnPick);
  return [...Object.values(rest), ...Object.values(playlist), ...Object.values(playlistOption)];
}

describe("welcome projects scene", () => {
  it("keeps its beats in order", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
  });

  it("keeps every fraction between 0 and 1", () => {
    for (const elapsed of moments()) {
      const list = projectsListScroll(elapsed);
      const settings = projectsSettingsScroll(elapsed);
      const values = [
        list.status,
        list.priority,
        settings.defaults,
        settings.music,
        projectsSettingsShown(elapsed),
        ...touchValues(elapsed, false),
        ...touchValues(elapsed, true),
      ];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("lists every default status and priority with the colors a new project seeds", () => {
    expect(PROJECTS_STATUS_ORDER).toHaveLength(Object.keys(PROJECTS_STATUS_COLORS).length);
    expect(PROJECTS_PRIORITY_ORDER).toHaveLength(Object.keys(PROJECTS_PRIORITY_COLORS).length);
    expect(PROJECTS_STATUS_ORDER).toContain(PROJECTS_NEW_STATUS);
    expect(PROJECTS_PRIORITY_ORDER).toContain(PROJECTS_NEW_PRIORITY);
  });

  it("types the task into the add row, adds it with Enter, and keeps the field open and empty", () => {
    expect(projectsAddActive(B.addTap - 1)).toBe(false);
    expect(projectsAddActive(B.addTap)).toBe(true);
    expect(projectsTaskDraft(B.taskTypeStart, TITLE)).toBe("");
    expect(projectsTaskDraft((B.taskTypeStart + B.taskTypeEnd) / 2, TITLE).length).toBeGreaterThan(0);
    expect(projectsTaskDraft(B.taskTypeEnd, TITLE)).toBe(TITLE);
    expect(projectsTaskAdded(B.taskSubmit - 1)).toBe(false);
    expect(projectsTaskDraft(B.taskSubmit - 1, TITLE)).toBe(TITLE);
    expect(projectsTaskAdded(B.taskSubmit)).toBe(true);
    expect(projectsTaskDraft(B.taskSubmit, TITLE)).toBe("");
    expect(projectsAddActive(B.taskSubmit)).toBe(true);
    expect(projectsTaskCaretOn((B.taskTypeStart + B.taskTypeEnd) / 2)).toBe(true);
  });

  it("closes the add field with the tap on the new task's status", () => {
    expect(projectsAddActive(B.statusTap - 1)).toBe(true);
    expect(projectsAddActive(B.statusTap)).toBe(false);
    expect(projectsTaskCaretOn(B.statusTap)).toBe(false);
  });

  it("adds the task as a new task starts, then sets its status and its priority from their menus", () => {
    expect(projectsNewStatus(B.statusPick - 1)).toBe(PROJECTS_NEW_TASK.status);
    expect(projectsNewStatus(B.statusPick)).toBe(PROJECTS_NEW_STATUS);
    expect(projectsNewPriority(B.priorityPick - 1)).toBe(PROJECTS_NEW_TASK.priority);
    expect(projectsNewPriority(B.priorityPick)).toBe(PROJECTS_NEW_PRIORITY);
    expect(PROJECTS_NEW_TASK.status).not.toBe(PROJECTS_NEW_STATUS);
    expect(PROJECTS_NEW_TASK.priority).not.toBe(PROJECTS_NEW_PRIORITY);
  });

  it("opens one cell menu at a time, highlighting its cell from the hover before the tap", () => {
    expect(projectsCellMenu(B.statusTap - 1)).toBeNull();
    expect(projectsCellHighlighted(B.statusTap - 1, "status")).toBe(true);
    expect(projectsCellMenu(B.statusTap)).toBe("status");
    expect(projectsOptionHovered(B.statusPick - 1, "status")).toBe(true);
    expect(projectsCellMenu(B.statusPick)).toBeNull();
    expect(projectsCellHighlighted(B.statusPick, "status")).toBe(false);
    expect(projectsCellHighlighted(B.statusTap, "priority")).toBe(false);
    expect(projectsCellMenu(B.priorityTap)).toBe("priority");
    expect(projectsCellMenu(B.priorityPick)).toBeNull();
    expect(projectsTouches(B.statusTap - FILM_TAP_MS / 2, false).status).toBeGreaterThan(0);
  });

  it("scrolls the list to each cell before its tap", () => {
    expect(projectsListScroll(B.statusScrollStart)).toEqual({ status: 0, priority: 0 });
    expect(projectsListScroll(B.statusTap)).toEqual({ status: 1, priority: 0 });
    expect(projectsListScroll(B.priorityTap)).toEqual({ status: 1, priority: 1 });
  });

  it("opens the settings with their tap and closes them with Save", () => {
    expect(projectsSettingsShown(B.settingsTap - 1)).toBe(0);
    expect(projectsSettingsOpen(B.settingsTap - 1)).toBe(false);
    expect(projectsSettingsShown(B.nameTap)).toBe(1);
    expect(projectsSettingsOpen(B.saveTap - 1)).toBe(true);
    expect(projectsSettingsOpen(B.saveTap)).toBe(false);
    expect(projectsSettingsShown(B.end)).toBe(0);
  });

  it("scrolls to Event defaults before the name is typed, and to the playlists before their pickers open", () => {
    expect(projectsSettingsScroll(B.settingsTap)).toEqual({ defaults: 0, music: 0 });
    expect(projectsSettingsScroll(B.nameTap)).toEqual({ defaults: 1, music: 0 });
    expect(projectsSettingsScroll(B.pomodoroPick).music).toBe(0);
    expect(projectsSettingsScroll(B.focusTap)).toEqual({ defaults: 1, music: 1 });
  });

  it("types the event name into its focused field", () => {
    expect(projectsEventNameFocused(B.nameTap - 1)).toBe(false);
    expect(projectsEventNameFocused(B.nameTap)).toBe(true);
    expect(projectsEventName(B.nameTypeStart, NAME)).toBe("");
    expect(projectsEventName(B.nameTypeEnd, NAME)).toBe(NAME);
    expect(projectsEventNameCaretOn(B.nameTypeEnd - 1)).toBe(true);
    expect(projectsEventNameFocused(B.colorTap)).toBe(false);
    expect(projectsEventNameCaretOn(B.colorTap)).toBe(false);
    expect(projectsEventName(B.end, NAME)).toBe(NAME);
  });

  it("closes the color palette on the pick on a phone, and with the tap beside it on desktop", () => {
    expect(projectsSettingsPicker(B.colorTap, true)).toBe("color");
    expect(projectsSettingsPicker(B.colorPick, true)).toBeNull();
    expect(projectsSettingsPicker(B.colorPick, false)).toBe("color");
    expect(projectsSettingsPicker(B.colorDismiss, false)).toBeNull();
    expect(projectsTouches(B.colorDismiss - FILM_TAP_MS / 2, false).colorDismiss).toBeGreaterThan(0);
    expect(projectsTouches(B.colorDismiss - FILM_TAP_MS / 2, true).colorDismiss).toBe(0);
    expect(projectsDefaultColor(B.colorPick - 1)).toBeNull();
    expect(projectsDefaultColor(B.colorPick)).toBe(PROJECTS_COLOR);
  });

  it("opens one picker at a time and sets each value as its option is picked", () => {
    for (const closesOnPick of [true, false]) {
      expect(projectsSettingsPicker(B.durationTap - 1, closesOnPick)).toBeNull();
      expect(projectsSettingsPicker(B.durationTap, closesOnPick)).toBe("duration");
      expect(projectsSettingsPicker(B.durationPick, closesOnPick)).toBeNull();
      expect(projectsSettingsPicker(B.pomodoroTap, closesOnPick)).toBe("pomodoro");
      expect(projectsSettingsPicker(B.pomodoroPick, closesOnPick)).toBeNull();
    }
    expect(projectsDefaultMinutes(B.durationPick - 1)).toBeNull();
    expect(projectsDefaultMinutes(B.durationPick)).toBe(PROJECTS_DEFAULT_MINUTES);
    expect(projectsDefaultPreset(B.pomodoroPick - 1)).toBeNull();
    expect(projectsDefaultPreset(B.pomodoroPick)).toBe(PROJECTS_PRESET);
  });

  it("picks a playlist for each phase in turn", () => {
    for (const phase of PROJECTS_PHASES) {
      const { tap, pick } = PHASE_TAPS[phase];
      expect(projectsPlaylist(tap, phase)).toBeNull();
      expect(projectsSettingsPicker(tap, false)).toBe(phase);
      expect(projectsOptionHovered(pick - 1, phase)).toBe(true);
      expect(projectsTouches(pick - FILM_TAP_MS / 2, false).playlistOption[phase]).toBeGreaterThan(0);
      expect(projectsSettingsPicker(pick, false)).toBeNull();
      expect(projectsPlaylist(pick, phase)).toBe(PROJECTS_PLAYLISTS[phase]);
    }
  });

  it("keeps the settings dirty from the first change until they are saved", () => {
    expect(projectsSettingsDirty(B.nameTap)).toBe(false);
    expect(projectsSettingsDirty(B.nameTypeStart)).toBe(true);
    expect(projectsSettingsDirty(B.saveTap - 1)).toBe(true);
    expect(projectsSettingsDirty(B.saveTap)).toBe(false);
  });

  it("shows every default set in its still frame, before Save", () => {
    expect(PROJECTS_STILL).toBeLessThan(B.saveTap - FILM_TAP_MS);
    expect(projectsSettingsShown(PROJECTS_STILL)).toBe(1);
    expect(projectsSettingsPicker(PROJECTS_STILL, false)).toBeNull();
    expect(projectsDefaultColor(PROJECTS_STILL)).toBe(PROJECTS_COLOR);
    expect(projectsDefaultMinutes(PROJECTS_STILL)).toBe(PROJECTS_DEFAULT_MINUTES);
    expect(projectsDefaultPreset(PROJECTS_STILL)).toBe(PROJECTS_PRESET);
    for (const phase of PROJECTS_PHASES) expect(projectsPlaylist(PROJECTS_STILL, phase)).toBe(PROJECTS_PLAYLISTS[phase]);
    expect(projectsNewStatus(PROJECTS_STILL)).toBe(PROJECTS_NEW_STATUS);
    expect(projectsNewPriority(PROJECTS_STILL)).toBe(PROJECTS_NEW_PRIORITY);
  });
});
