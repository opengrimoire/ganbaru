import { describe, it, expect } from "vitest";
import { PALETTE_SIZE } from "$lib/calendar/types";
import {
  FONT_FAMILIES,
  DEFAULT_FONT_FAMILY_ID,
  FONT_SCALE_MIN,
  FONT_SCALE_MAX,
  DEFAULT_FONT_SCALE,
  DEFAULT_CALENDAR_DIM_PAST_EVENTS,
  DEFAULT_CALENDAR_VIEW_MODE,
  DEFAULT_MUSIC_PAUSE_ON_POMODORO_PAUSE,
  DEFAULT_PROFILE_DISPLAY_NAME,
  DEFAULT_PROFILE_IMAGE_PATH,
  DEFAULT_PROFILE_FULL_NAME,
  DEFAULT_PROFILE_USE_IMAGE,
  PROFILE_DISPLAY_NAME_MAX_CHARS,
  isProfileColor,
  randomProfileColor,
  isProfileImagePath,
  PROFILE_FULL_NAME_MAX_CHARS,
  DEFAULT_CALENDAR_TIME_FORMAT,
  CALENDAR_VIEW_MODES,
  DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
  DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
  DEFAULT_FOCUS_BREAK_END_ESC_PRESSES,
  DEFAULT_FOCUS_BREAK_END_WARNING_SECONDS,
  DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT,
  DEFAULT_FOCUS_BREAK_FINISHED_REPEAT_SECONDS,
  DEFAULT_FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES,
  FOCUS_BREAK_END_ESC_PRESS_OPTIONS,
  FOCUS_BREAK_EXTENSION_LIMIT_OPTIONS,
  FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS,
  FOCUS_BREAK_SOUND_INTERVAL_SECONDS_OPTIONS,
  FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES_OPTIONS,
  DEFAULT_TITLE_BAR_VISIBILITY,
  DEFAULT_LANGUAGE_PREFERENCE,
  TITLE_BAR_CONTROL_IDS,
  clampFocusIdleThresholdMinutes,
  clampFontScale,
  getFontFamilyById,
  isCalendarTimeFormat,
  isCalendarViewMode,
  isFocusBreakEndEscPresses,
  isFocusBreakExtensionLimit,
  isFocusBreakSoundIntervalSeconds,
  isFocusPauseNotificationIntervalMinutes,
  isTitleBarControlId,
  isLanguagePreference,
  parseFocusBreakEndEscPresses,
  parseFocusBreakExtensionLimit,
  parseFocusBreakSoundIntervalSeconds,
  parseFocusPauseNotificationIntervalMinutes,
  parseLanguagePreference,
  parseTitleBarVisibility,
  normalizeProfileDisplayName,
  normalizeProfileFullName,
  resolveFontFamilyStack,
} from "./preference-options";

describe("FONT_FAMILIES registry", () => {
  it("includes the default font family ID", () => {
    expect(FONT_FAMILIES.some((f) => f.id === DEFAULT_FONT_FAMILY_ID)).toBe(true);
  });

  it("has unique IDs across all options", () => {
    const ids = FONT_FAMILIES.map((f) => f.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("every option carries a non-empty CSS font stack", () => {
    for (const option of FONT_FAMILIES) {
      expect(option.cssStack.length).toBeGreaterThan(0);
    }
  });

  it("every option carries a user-visible display name", () => {
    for (const option of FONT_FAMILIES) {
      expect(option.displayName.length).toBeGreaterThan(0);
    }
  });

  it("is frozen to prevent mutation", () => {
    expect(Object.isFrozen(FONT_FAMILIES)).toBe(true);
  });
});

describe("clampFontScale", () => {
  it("returns the value unchanged when it sits within range", () => {
    expect(clampFontScale(1.0)).toBe(1.0);
    expect(clampFontScale(0.9)).toBe(0.9);
    expect(clampFontScale(1.2)).toBe(1.2);
  });

  it("clamps values below the minimum up to FONT_SCALE_MIN", () => {
    expect(clampFontScale(0.5)).toBe(FONT_SCALE_MIN);
    expect(clampFontScale(-1)).toBe(FONT_SCALE_MIN);
    expect(clampFontScale(0)).toBe(FONT_SCALE_MIN);
  });

  it("clamps values above the maximum down to FONT_SCALE_MAX", () => {
    expect(clampFontScale(2)).toBe(FONT_SCALE_MAX);
    expect(clampFontScale(100)).toBe(FONT_SCALE_MAX);
  });

  it("accepts the exact boundary values", () => {
    expect(clampFontScale(FONT_SCALE_MIN)).toBe(FONT_SCALE_MIN);
    expect(clampFontScale(FONT_SCALE_MAX)).toBe(FONT_SCALE_MAX);
  });

  it("falls back to the default scale for non-finite inputs", () => {
    expect(clampFontScale(Number.NaN)).toBe(DEFAULT_FONT_SCALE);
    expect(clampFontScale(Number.POSITIVE_INFINITY)).toBe(DEFAULT_FONT_SCALE);
    expect(clampFontScale(Number.NEGATIVE_INFINITY)).toBe(DEFAULT_FONT_SCALE);
  });
});

describe("getFontFamilyById", () => {
  it("returns the option for a registered ID", () => {
    const option = getFontFamilyById(DEFAULT_FONT_FAMILY_ID);
    expect(option?.id).toBe(DEFAULT_FONT_FAMILY_ID);
  });

  it("returns undefined for unknown IDs", () => {
    expect(getFontFamilyById("does-not-exist")).toBeUndefined();
  });

  it("returns undefined for null, undefined, and empty inputs", () => {
    expect(getFontFamilyById(null)).toBeUndefined();
    expect(getFontFamilyById(undefined)).toBeUndefined();
    expect(getFontFamilyById("")).toBeUndefined();
  });
});

describe("resolveFontFamilyStack", () => {
  it("returns the stack for a known ID", () => {
    const stack = resolveFontFamilyStack(DEFAULT_FONT_FAMILY_ID);
    const defaultOption = FONT_FAMILIES.find(
      (f) => f.id === DEFAULT_FONT_FAMILY_ID,
    );
    expect(stack).toBe(defaultOption!.cssStack);
  });

  it("falls back to the default option's stack for unknown IDs", () => {
    const defaultOption = FONT_FAMILIES.find(
      (f) => f.id === DEFAULT_FONT_FAMILY_ID,
    );
    expect(resolveFontFamilyStack("nope")).toBe(defaultOption!.cssStack);
  });

  it("falls back to the default option's stack for null/undefined", () => {
    const defaultOption = FONT_FAMILIES.find(
      (f) => f.id === DEFAULT_FONT_FAMILY_ID,
    );
    expect(resolveFontFamilyStack(null)).toBe(defaultOption!.cssStack);
    expect(resolveFontFamilyStack(undefined)).toBe(defaultOption!.cssStack);
  });
});

describe("calendar appearance preferences", () => {
  it("defaults to 24-hour time and dimmed past event colors", () => {
    expect(DEFAULT_CALENDAR_TIME_FORMAT).toBe("24h");
    expect(DEFAULT_CALENDAR_VIEW_MODE).toBe("week");
    expect(DEFAULT_CALENDAR_DIM_PAST_EVENTS).toBe(true);
  });

  it("accepts supported calendar time formats only", () => {
    expect(isCalendarTimeFormat("24h")).toBe(true);
    expect(isCalendarTimeFormat("12h")).toBe(true);
    expect(isCalendarTimeFormat("24-hour")).toBe(false);
    expect(isCalendarTimeFormat(true)).toBe(false);
    expect(isCalendarTimeFormat(undefined)).toBe(false);
  });

  it("accepts supported calendar view modes only", () => {
    expect(CALENDAR_VIEW_MODES).toEqual(["day", "workweek", "week", "month"]);
    expect(isCalendarViewMode("day")).toBe(true);
    expect(isCalendarViewMode("workweek")).toBe(true);
    expect(isCalendarViewMode("week")).toBe(true);
    expect(isCalendarViewMode("month")).toBe(true);
    expect(isCalendarViewMode("7d")).toBe(false);
    expect(isCalendarViewMode("agenda")).toBe(false);
    expect(isCalendarViewMode(undefined)).toBe(false);
  });
});

describe("language preferences", () => {
  it("defaults to the system language", () => {
    expect(DEFAULT_LANGUAGE_PREFERENCE).toBe("system");
  });

  it("accepts supported language preferences only", () => {
    expect(isLanguagePreference("system")).toBe(true);
    expect(isLanguagePreference("en")).toBe(true);
    expect(isLanguagePreference("es")).toBe(true);
    expect(isLanguagePreference("fr")).toBe(false);
    expect(isLanguagePreference(undefined)).toBe(false);
  });

  it("falls back to system when parsing invalid stored language preferences", () => {
    expect(parseLanguagePreference("es")).toBe("es");
    expect(parseLanguagePreference("fr")).toBe(DEFAULT_LANGUAGE_PREFERENCE);
    expect(parseLanguagePreference(null)).toBe(DEFAULT_LANGUAGE_PREFERENCE);
  });
});

describe("music pomodoro preferences", () => {
  it("pauses music on pomodoro pause by default", () => {
    expect(DEFAULT_MUSIC_PAUSE_ON_POMODORO_PAUSE).toBe(true);
  });
});

describe("profile preferences", () => {
  it("defaults to empty profile names", () => {
    expect(DEFAULT_PROFILE_DISPLAY_NAME).toBe("");
    expect(DEFAULT_PROFILE_FULL_NAME).toBe("");
    expect(PROFILE_DISPLAY_NAME_MAX_CHARS).toBe(25);
    expect(PROFILE_FULL_NAME_MAX_CHARS).toBe(50);
  });

  it("trims profile display names while allowing an empty value", () => {
    expect(normalizeProfileDisplayName("  Alice  ")).toEqual({
      ok: true,
      value: "Alice",
    });
    expect(normalizeProfileDisplayName("   ")).toEqual({
      ok: true,
      value: "",
    });
  });

  it("accepts only managed raster profile image paths", () => {
    expect(DEFAULT_PROFILE_IMAGE_PATH).toBeNull();
    expect(isProfileImagePath(`profile/${"a".repeat(64)}.png`)).toBe(true);
    expect(isProfileImagePath(`profile/${"b".repeat(64)}.jpeg`)).toBe(true);
    expect(isProfileImagePath("profile/image.png")).toBe(false);
    expect(isProfileImagePath(`profile/nested/${"a".repeat(64)}.webp`)).toBe(false);
    expect(isProfileImagePath(`project-icons/${"a".repeat(64)}.png`)).toBe(false);
    expect(isProfileImagePath(`profile/${"a".repeat(64)}.svg`)).toBe(false);
  });

  it("accepts only theme palette slots as profile colors", () => {
    expect(DEFAULT_PROFILE_USE_IMAGE).toBe(true);
    expect(isProfileColor(0)).toBe(true);
    expect(isProfileColor(PALETTE_SIZE - 1)).toBe(true);
    expect(isProfileColor(PALETTE_SIZE)).toBe(false);
    expect(isProfileColor(-1)).toBe(false);
    expect(isProfileColor(1.5)).toBe(false);
    expect(isProfileColor("3")).toBe(false);
    expect(isProfileColor(null)).toBe(false);
  });

  it("picks a random profile color that is always a palette slot", () => {
    expect(randomProfileColor(() => 0)).toBe(0);
    expect(randomProfileColor(() => 0.5)).toBe(PALETTE_SIZE / 2);
    expect(randomProfileColor(() => 0.999999)).toBe(PALETTE_SIZE - 1);
    expect(randomProfileColor(() => 1)).toBe(PALETTE_SIZE - 1);
    expect(randomProfileColor(() => -0.2)).toBe(0);
    for (let sample = 0; sample < 50; sample += 1) {
      expect(isProfileColor(randomProfileColor())).toBe(true);
    }
  });

  it("trims profile full names while allowing an empty value", () => {
    expect(normalizeProfileFullName("  Alice Example  ")).toEqual({
      ok: true,
      value: "Alice Example",
    });
    expect(normalizeProfileFullName("   ")).toEqual({
      ok: true,
      value: "",
    });
  });

  it("rejects profile display names that are too long or contain control characters", () => {
    expect(normalizeProfileDisplayName("a".repeat(PROFILE_DISPLAY_NAME_MAX_CHARS + 1))).toEqual({
      ok: false,
      reason: "too_long",
    });
    expect(normalizeProfileDisplayName("Bad\u0000Name")).toEqual({
      ok: false,
      reason: "control_characters",
    });
  });

  it("rejects profile full names that are too long or contain control characters", () => {
    expect(normalizeProfileFullName("a".repeat(PROFILE_FULL_NAME_MAX_CHARS + 1))).toEqual({
      ok: false,
      reason: "too_long",
    });
    expect(normalizeProfileFullName("Bad\u0000Name")).toEqual({
      ok: false,
      reason: "control_characters",
    });
  });
});

describe("focus preferences", () => {
  it("defaults idle pause to enabled with a 3 minute threshold", () => {
    expect(DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE).toBe(true);
    expect(DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES).toBe(3);
  });

  it("uses fixed idle threshold minutes with 3 minutes as the default", () => {
    expect(FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS).toEqual([1, 2, 3, 4, 5, 10, 15]);
    expect(Object.isFrozen(FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS)).toBe(true);
    expect(DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES).toBe(3);
  });

  it("uses fixed break screen sound intervals with 10 seconds as the default", () => {
    expect(FOCUS_BREAK_SOUND_INTERVAL_SECONDS_OPTIONS).toEqual([0, 10, 15, 30, 60]);
    expect(Object.isFrozen(FOCUS_BREAK_SOUND_INTERVAL_SECONDS_OPTIONS)).toBe(true);
    expect(DEFAULT_FOCUS_BREAK_FINISHED_REPEAT_SECONDS).toBe(10);
    expect(DEFAULT_FOCUS_BREAK_END_WARNING_SECONDS).toBe(10);
  });

  it("uses fixed break end Esc counts with 10 presses as the default", () => {
    expect(FOCUS_BREAK_END_ESC_PRESS_OPTIONS).toEqual([1, 3, 10, 20, 50]);
    expect(Object.isFrozen(FOCUS_BREAK_END_ESC_PRESS_OPTIONS)).toBe(true);
    expect(DEFAULT_FOCUS_BREAK_END_ESC_PRESSES).toBe(10);
  });

  it("uses fixed break extension limits with 3 times as the default", () => {
    expect(FOCUS_BREAK_EXTENSION_LIMIT_OPTIONS).toEqual([1, 3, 5, 10, 15]);
    expect(Object.isFrozen(FOCUS_BREAK_EXTENSION_LIMIT_OPTIONS)).toBe(true);
    expect(DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT).toBe(3);
  });

  it("uses fixed paused focus notification intervals with 3 minutes as the default", () => {
    expect(FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES_OPTIONS).toEqual([0, 3, 5, 10, 15]);
    expect(Object.isFrozen(FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES_OPTIONS)).toBe(true);
    expect(DEFAULT_FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES).toBe(3);
  });

  it("accepts only supported break screen sound intervals", () => {
    expect(isFocusBreakSoundIntervalSeconds(0)).toBe(true);
    expect(isFocusBreakSoundIntervalSeconds(10)).toBe(true);
    expect(isFocusBreakSoundIntervalSeconds(15)).toBe(true);
    expect(isFocusBreakSoundIntervalSeconds(30)).toBe(true);
    expect(isFocusBreakSoundIntervalSeconds(60)).toBe(true);
    expect(isFocusBreakSoundIntervalSeconds(20)).toBe(false);
    expect(isFocusBreakSoundIntervalSeconds("10")).toBe(false);
    expect(isFocusBreakSoundIntervalSeconds(null)).toBe(false);
  });

  it("accepts supported break end Esc counts plus disabled", () => {
    expect(isFocusBreakEndEscPresses(null)).toBe(true);
    expect(isFocusBreakEndEscPresses(1)).toBe(true);
    expect(isFocusBreakEndEscPresses(3)).toBe(true);
    expect(isFocusBreakEndEscPresses(10)).toBe(true);
    expect(isFocusBreakEndEscPresses(20)).toBe(true);
    expect(isFocusBreakEndEscPresses(50)).toBe(true);
    expect(isFocusBreakEndEscPresses(2)).toBe(false);
    expect(isFocusBreakEndEscPresses("10")).toBe(false);
    expect(isFocusBreakEndEscPresses(undefined)).toBe(false);
  });

  it("accepts supported break extension limits plus disabled", () => {
    expect(isFocusBreakExtensionLimit(null)).toBe(true);
    expect(isFocusBreakExtensionLimit(1)).toBe(true);
    expect(isFocusBreakExtensionLimit(3)).toBe(true);
    expect(isFocusBreakExtensionLimit(5)).toBe(true);
    expect(isFocusBreakExtensionLimit(10)).toBe(true);
    expect(isFocusBreakExtensionLimit(15)).toBe(true);
    expect(isFocusBreakExtensionLimit(2)).toBe(false);
    expect(isFocusBreakExtensionLimit("3")).toBe(false);
    expect(isFocusBreakExtensionLimit(undefined)).toBe(false);
  });

  it("accepts only supported paused focus notification intervals", () => {
    expect(isFocusPauseNotificationIntervalMinutes(0)).toBe(true);
    expect(isFocusPauseNotificationIntervalMinutes(3)).toBe(true);
    expect(isFocusPauseNotificationIntervalMinutes(5)).toBe(true);
    expect(isFocusPauseNotificationIntervalMinutes(10)).toBe(true);
    expect(isFocusPauseNotificationIntervalMinutes(15)).toBe(true);
    expect(isFocusPauseNotificationIntervalMinutes(1)).toBe(false);
    expect(isFocusPauseNotificationIntervalMinutes("3")).toBe(false);
    expect(isFocusPauseNotificationIntervalMinutes(null)).toBe(false);
  });

  it("falls back when parsing unsupported break screen sound intervals", () => {
    expect(parseFocusBreakSoundIntervalSeconds(30, 10)).toBe(30);
    expect(parseFocusBreakSoundIntervalSeconds(20, 10)).toBe(10);
    expect(parseFocusBreakSoundIntervalSeconds("10", 10)).toBe(10);
  });

  it("falls back when parsing unsupported break end Esc counts", () => {
    expect(parseFocusBreakEndEscPresses(20, 10)).toBe(20);
    expect(parseFocusBreakEndEscPresses(null, 10)).toBeNull();
    expect(parseFocusBreakEndEscPresses(2, 10)).toBe(10);
    expect(parseFocusBreakEndEscPresses("10", 10)).toBe(10);
  });

  it("falls back when parsing unsupported break extension limits", () => {
    expect(parseFocusBreakExtensionLimit(5, 3)).toBe(5);
    expect(parseFocusBreakExtensionLimit(null, 3)).toBeNull();
    expect(parseFocusBreakExtensionLimit(2, 3)).toBe(3);
    expect(parseFocusBreakExtensionLimit("3", 3)).toBe(3);
  });

  it("falls back when parsing unsupported paused focus notification intervals", () => {
    expect(parseFocusPauseNotificationIntervalMinutes(5, 3)).toBe(5);
    expect(parseFocusPauseNotificationIntervalMinutes(1, 3)).toBe(3);
    expect(parseFocusPauseNotificationIntervalMinutes("3", 3)).toBe(3);
  });

  it("clamps idle threshold minutes to the supported list", () => {
    expect(clampFocusIdleThresholdMinutes(1)).toBe(1);
    expect(clampFocusIdleThresholdMinutes(5)).toBe(5);
    expect(clampFocusIdleThresholdMinutes(10)).toBe(10);
    expect(clampFocusIdleThresholdMinutes(15)).toBe(15);
    expect(clampFocusIdleThresholdMinutes(0)).toBe(DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES);
    expect(clampFocusIdleThresholdMinutes(6)).toBe(DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES);
    expect(clampFocusIdleThresholdMinutes(16)).toBe(DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES);
  });

  it("rounds fractional values and falls back for non-finite inputs", () => {
    expect(clampFocusIdleThresholdMinutes(4.4)).toBe(4);
    expect(clampFocusIdleThresholdMinutes(4.5)).toBe(5);
    expect(clampFocusIdleThresholdMinutes(8.5)).toBe(DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES);
    expect(clampFocusIdleThresholdMinutes(Number.NaN)).toBe(
      DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
    );
  });
});

describe("title bar visibility helpers", () => {
  it("accepts registered title bar control IDs only", () => {
    expect(isTitleBarControlId("pomodoro")).toBe(true);
    expect(isTitleBarControlId("music")).toBe(true);
    expect(isTitleBarControlId("settings")).toBe(true);
    expect(isTitleBarControlId("compactTabs")).toBe(true);
    expect(isTitleBarControlId("reset")).toBe(false);
    expect(isTitleBarControlId("help")).toBe(false);
    expect(isTitleBarControlId("calendarTab")).toBe(false);
    expect(isTitleBarControlId("todoTab")).toBe(false);
    expect(isTitleBarControlId("tabs")).toBe(false);
    expect(isTitleBarControlId("window-controls")).toBe(false);
    expect(isTitleBarControlId(null)).toBe(false);
  });

  it("has a default visibility value for every registered control", () => {
    expect(Object.keys(DEFAULT_TITLE_BAR_VISIBILITY).sort()).toEqual(
      [...TITLE_BAR_CONTROL_IDS].sort(),
    );
    expect(DEFAULT_TITLE_BAR_VISIBILITY.performance).toBe(false);
  });

  it("parses stored booleans and fills missing values with defaults", () => {
    expect(
      parseTitleBarVisibility({
        compactTabs: true,
        music: false,
        performance: true,
      }),
    ).toEqual({
      ...DEFAULT_TITLE_BAR_VISIBILITY,
      compactTabs: true,
      music: false,
      performance: true,
    });
  });

  it("ignores unknown keys and non-boolean values", () => {
    expect(
      parseTitleBarVisibility({
        compactTabs: "true",
        reset: true,
        settings: false,
        unknown: false,
      }),
    ).toEqual({
      ...DEFAULT_TITLE_BAR_VISIBILITY,
      settings: false,
    });
  });

  it("falls back to defaults for malformed stored values", () => {
    expect(parseTitleBarVisibility(null)).toEqual(DEFAULT_TITLE_BAR_VISIBILITY);
    expect(parseTitleBarVisibility([])).toEqual(DEFAULT_TITLE_BAR_VISIBILITY);
    expect(parseTitleBarVisibility("bad")).toEqual(DEFAULT_TITLE_BAR_VISIBILITY);
  });
});
