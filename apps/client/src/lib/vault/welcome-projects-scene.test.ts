import { describe, expect, it } from "vitest";
import { FILM_TAP_MS } from "./welcome-film";
import {
  PROJECTS_BEATS,
  PROJECTS_COLOR,
  PROJECTS_DEFAULT_MINUTES,
  PROJECTS_DEFAULT_RAIL,
  PROJECTS_EVENT,
  PROJECTS_NEW_PRIORITY,
  PROJECTS_NEW_STATUS,
  PROJECTS_NEW_TASK,
  PROJECTS_PHASES,
  PROJECTS_PLAYLISTS,
  PROJECTS_PRESET,
  PROJECTS_PRIORITY_COLORS,
  PROJECTS_PRIORITY_ORDER,
  PROJECTS_RAIL,
  PROJECTS_STATUS_COLORS,
  PROJECTS_STATUS_ORDER,
  PROJECTS_STILL,
  projectsAddActive,
  projectsCalendarShown,
  projectsCellHighlighted,
  projectsCellMenu,
  projectsDefaultColor,
  projectsDefaultMinutes,
  projectsDefaultPreset,
  projectsEventCreating,
  projectsEventDrawnIn,
  projectsEventMinutes,
  projectsEventName,
  projectsEventNameCaretOn,
  projectsEventNameFocused,
  projectsEventPanelFrame,
  projectsEventRail,
  projectsListScroll,
  projectsListShown,
  projectsNewPriority,
  projectsPanelOpen,
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
  type ProjectsSettingsTarget,
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
        settings.progress,
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

  it("opens the settings with their tap, keeps them open after Save, and closes them with the close button", () => {
    expect(projectsSettingsShown(B.settingsTap - 1)).toBe(0);
    expect(projectsSettingsOpen(B.settingsTap - 1)).toBe(false);
    expect(projectsSettingsShown(B.nameTap)).toBe(1);
    expect(projectsSettingsOpen(B.settingsSaveTap)).toBe(true);
    expect(projectsSettingsShown(B.settingsCloseTap - 1)).toBe(1);
    expect(projectsSettingsDirty(B.settingsCloseTap - 1)).toBe(false);
    expect(projectsSettingsOpen(B.settingsCloseTap - 1)).toBe(true);
    expect(projectsSettingsOpen(B.settingsCloseTap)).toBe(false);
    expect(projectsSettingsShown(B.end)).toBe(0);
  });

  it("scrolls each control into view before its tap, from the one before it", () => {
    expect(projectsSettingsScroll(B.settingsTap)).toEqual({ from: null, to: null, progress: 0 });
    expect(projectsSettingsScroll(B.nameTap)).toEqual({ from: null, to: "name", progress: 1 });
    const taps: readonly [ProjectsSettingsTarget, number][] = [
      ["color", B.colorTap],
      ["duration", B.durationTap],
      ["pomodoro", B.pomodoroTap],
      ["focus", B.focusTap],
      ["short-break", B.shortBreakTap],
      ["long-break", B.longBreakTap],
    ];
    let previous: ProjectsSettingsTarget = "name";
    for (const [target, tap] of taps) {
      expect(projectsSettingsScroll(tap)).toEqual({ from: previous, to: target, progress: 1 });
      previous = target;
    }
    const midColor = projectsSettingsScroll((B.colorScrollStart + B.colorScrollEnd) / 2);
    expect(midColor.progress).toBeGreaterThan(0);
    expect(midColor.progress).toBeLessThan(1);
    expect(projectsSettingsScroll(B.end)).toEqual({ from: "short-break", to: "long-break", progress: 1 });
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
    expect(projectsSettingsDirty(B.settingsSaveTap - 1)).toBe(true);
    expect(projectsSettingsDirty(B.settingsSaveTap)).toBe(false);
  });

  it("gives way to the Calendar once the settings are closed", () => {
    expect(PROJECTS_BEATS.calendarIn).toBeGreaterThan(B.settingsCloseTap);
    expect(projectsCalendarShown(B.settingsCloseTap)).toBe(0);
    expect(projectsListShown(B.settingsCloseTap)).toBe(1);
    expect(projectsCalendarShown(B.calendarIn)).toBe(1);
    expect(projectsListShown(B.calendarIn)).toBe(0);
  });

  it("drags the event out to its drawn length, then grows it to the project's default duration once Novel is picked", () => {
    const drawn = PROJECTS_EVENT.drawnEnd - PROJECTS_EVENT.start;
    expect(projectsEventDrawnIn(B.createStart - 1)).toBe(false);
    expect(projectsEventMinutes(B.createStart)).toBe(0);
    expect(projectsEventMinutes(B.createEnd)).toBe(drawn);
    expect(projectsEventMinutes(B.projectPick - 1)).toBe(drawn);
    expect(projectsEventMinutes(B.end)).toBe(PROJECTS_DEFAULT_MINUTES);
    expect(projectsEventCreating(B.eventSaveTap - 1)).toBe(true);
    expect(projectsEventCreating(B.eventSaveTap)).toBe(false);
    expect(projectsEventDrawnIn(B.end)).toBe(true);
  });

  it("plans the event's rail from the panel opening, with the project's Pomodoro once picked", () => {
    expect(projectsEventRail(B.panelOpen - 1)).toBeNull();
    expect(projectsEventRail(B.panelOpen)).toBe(PROJECTS_DEFAULT_RAIL);
    expect(projectsEventRail(B.projectPick)).toBe(PROJECTS_RAIL);
    expect(PROJECTS_RAIL.end - PROJECTS_RAIL.start).toBe(PROJECTS_DEFAULT_MINUTES);
  });

  it("opens the event panel on the drawn event, and fills it with every project default as Novel is picked", () => {
    const playlist = (key: string) => ({ icon: `icon-${key}`, name: key });
    expect(projectsPanelOpen(B.panelOpen - 1)).toBe(false);
    expect(projectsPanelOpen(B.panelOpen)).toBe(true);
    expect(projectsPanelOpen(B.eventSaveTap)).toBe(false);

    const before = projectsEventPanelFrame(B.projectPick - 1, NAME, playlist);
    expect(before.picked).toBe(false);
    expect(before.typed).toBe("");
    expect(before.preset).not.toBe(PROJECTS_PRESET);
    expect(before.music?.enabled).toBe(false);

    const after = projectsEventPanelFrame(B.projectPick, NAME, playlist);
    expect(after.picked).toBe(true);
    expect(after.typed).toBe(NAME);
    expect(after.preset).toBe(PROJECTS_PRESET);
    expect(after.music?.enabled).toBe(true);
    expect(after.music?.rows.map((row) => row.playlistName)).toEqual(PROJECTS_PHASES.map((phase) => PROJECTS_PLAYLISTS[phase]));
    expect(projectsEventPanelFrame(B.projectTap, NAME, playlist).pane).toBe("groups");
    expect(projectsEventPanelFrame(B.groupTap, NAME, playlist).pane).toBe("projects");
    expect(after.pane).toBeNull();
  });

  it("shows the event with every project default in its still frame, its Music section open, before Save", () => {
    const playlist = (key: string) => ({ icon: key, name: key });
    expect(PROJECTS_STILL).toBeLessThan(B.eventSaveTap - FILM_TAP_MS);
    expect(projectsPanelOpen(PROJECTS_STILL)).toBe(true);
    expect(projectsEventMinutes(PROJECTS_STILL)).toBe(PROJECTS_DEFAULT_MINUTES);
    const frame = projectsEventPanelFrame(PROJECTS_STILL, NAME, playlist);
    expect(frame.picked).toBe(true);
    expect(frame.music?.expanded).toBe(1);
    expect(projectsDefaultColor(PROJECTS_STILL)).toBe(PROJECTS_COLOR);
    expect(projectsDefaultPreset(PROJECTS_STILL)).toBe(PROJECTS_PRESET);
  });
});
