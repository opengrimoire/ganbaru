import {
  CALENDAR_VIEW_MODES,
  DEFAULT_CALENDAR_VIEW_MODE,
  PALETTE_SIZE,
  isCalendarViewMode,
  type CalendarViewMode,
  type EventColor,
} from "$lib/calendar/types";
import {
  DEFAULT_LANGUAGE_PREFERENCE,
  LANGUAGE_PREFERENCES,
  isLanguagePreference,
  parseLanguagePreference,
  type LanguagePreference,
} from "$lib/i18n/locales";

export {
  CALENDAR_VIEW_MODES,
  DEFAULT_CALENDAR_VIEW_MODE,
  DEFAULT_LANGUAGE_PREFERENCE,
  LANGUAGE_PREFERENCES,
  isCalendarViewMode,
  isLanguagePreference,
  parseLanguagePreference,
  type CalendarViewMode,
  type LanguagePreference,
};

/**
 * Pure types and helpers for app-wide user preferences that live outside
 * the theme system (font family, font scale, density). Themes may ship a
 * recommended pair of these, but the active values stay user-controlled.
 *
 * See `docs/features/themes/README.md`, section "Typography and density".
 */

export type FontFamilyId = string;

export interface FontFamilyOption {
  /** Stable identifier stored in user preferences. */
  id: FontFamilyId;
  /** Label shown in the settings picker. */
  displayName: string;
  /** CSS font-family stack applied on the root. */
  cssStack: string;
}

/**
 * Curated list of font family options. No remote fetching and no font file
 * loading: every option resolves through CSS system/installed-font fallbacks.
 * To add an option, append a new entry here. Unknown or removed IDs fall
 * back to `DEFAULT_FONT_FAMILY_ID`.
 */
export const FONT_FAMILIES: readonly FontFamilyOption[] = Object.freeze([
  {
    id: "inter",
    displayName: "Inter",
    cssStack:
      '"Inter Variable", ui-sans-serif, system-ui, sans-serif',
  },
  {
    id: "system",
    displayName: "System default",
    cssStack: "system-ui, -apple-system, Segoe UI, Roboto, sans-serif",
  },
  {
    id: "serif",
    displayName: "Serif",
    cssStack: 'ui-serif, Georgia, "Times New Roman", serif',
  },
  {
    id: "mono",
    displayName: "Monospace",
    cssStack:
      'ui-monospace, "SF Mono", Menlo, Monaco, Consolas, monospace',
  },
]);

export const DEFAULT_FONT_FAMILY_ID: FontFamilyId = "inter";

/**
 * Lower and upper bounds for the font scale multiplier. Clamped so a bad
 * input (or a theme recommendation out of range) cannot break the layout.
 */
export const FONT_SCALE_MIN = 0.85;
export const FONT_SCALE_MAX = 1.3;
export const DEFAULT_FONT_SCALE = 1.0;
export const FONT_SCALE_STEP = 0.05;
export const FONT_SCALE_LEVELS: readonly number[] = Object.freeze(
  Array.from(
    {
      length: Math.round((FONT_SCALE_MAX - FONT_SCALE_MIN) / FONT_SCALE_STEP) + 1,
    },
    (_, index) => Number((FONT_SCALE_MIN + index * FONT_SCALE_STEP).toFixed(2)),
  ),
);

export type CalendarTimeFormat = "24h" | "12h";

export const DEFAULT_CALENDAR_TIME_FORMAT: CalendarTimeFormat = "24h";
export const DEFAULT_CALENDAR_DIM_PAST_EVENTS = true;
export const DEFAULT_MUSIC_PAUSE_ON_POMODORO_PAUSE = true;
export const DEFAULT_PROFILE_DISPLAY_NAME = "";
export const DEFAULT_PROFILE_FULL_NAME = "";
export const DEFAULT_PROFILE_IMAGE_PATH: string | null = null;
export const DEFAULT_PROFILE_USE_IMAGE = true;
export const PROFILE_DISPLAY_NAME_FALLBACK = "You";
export const PROFILE_DISPLAY_NAME_MAX_CHARS = 25;
export const PROFILE_FULL_NAME_MAX_CHARS = 50;
const PROFILE_IMAGE_PATH_PATTERN = /^profile\/[a-f0-9]{64}\.(?:png|jpe?g|webp)$/i;
export const DEFAULT_NOTES_MENTION_NOTIFICATIONS_ENABLED = true;
export const DEFAULT_NOTES_REMINDER_NOTIFICATIONS_ENABLED = true;
export const DEFAULT_NOTES_USER_MENTION_NOTIFICATIONS_ENABLED = true;
export const DEFAULT_NOTES_TASK_MENTION_NOTIFICATIONS_ENABLED = true;
export const DEFAULT_NOTES_NOTIFICATION_INCLUDE_CONTENT = false;
export const DEFAULT_NOTES_SHOW_PROPERTY_ICONS = true;
export const DEFAULT_PROJECTS_SHOW_PROPERTY_ICONS = true;
export const FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS = Object.freeze(
  [1, 2, 3, 4, 5, 10, 15] as const,
);
export type FocusIdleThresholdMinutes =
  (typeof FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS)[number];
export const DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES: FocusIdleThresholdMinutes = 3;
export const DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE = true;
export const FOCUS_BREAK_SOUND_INTERVAL_SECONDS_OPTIONS = Object.freeze([0, 10, 15, 30, 60] as const);
export type FocusBreakSoundIntervalSeconds =
  (typeof FOCUS_BREAK_SOUND_INTERVAL_SECONDS_OPTIONS)[number];
export const DEFAULT_FOCUS_BREAK_FINISHED_REPEAT_SECONDS: FocusBreakSoundIntervalSeconds = 10;
export const DEFAULT_FOCUS_BREAK_END_WARNING_SECONDS: FocusBreakSoundIntervalSeconds = 10;
export const FOCUS_BREAK_END_ESC_PRESS_OPTIONS = Object.freeze([1, 3, 10, 20, 50] as const);
export type FocusBreakEndEscPresses =
  | (typeof FOCUS_BREAK_END_ESC_PRESS_OPTIONS)[number]
  | null;
export const DEFAULT_FOCUS_BREAK_END_ESC_PRESSES: FocusBreakEndEscPresses = 10;
export const FOCUS_BREAK_EXTENSION_LIMIT_OPTIONS = Object.freeze([1, 3, 5, 10, 15] as const);
export type FocusBreakExtensionLimit =
  | (typeof FOCUS_BREAK_EXTENSION_LIMIT_OPTIONS)[number]
  | null;
export const DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT: FocusBreakExtensionLimit = 3;
export const FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES_OPTIONS = Object.freeze(
  [0, 3, 5, 10, 15] as const,
);
export type FocusPauseNotificationIntervalMinutes =
  (typeof FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES_OPTIONS)[number];
export const DEFAULT_FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES:
  FocusPauseNotificationIntervalMinutes = 3;

export const TITLE_BAR_CONTROL_IDS = [
  "pomodoro",
  "music",
  "theme",
  "performance",
  "settings",
  "compactTabs",
] as const;

export type TitleBarControlId = (typeof TITLE_BAR_CONTROL_IDS)[number];
export type TitleBarVisibility = Record<TitleBarControlId, boolean>;

export const DEFAULT_TITLE_BAR_VISIBILITY: TitleBarVisibility = Object.freeze({
  pomodoro: true,
  music: true,
  theme: true,
  performance: false,
  settings: true,
  compactTabs: false,
});

/**
 * Clamp an arbitrary number into the supported font-scale range. Non-finite
 * inputs fall back to the default scale.
 */
export function clampFontScale(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_FONT_SCALE;
  if (value < FONT_SCALE_MIN) return FONT_SCALE_MIN;
  if (value > FONT_SCALE_MAX) return FONT_SCALE_MAX;
  return value;
}

/**
 * Look up a font family option by its ID. Returns undefined for unknown
 * IDs or non-string inputs.
 */
export function getFontFamilyById(
  id: FontFamilyId | undefined | null,
): FontFamilyOption | undefined {
  if (!id || typeof id !== "string") return undefined;
  return FONT_FAMILIES.find((f) => f.id === id);
}

/**
 * Resolve a stored font family ID to a CSS font-family stack. Unknown IDs
 * resolve to the default option's stack so a removed or typo'd setting
 * cannot render the app unstyled.
 */
export function resolveFontFamilyStack(id: FontFamilyId | undefined | null): string {
  const option = getFontFamilyById(id) ?? getFontFamilyById(DEFAULT_FONT_FAMILY_ID);
  // DEFAULT_FONT_FAMILY_ID is guaranteed to be in FONT_FAMILIES, so option is
  // never undefined; the non-null assertion is a type-system formality.
  return option!.cssStack;
}

export type ProfileDisplayNameValidation =
  | { ok: true; value: string }
  | { ok: false; reason: "too_long" | "control_characters" };

export type ProfileFullNameValidation =
  | { ok: true; value: string }
  | { ok: false; reason: "too_long" | "control_characters" };

/** Returns true when a profile image points to one managed raster asset. */
export function isProfileImagePath(value: unknown): value is string {
  return typeof value === "string" && PROFILE_IMAGE_PATH_PATTERN.test(value.trim());
}

/** Returns true when a profile color names one slot of the theme event palette. */
export function isProfileColor(value: unknown): value is EventColor {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 && value < PALETTE_SIZE;
}

/** Picks the first profile color uniformly from the theme event palette. */
export function randomProfileColor(random: () => number = Math.random): EventColor {
  return Math.min(PALETTE_SIZE - 1, Math.max(0, Math.floor(random() * PALETTE_SIZE)));
}

function normalizeProfileTextField(
  value: string,
  maxCharacters: number,
): ProfileDisplayNameValidation {
  const trimmed = value.trim();
  if ([...trimmed].length > maxCharacters) {
    return { ok: false, reason: "too_long" };
  }
  if ([...trimmed].some((character) => {
    const codePoint = character.codePointAt(0);
    return codePoint !== undefined && codePoint < 32;
  })) {
    return { ok: false, reason: "control_characters" };
  }
  return { ok: true, value: trimmed };
}

/**
 * Normalize the local profile display name. Empty names are valid because
 * product surfaces can fall back to a contextual label when needed.
 */
export function normalizeProfileDisplayName(value: string): ProfileDisplayNameValidation {
  return normalizeProfileTextField(value, PROFILE_DISPLAY_NAME_MAX_CHARS);
}

/**
 * Normalize the local profile full name. Empty names are valid while profile
 * completion remains optional.
 */
export function normalizeProfileFullName(value: string): ProfileFullNameValidation {
  return normalizeProfileTextField(value, PROFILE_FULL_NAME_MAX_CHARS);
}

export function isTitleBarControlId(value: unknown): value is TitleBarControlId {
  return typeof value === "string"
    && TITLE_BAR_CONTROL_IDS.includes(value as TitleBarControlId);
}

export function isCalendarTimeFormat(value: unknown): value is CalendarTimeFormat {
  return value === "24h" || value === "12h";
}

/**
 * How long a contact keeps one trust permission, as chosen in Contacts settings and the trust editor. Kept here so the
 * preferences store validates its trust defaults without loading Contacts modules into every shell.
 */
export const CONTACT_TRUST_DURATIONS = ["notAllowed", "once", "sevenDays", "thirtyDays", "untilRevoked"] as const;
export type ContactTrustDuration = (typeof CONTACT_TRUST_DURATIONS)[number];
/** Asking every time is the privacy-first default for new contacts. */
export const DEFAULT_CONTACT_TRUST_DURATION: ContactTrustDuration = "notAllowed";

export function isContactTrustDuration(value: unknown): value is ContactTrustDuration {
  return typeof value === "string" && (CONTACT_TRUST_DURATIONS as readonly string[]).includes(value);
}

export function clampFocusIdleThresholdMinutes(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES;
  const integerValue = Math.round(value);
  if (
    FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS.includes(
      integerValue as FocusIdleThresholdMinutes,
    )
  ) {
    return integerValue;
  }
  return DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES;
}

export function isFocusBreakSoundIntervalSeconds(
  value: unknown,
): value is FocusBreakSoundIntervalSeconds {
  return typeof value === "number"
    && FOCUS_BREAK_SOUND_INTERVAL_SECONDS_OPTIONS.includes(
      value as FocusBreakSoundIntervalSeconds,
    );
}

export function parseFocusBreakSoundIntervalSeconds(
  value: unknown,
  fallback: FocusBreakSoundIntervalSeconds,
): FocusBreakSoundIntervalSeconds {
  return isFocusBreakSoundIntervalSeconds(value) ? value : fallback;
}

export function isFocusBreakEndEscPresses(
  value: unknown,
): value is FocusBreakEndEscPresses {
  return value === null
    || (typeof value === "number"
      && FOCUS_BREAK_END_ESC_PRESS_OPTIONS.includes(
        value as Exclude<FocusBreakEndEscPresses, null>,
      ));
}

export function parseFocusBreakEndEscPresses(
  value: unknown,
  fallback: FocusBreakEndEscPresses,
): FocusBreakEndEscPresses {
  return isFocusBreakEndEscPresses(value) ? value : fallback;
}

export function isFocusBreakExtensionLimit(
  value: unknown,
): value is FocusBreakExtensionLimit {
  return value === null
    || (typeof value === "number"
      && FOCUS_BREAK_EXTENSION_LIMIT_OPTIONS.includes(
        value as Exclude<FocusBreakExtensionLimit, null>,
      ));
}

export function parseFocusBreakExtensionLimit(
  value: unknown,
  fallback: FocusBreakExtensionLimit,
): FocusBreakExtensionLimit {
  return isFocusBreakExtensionLimit(value) ? value : fallback;
}

export function isFocusPauseNotificationIntervalMinutes(
  value: unknown,
): value is FocusPauseNotificationIntervalMinutes {
  return typeof value === "number"
    && FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES_OPTIONS.includes(
      value as FocusPauseNotificationIntervalMinutes,
    );
}

export function parseFocusPauseNotificationIntervalMinutes(
  value: unknown,
  fallback: FocusPauseNotificationIntervalMinutes,
): FocusPauseNotificationIntervalMinutes {
  return isFocusPauseNotificationIntervalMinutes(value) ? value : fallback;
}

/**
 * Normalize persisted title bar visibility. Unknown keys are ignored and
 * missing or invalid values take their `DEFAULT_TITLE_BAR_VISIBILITY` entry.
 */
export function parseTitleBarVisibility(value: unknown): TitleBarVisibility {
  const visibility: TitleBarVisibility = { ...DEFAULT_TITLE_BAR_VISIBILITY };
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return visibility;
  }
  const record = value as Record<string, unknown>;
  for (const id of TITLE_BAR_CONTROL_IDS) {
    if (typeof record[id] === "boolean") {
      visibility[id] = record[id];
    }
  }
  return visibility;
}
