import {
  createCustomCountPomodoroConfig,
  createPresetPomodoroConfig,
  type PomodoroConfig,
  type PomodoroPresetKey,
} from "$lib/pomodoro/rhythm";
import {
  DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
  DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
  FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS,
} from "$lib/stores/preference-options";

export type ProjectDefaultPomodoroMode = "none" | "preset" | "custom";
export type ProjectDefaultIdleSettingsSource = "global" | "custom";

export interface ProjectDefaultPomodoroCustom {
  focusDurationMinutes: number;
  shortBreakMinutes: number;
  longBreakMinutes: number;
  longBreakAfterFocusCount: number;
}

export interface ProjectDefaultPomodoroConfigInput {
  defaultPomodoroMode: ProjectDefaultPomodoroMode;
  defaultPomodoroPresetKey?: PomodoroPresetKey;
  defaultPomodoroFocusMinutes?: number;
  defaultPomodoroShortBreakMinutes?: number;
  defaultPomodoroLongBreakMinutes?: number;
  defaultPomodoroLongBreakAfterFocusCount?: number;
}

export interface ProjectDefaultIdleConfigInput {
  defaultIdleSettingsSource?: ProjectDefaultIdleSettingsSource;
  defaultIdlePauseEnabled?: boolean;
  defaultIdleThresholdMinutes?: number;
}

export interface ProjectGlobalIdleDefaults {
  idlePauseEnabled: boolean;
  idleThresholdMinutes: number;
}

export const PROJECT_POMODORO_PRESET_ORDER: PomodoroPresetKey[] = [
  "adaptive",
  "creative",
  "balanced",
  "deep",
  "extended",
];

export const PROJECT_DEFAULT_CUSTOM_POMODORO: ProjectDefaultPomodoroCustom = {
  focusDurationMinutes: 40,
  shortBreakMinutes: 10,
  longBreakMinutes: 10,
  longBreakAfterFocusCount: 4,
};

/**
 * Formats the compact Pomodoro rhythm summary used in settings controls.
 */
export function projectPomodoroSummaryLabel(rhythm: ProjectDefaultPomodoroCustom): string {
  return `F ${formatDurationSlot(rhythm.focusDurationMinutes)} / SB ${formatDurationSlot(rhythm.shortBreakMinutes)} / LB ${formatDurationSlot(rhythm.longBreakMinutes)} / C ${rhythm.longBreakAfterFocusCount}`;
}

/**
 * Returns the custom count rhythm represented by flat project default fields.
 */
export function projectCustomPomodoroFromDefaults(
  input: ProjectDefaultPomodoroConfigInput,
): ProjectDefaultPomodoroCustom {
  return {
    focusDurationMinutes: input.defaultPomodoroFocusMinutes ?? PROJECT_DEFAULT_CUSTOM_POMODORO.focusDurationMinutes,
    shortBreakMinutes: input.defaultPomodoroShortBreakMinutes ?? PROJECT_DEFAULT_CUSTOM_POMODORO.shortBreakMinutes,
    longBreakMinutes: input.defaultPomodoroLongBreakMinutes ?? PROJECT_DEFAULT_CUSTOM_POMODORO.longBreakMinutes,
    longBreakAfterFocusCount: input.defaultPomodoroLongBreakAfterFocusCount
      ?? PROJECT_DEFAULT_CUSTOM_POMODORO.longBreakAfterFocusCount,
  };
}

/**
 * Resolves the idle timeout that should be copied into a project Pomodoro event.
 */
export function projectDefaultIdleTimeoutMinutes(
  input: ProjectDefaultIdleConfigInput,
  globalDefaults: ProjectGlobalIdleDefaults,
): number | null {
  const useProjectIdleSettings = input.defaultIdleSettingsSource === "custom";
  const idlePauseEnabled = useProjectIdleSettings
    ? input.defaultIdlePauseEnabled ?? DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE
    : globalDefaults.idlePauseEnabled;
  if (!idlePauseEnabled) return null;
  return useProjectIdleSettings
    ? normalizeIdleThresholdMinutes(input.defaultIdleThresholdMinutes)
    : normalizeIdleThresholdMinutes(globalDefaults.idleThresholdMinutes);
}

/**
 * Builds the Pomodoro config to copy into a new project event.
 */
export function projectDefaultPomodoroConfig(
  input: ProjectDefaultPomodoroConfigInput,
  idleTimeoutMinutes: number | null = null,
): PomodoroConfig | undefined {
  if (input.defaultPomodoroMode === "none") return undefined;
  if (input.defaultPomodoroMode === "preset") {
    return input.defaultPomodoroPresetKey
      ? createPresetPomodoroConfig(input.defaultPomodoroPresetKey, idleTimeoutMinutes)
      : undefined;
  }
  const custom = projectCustomPomodoroFromDefaults(input);
  return createCustomCountPomodoroConfig(custom, idleTimeoutMinutes);
}

function formatDurationSlot(value: number): string {
  return String(value).padStart(2, "0");
}

function normalizeIdleThresholdMinutes(value: number | undefined): number {
  if (value === undefined) return DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES;
  return FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS.some((option) => option === value)
    ? value
    : DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES;
}
