import { describe, expect, it } from "vitest";
import { FILM_TAP_MS } from "./welcome-film";
import {
  PROJECTS_BEATS,
  PROJECTS_DEFAULT_MINUTES,
  PROJECTS_EVENTS,
  PROJECTS_PRESET,
  PROJECTS_RAIL,
  PROJECTS_SCHEDULE_START,
  PROJECTS_SELECTED_ROWS,
  PROJECTS_STILL,
  PROJECTS_TASKS,
  projectsCalendarShown,
  projectsClock,
  projectsDefaultMinutes,
  projectsDefaultPreset,
  projectsEventIn,
  projectsEventRail,
  projectsEventsShown,
  projectsFocusing,
  projectsListShown,
  projectsMinutesBump,
  projectsPhaseSeconds,
  projectsRailRecorded,
  projectsRowCheckboxShown,
  projectsRowChecked,
  projectsRowTouch,
  projectsScheduleOpen,
  projectsScheduleTouches,
  projectsSelectedCount,
  projectsSettingsDirty,
  projectsSettingsOpen,
  projectsSettingsPicker,
  projectsSettingsScroll,
  projectsSettingsShown,
  projectsSettingsTouches,
} from "./welcome-projects-scene";

const B = PROJECTS_BEATS;

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(B.end / step) + 1 }, (_, index) => index * step);
}

describe("welcome projects scene", () => {
  it("keeps its beats in order", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
  });

  it("keeps every fraction between 0 and 1", () => {
    for (const elapsed of moments()) {
      const settings = projectsSettingsTouches(elapsed);
      const scroll = projectsSettingsScroll(elapsed);
      const schedule = projectsScheduleTouches(elapsed);
      const values = [
        projectsListShown(elapsed),
        projectsCalendarShown(elapsed),
        projectsSettingsShown(elapsed),
        scroll.defaults,
        scroll.music,
        ...Object.values(settings),
        schedule.schedule,
        schedule.submit,
        projectsMinutesBump(elapsed),
        ...PROJECTS_TASKS.map((_, row) => projectsRowTouch(elapsed, row)),
        ...PROJECTS_EVENTS.map((_, index) => projectsEventIn(elapsed, index)),
      ];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("schedules one event per checked task, back to back for the project's duration", () => {
    expect(PROJECTS_EVENTS.map((event) => event.row)).toEqual([...PROJECTS_SELECTED_ROWS]);
    expect(PROJECTS_EVENTS[0].start).toBe(PROJECTS_SCHEDULE_START);
    for (const [index, event] of PROJECTS_EVENTS.entries()) {
      expect(event.end - event.start).toBe(PROJECTS_DEFAULT_MINUTES);
      if (index > 0) expect(event.start).toBe(PROJECTS_EVENTS[index - 1].end);
    }
    for (const row of PROJECTS_SELECTED_ROWS) expect(PROJECTS_TASKS[row]).toBeDefined();
  });

  it("opens the settings with their tap and closes them with Save", () => {
    expect(projectsSettingsShown(B.settingsTap - 1)).toBe(0);
    expect(projectsSettingsOpen(B.settingsTap - 1)).toBe(false);
    expect(projectsSettingsShown(B.durationTap)).toBe(1);
    expect(projectsSettingsOpen(B.durationTap)).toBe(true);
    expect(projectsSettingsOpen(B.saveTap)).toBe(false);
    expect(projectsSettingsShown(B.selectFirstTap)).toBe(0);
  });

  it("scrolls to Event defaults before the first picker opens, and to the playlists before Save", () => {
    expect(projectsSettingsScroll(B.settingsTap)).toEqual({ defaults: 0, music: 0 });
    expect(projectsSettingsScroll(B.durationTap)).toEqual({ defaults: 1, music: 0 });
    expect(projectsSettingsScroll(B.pomodoroPick).music).toBe(0);
    expect(projectsSettingsScroll(B.saveTap)).toEqual({ defaults: 1, music: 1 });
  });

  it("opens one picker at a time and sets each value as its option is picked", () => {
    expect(projectsSettingsPicker(B.durationTap - 1)).toBeNull();
    expect(projectsSettingsPicker(B.durationTap)).toBe("duration");
    expect(projectsDefaultMinutes(B.durationPick - 1)).toBeNull();
    expect(projectsSettingsPicker(B.durationPick)).toBeNull();
    expect(projectsDefaultMinutes(B.durationPick)).toBe(PROJECTS_DEFAULT_MINUTES);
    expect(projectsSettingsPicker(B.pomodoroTap)).toBe("pomodoro");
    expect(projectsDefaultPreset(B.pomodoroPick - 1)).toBeNull();
    expect(projectsSettingsPicker(B.pomodoroPick)).toBeNull();
    expect(projectsDefaultPreset(B.pomodoroPick)).toBe(PROJECTS_PRESET);
  });

  it("keeps the settings dirty from the first change until they are saved", () => {
    expect(projectsSettingsDirty(B.durationPick - 1)).toBe(false);
    expect(projectsSettingsDirty(B.durationPick)).toBe(true);
    expect(projectsSettingsDirty(B.saveTap - 1)).toBe(true);
    expect(projectsSettingsDirty(B.saveTap)).toBe(false);
    expect(projectsDefaultMinutes(B.end)).toBe(PROJECTS_DEFAULT_MINUTES);
    expect(projectsDefaultPreset(B.end)).toBe(PROJECTS_PRESET);
  });

  it("checks the selected tasks one per tap, showing each checkbox under the pointer first", () => {
    const taps = [B.selectFirstTap, B.selectSecondTap, B.selectThirdTap];
    for (const [index, row] of PROJECTS_SELECTED_ROWS.entries()) {
      expect(projectsRowChecked(taps[index] - 1, row)).toBe(false);
      expect(projectsRowCheckboxShown(taps[index] - 1, row)).toBe(true);
      expect(projectsRowChecked(taps[index], row)).toBe(true);
      expect(projectsRowTouch(taps[index] - FILM_TAP_MS / 2, row)).toBeGreaterThan(0);
      expect(projectsSelectedCount(taps[index])).toBe(index + 1);
    }
    const unselected = PROJECTS_TASKS.map((_, row) => row).filter((row) => !PROJECTS_SELECTED_ROWS.includes(row));
    for (const row of unselected) {
      expect(projectsRowChecked(B.end, row)).toBe(false);
      expect(projectsRowCheckboxShown(B.end, row)).toBe(false);
      expect(projectsRowTouch(B.selectSecondTap, row)).toBe(0);
    }
  });

  it("opens the Schedule form after the last check and closes it as the list gives way", () => {
    expect(projectsScheduleOpen(B.selectThirdTap)).toBe(false);
    expect(projectsScheduleOpen(B.scheduleTap)).toBe(true);
    expect(projectsScheduleOpen(B.scheduleSubmitTap)).toBe(true);
    expect(projectsScheduleOpen(B.listOut)).toBe(false);
    expect(projectsMinutesBump(B.minutesBumpStart)).toBe(0);
    expect(projectsMinutesBump((B.minutesBumpStart + B.minutesBumpEnd) / 2)).toBeCloseTo(1, 9);
    expect(projectsMinutesBump(B.minutesBumpEnd)).toBe(0);
  });

  it("hands the list over to the Calendar before the events land", () => {
    expect(projectsListShown(B.listOut)).toBe(1);
    expect(projectsListShown(B.calendarIn)).toBe(0);
    expect(projectsCalendarShown(B.calendarIn)).toBe(1);
    expect(projectsEventsShown(B.firstEventIn - 1)).toBe(0);
    expect(projectsEventIn(B.firstEventIn - 1, 0)).toBe(0);
  });

  it("lands the events one after another and grows their shared rail with each", () => {
    const landings = [B.firstEventIn, B.secondEventIn, B.thirdEventIn];
    expect(projectsEventRail(B.firstEventIn - 1)).toBeNull();
    for (const [index, landing] of landings.entries()) {
      expect(projectsEventsShown(landing)).toBe(index + 1);
      const rail = projectsEventRail(landing);
      expect(rail?.start).toBe(PROJECTS_EVENTS[0].start);
      expect(rail?.end).toBe(PROJECTS_EVENTS[index].end);
    }
    expect(projectsEventIn(B.focusStart, 2)).toBe(1);
    expect(projectsEventRail(B.end)).toEqual(PROJECTS_RAIL);
    expect(PROJECTS_RAIL.focus.length).toBeGreaterThan(1);
    expect(PROJECTS_RAIL.breaks.length).toBeGreaterThan(0);
  });

  it("walks the clock into the first event and starts its Focus there", () => {
    for (const elapsed of moments()) {
      if (elapsed === 0) continue;
      expect(projectsClock(elapsed)).toBeGreaterThanOrEqual(projectsClock(elapsed - 10));
    }
    expect(projectsFocusing(B.focusStart - 1)).toBe(false);
    expect(projectsClock(B.focusStart)).toBe(PROJECTS_SCHEDULE_START);
    expect(projectsFocusing(B.focusStart)).toBe(true);
    expect(projectsClock(B.end)).toBeLessThan(PROJECTS_RAIL.focus[0].end);
  });

  it("counts the first focus phase down from its full length", () => {
    expect(projectsPhaseSeconds(B.focusStart - 1)).toEqual({ remaining: 0, total: 0 });
    const span = PROJECTS_RAIL.focus[0];
    const total = Math.round((span.end - span.start) * 60);
    expect(projectsPhaseSeconds(B.focusStart)).toEqual({ remaining: total, total });
    const later = projectsPhaseSeconds(B.end);
    expect(later.total).toBe(total);
    expect(later.remaining).toBeLessThan(total);
    expect(later.remaining).toBeGreaterThan(0);
  });

  it("records focus only up to the current time once the Focus runs", () => {
    expect(projectsRailRecorded(B.focusStart - 1)).toEqual([]);
    expect(projectsRailRecorded(B.focusStart)).toEqual([]);
    const recorded = projectsRailRecorded(B.end);
    expect(recorded).toEqual([{ start: PROJECTS_RAIL.focus[0].start, end: projectsClock(B.end) }]);
  });

  it("shows the whole result in its still frame", () => {
    expect(PROJECTS_STILL).toBeLessThan(B.end);
    expect(projectsEventsShown(PROJECTS_STILL)).toBe(PROJECTS_EVENTS.length);
    expect(projectsFocusing(PROJECTS_STILL)).toBe(true);
    expect(projectsCalendarShown(PROJECTS_STILL)).toBe(1);
    expect(projectsSettingsShown(PROJECTS_STILL)).toBe(0);
  });
});
