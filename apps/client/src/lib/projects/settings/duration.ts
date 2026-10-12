export type ProjectDurationUnit = "minutes" | "hours";
export type ProjectDefaultEventTimeMode = "timed" | "all_day";
export type ProjectDurationPresetValue =
  | "default"
  | "all_day"
  | "10"
  | "15"
  | "30"
  | "60"
  | "120"
  | "180"
  | "240"
  | "custom";

export const PROJECT_DEFAULT_DURATION_MINUTES = 60;
export const PROJECT_MAX_DURATION_MINUTES = 24 * 60;

export const PROJECT_DURATION_PRESET_MINUTES = {
  "10": 10,
  "15": 15,
  "30": 30,
  "60": 60,
  "120": 120,
  "180": 180,
  "240": 240,
} satisfies Record<Exclude<ProjectDurationPresetValue, "custom" | "default" | "all_day">, number>;

export interface ProjectCustomDurationDraft {
  value: string;
  unit: ProjectDurationUnit;
}

const customDurationShape = /^\d+(?:\.\d{0,2})?$/;

/**
 * Returns the preset that should represent a stored duration in the settings UI.
 */
export function projectDurationPresetFromMinutes(
  minutes: number | null,
  timeMode: ProjectDefaultEventTimeMode = "timed",
): ProjectDurationPresetValue {
  if (timeMode === "all_day") return "all_day";
  if (minutes === null) return "default";
  const preset = Object.entries(PROJECT_DURATION_PRESET_MINUTES)
    .find(([, value]) => value === minutes);
  return preset ? preset[0] as ProjectDurationPresetValue : "custom";
}

/**
 * Returns the effective duration used when a project has no explicit duration.
 */
export function projectEffectiveDurationMinutes(minutes: number | null | undefined): number {
  return minutes ?? PROJECT_DEFAULT_DURATION_MINUTES;
}

/**
 * Builds a custom duration input draft that preserves a stored minute duration.
 */
export function projectCustomDurationDraftFromMinutes(minutes: number): ProjectCustomDurationDraft {
  const hourValue = minutes / 60;
  if (Number.isInteger(hourValue * 100)) {
    return {
      value: String(hourValue),
      unit: "hours",
    };
  }
  return {
    value: String(minutes),
    unit: "minutes",
  };
}

/**
 * Checks whether the user-entered duration has at most two decimal digits.
 */
export function isProjectCustomDurationInputShape(value: string): boolean {
  const trimmed = value.trim();
  return trimmed === "" || customDurationShape.test(trimmed);
}

/**
 * Converts a valid custom duration input to stored whole minutes.
 */
export function projectDurationMinutesFromCustomInput(
  value: string,
  unit: ProjectDurationUnit,
): number | null {
  const trimmed = value.trim();
  if (!customDurationShape.test(trimmed)) return null;
  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed) || parsed <= 0) return null;
  if (unit === "hours" && parsed > 24) return null;
  if (unit === "minutes" && parsed > PROJECT_MAX_DURATION_MINUTES) return null;
  const minutes = unit === "hours" ? parsed * 60 : parsed;
  if (minutes > PROJECT_MAX_DURATION_MINUTES) return null;
  const roundedMinutes = Math.round(minutes);
  return roundedMinutes > 0 && roundedMinutes <= PROJECT_MAX_DURATION_MINUTES
    ? roundedMinutes
    : null;
}
