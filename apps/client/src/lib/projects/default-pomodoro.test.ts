import { describe, expect, it } from "vitest";
import {
  projectDefaultIdleTimeoutMinutes,
  projectDefaultPomodoroConfig,
  projectPomodoroSummaryLabel,
} from "./default-pomodoro";

describe("project default pomodoro helpers", () => {
  it("formats compact rhythm summaries", () => {
    expect(projectPomodoroSummaryLabel({
      focusDurationMinutes: 40,
      shortBreakMinutes: 5,
      longBreakMinutes: 10,
      longBreakAfterFocusCount: 4,
    })).toBe("F 40 / SB 05 / LB 10 / C 4");
  });

  it("returns no config when the project default is none", () => {
    expect(projectDefaultPomodoroConfig({
      defaultPomodoroMode: "none",
      defaultPomodoroPresetKey: "adaptive",
    })).toBeUndefined();
  });

  it("builds preset configs from project defaults", () => {
    expect(projectDefaultPomodoroConfig({
      defaultPomodoroMode: "preset",
      defaultPomodoroPresetKey: "creative",
    }, 5)).toEqual({
      rhythm: {
        kind: "count",
        focusDurationMinutes: 25,
        shortBreakMinutes: 5,
        longBreakMinutes: 15,
        longBreakAfterFocusCount: 4,
      },
      rhythmSource: "preset",
      presetKey: "creative",
      idleTimeoutMinutes: 5,
    });
  });

  it("builds custom count configs from project defaults", () => {
    expect(projectDefaultPomodoroConfig({
      defaultPomodoroMode: "custom",
      defaultPomodoroFocusMinutes: 45,
      defaultPomodoroShortBreakMinutes: 8,
      defaultPomodoroLongBreakMinutes: 20,
      defaultPomodoroLongBreakAfterFocusCount: 3,
    })).toEqual({
      rhythm: {
        kind: "count",
        focusDurationMinutes: 45,
        shortBreakMinutes: 8,
        longBreakMinutes: 20,
        longBreakAfterFocusCount: 3,
      },
      rhythmSource: "custom",
      presetKey: null,
      idleTimeoutMinutes: null,
    });
  });

  it("resolves global and custom idle defaults", () => {
    expect(projectDefaultIdleTimeoutMinutes({
      defaultIdleSettingsSource: "global",
      defaultIdlePauseEnabled: false,
      defaultIdleThresholdMinutes: 15,
    }, {
      idlePauseEnabled: true,
      idleThresholdMinutes: 5,
    })).toBe(5);

    expect(projectDefaultIdleTimeoutMinutes({
      defaultIdleSettingsSource: "custom",
      defaultIdlePauseEnabled: true,
      defaultIdleThresholdMinutes: 15,
    }, {
      idlePauseEnabled: false,
      idleThresholdMinutes: 5,
    })).toBe(15);

    expect(projectDefaultIdleTimeoutMinutes({
      defaultIdleSettingsSource: "custom",
      defaultIdlePauseEnabled: false,
      defaultIdleThresholdMinutes: 15,
    }, {
      idlePauseEnabled: true,
      idleThresholdMinutes: 5,
    })).toBeNull();
  });
});
