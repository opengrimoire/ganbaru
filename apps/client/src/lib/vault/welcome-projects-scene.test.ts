import { describe, expect, it } from "vitest";
import { welcomeCursorHovered, welcomeCursorScriptOrdered } from "./welcome-cursor";
import { FILM_TAP_MS } from "./welcome-film";
import {
  PROJECTS_BEATS,
  PROJECTS_COLOR,
  PROJECTS_CREATE_PRESS_MS,
  PROJECTS_CURSOR,
  PROJECTS_DEFAULT_MINUTES,
  PROJECTS_DEFAULT_RAIL,
  PROJECTS_EVENT,
  PROJECTS_PHASES,
  PROJECTS_PLAYLISTS,
  PROJECTS_PRESET,
  PROJECTS_PRIORITY_COLORS,
  PROJECTS_PRIORITY_ORDER,
  PROJECTS_RAIL,
  PROJECTS_STATUS_COLORS,
  PROJECTS_STATUS_ORDER,
  PROJECTS_STILL,
  projectsCalendarShown,
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
  projectsListShown,
  projectsPanelOpen,
  projectsOptionHovered,
  projectsPlaylist,
  projectsSettingsDirty,
  projectsSettingsOpen,
  projectsSettingsPicker,
  projectsSettingsScroll,
  projectsSettingsShown,
  projectsTouches,
  type ProjectsScenePhase,
  type ProjectsSettingsTarget,
} from "./welcome-projects-scene";

const B = PROJECTS_BEATS;
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
function touchValues(elapsed: number): number[] {
  const { playlist, playlistOption, ...rest } = projectsTouches(elapsed);
  return [...Object.values(rest), ...Object.values(playlist), ...Object.values(playlistOption)];
}

describe("welcome projects scene", () => {
  it("keeps its beats in order", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
  });

  it("keeps every fraction between 0 and 1", () => {
    for (const elapsed of moments()) {
      const values = [projectsSettingsScroll(elapsed).progress, projectsSettingsShown(elapsed), ...touchValues(elapsed)];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("lists every default status and priority with the colors a new project seeds", () => {
    expect(PROJECTS_STATUS_ORDER).toHaveLength(Object.keys(PROJECTS_STATUS_COLORS).length);
    expect(PROJECTS_PRIORITY_ORDER).toHaveLength(Object.keys(PROJECTS_PRIORITY_COLORS).length);
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

  it("closes the color palette as its swatch is picked", () => {
    expect(projectsSettingsPicker(B.colorTap)).toBe("color");
    expect(projectsSettingsPicker(B.colorPick)).toBeNull();
    expect(projectsDefaultColor(B.colorPick - 1)).toBeNull();
    expect(projectsDefaultColor(B.colorPick)).toBe(PROJECTS_COLOR);
  });

  it("opens one picker at a time and sets each value as its option is picked", () => {
    expect(projectsSettingsPicker(B.durationTap - 1)).toBeNull();
    expect(projectsSettingsPicker(B.durationTap)).toBe("duration");
    expect(projectsSettingsPicker(B.durationPick)).toBeNull();
    expect(projectsSettingsPicker(B.pomodoroTap)).toBe("pomodoro");
    expect(projectsSettingsPicker(B.pomodoroPick)).toBeNull();
    expect(projectsDefaultMinutes(B.durationPick - 1)).toBeNull();
    expect(projectsDefaultMinutes(B.durationPick)).toBe(PROJECTS_DEFAULT_MINUTES);
    expect(projectsDefaultPreset(B.pomodoroPick - 1)).toBeNull();
    expect(projectsDefaultPreset(B.pomodoroPick)).toBe(PROJECTS_PRESET);
  });

  it("picks a playlist for each phase in turn", () => {
    for (const phase of PROJECTS_PHASES) {
      const { tap, pick } = PHASE_TAPS[phase];
      expect(projectsPlaylist(tap, phase)).toBeNull();
      expect(projectsSettingsPicker(tap)).toBe(phase);
      expect(projectsOptionHovered(pick - 1, phase)).toBe(true);
      expect(projectsTouches(pick - FILM_TAP_MS / 2).playlistOption[phase]).toBeGreaterThan(0);
      expect(projectsSettingsPicker(pick)).toBeNull();
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

  it("clicks with the desktop pointer on the beats the touches land on, resting on each control through its press", () => {
    expect(welcomeCursorScriptOrdered(PROJECTS_CURSOR)).toBe(true);
    const clicks = PROJECTS_CURSOR.filter((stop) => stop.press && !stop.follow);
    expect(clicks.map((stop) => [stop.target, stop.press?.end])).toEqual([
      ["projects-settings", B.settingsTap],
      ["projects-name", B.nameTap],
      ["projects-color", B.colorTap],
      ["projects-color-option", B.colorPick],
      ["projects-duration", B.durationTap],
      ["projects-duration-option", B.durationPick],
      ["projects-pomodoro", B.pomodoroTap],
      ["projects-pomodoro-option", B.pomodoroPick],
      ...PROJECTS_PHASES.flatMap((phase) => [
        [`projects-${phase}`, PHASE_TAPS[phase].tap],
        [`projects-${phase}-option`, PHASE_TAPS[phase].pick],
      ]),
      ["projects-save", B.settingsSaveTap],
      ["projects-close", B.settingsCloseTap],
      ["panel-project", B.projectTap],
      ["panel-project-pick", B.projectPick],
      ["panel-music", B.musicTap],
      ["panel-save", B.eventSaveTap],
    ]);
    for (const stop of clicks) {
      const press = stop.press;
      if (!press) continue;
      expect(press.start).toBe(press.end - FILM_TAP_MS);
      for (let elapsed = press.start; elapsed <= press.end; elapsed += 10) expect(welcomeCursorHovered(PROJECTS_CURSOR, elapsed)).toBe(stop.target);
    }
  });

  it("holds the button down while it drags the event out, following its growing edge", () => {
    const drag = PROJECTS_CURSOR.find((stop) => stop.follow);
    expect(drag?.target).toBe("projects-drag");
    expect(drag?.press).toEqual({ start: B.createStart - PROJECTS_CREATE_PRESS_MS, end: B.createEnd });
    expect(drag?.depart).toBe(B.createStart);
    expect(drag?.arrive).toBe(B.createEnd);
    expect(welcomeCursorHovered(PROJECTS_CURSOR, B.createStart - PROJECTS_CREATE_PRESS_MS)).toBe("projects-drag");
  });

  it("hovers Personal to show its projects, as the desktop picker does, without clicking it", () => {
    const group = PROJECTS_CURSOR.find((stop) => stop.target === "panel-group");
    expect(group?.press).toBeUndefined();
    expect(group?.arrive).toBe(B.groupTap);
    expect(welcomeCursorHovered(PROJECTS_CURSOR, B.groupTap)).toBe("panel-group");
  });

  it("moves into the settings before they scroll, since a wheel scrolls what is under the pointer", () => {
    expect(welcomeCursorHovered(PROJECTS_CURSOR, B.nameScrollStart)).toBe("projects-settings-scroll");
    expect(PROJECTS_CURSOR.find((stop) => stop.target === "projects-settings-scroll")?.depart).toBeGreaterThanOrEqual(B.settingsTap);
  });

  it("reaches the event name under a text beam, and each picker only once the settings have scrolled to it", () => {
    expect(PROJECTS_CURSOR.find((stop) => stop.target === "projects-name")?.glyph).toBe("text");
    expect(PROJECTS_CURSOR.find((stop) => stop.target === "projects-name")?.depart).toBeGreaterThan(B.nameScrollStart);
    expect(PROJECTS_CURSOR.find((stop) => stop.target === "projects-color")?.depart).toBeGreaterThan(B.colorScrollStart);
    for (const phase of PROJECTS_PHASES) {
      expect(PROJECTS_CURSOR.find((stop) => stop.target === `projects-${phase}`)?.depart).toBeGreaterThan(B.focusScrollStart);
    }
  });
});
