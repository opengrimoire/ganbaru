import {
  type FontFamilyId,
  FONT_FAMILIES,
  DEFAULT_FONT_FAMILY_ID,
  DEFAULT_FONT_SCALE,
  DEFAULT_CALENDAR_DIM_PAST_EVENTS,
  DEFAULT_PROFILE_DISPLAY_NAME,
  DEFAULT_PROFILE_FULL_NAME,
  DEFAULT_PROFILE_IMAGE_PATH,
  DEFAULT_PROFILE_USE_IMAGE,
  DEFAULT_MUSIC_PAUSE_ON_POMODORO_PAUSE,
  DEFAULT_CALENDAR_TIME_FORMAT,
  DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
  DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
  DEFAULT_FOCUS_BREAK_END_WARNING_SECONDS,
  DEFAULT_FOCUS_BREAK_END_ESC_PRESSES,
  DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT,
  DEFAULT_FOCUS_BREAK_FINISHED_REPEAT_SECONDS,
  DEFAULT_FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES,
  DEFAULT_NOTES_MENTION_NOTIFICATIONS_ENABLED,
  DEFAULT_NOTES_NOTIFICATION_INCLUDE_CONTENT,
  DEFAULT_NOTES_REMINDER_NOTIFICATIONS_ENABLED,
  DEFAULT_NOTES_TASK_MENTION_NOTIFICATIONS_ENABLED,
  DEFAULT_NOTES_USER_MENTION_NOTIFICATIONS_ENABLED,
  DEFAULT_NOTES_SHOW_PROPERTY_ICONS,
  DEFAULT_PROJECTS_SHOW_PROPERTY_ICONS,
  DEFAULT_TITLE_BAR_VISIBILITY,
  DEFAULT_CALENDAR_VIEW_MODE,
  DEFAULT_CONTACT_TRUST_DURATION,
  LANGUAGE_PREFERENCES,
  type CalendarTimeFormat,
  type CalendarViewMode,
  type FocusBreakEndEscPresses,
  type FocusBreakExtensionLimit,
  type FocusBreakSoundIntervalSeconds,
  type FocusPauseNotificationIntervalMinutes,
  type LanguagePreference,
  type ContactTrustDuration,
  type TitleBarControlId,
  type TitleBarVisibility,
  clampFocusIdleThresholdMinutes,
  clampFontScale,
  getFontFamilyById,
  isCalendarTimeFormat,
  isCalendarViewMode,
  isContactTrustDuration,
  isProfileColor,
  randomProfileColor,
  isProfileImagePath,
  isTitleBarControlId,
  parseFocusBreakEndEscPresses,
  parseFocusBreakExtensionLimit,
  parseFocusBreakSoundIntervalSeconds,
  parseFocusPauseNotificationIntervalMinutes,
  parseTitleBarVisibility,
  normalizeProfileDisplayName,
  normalizeProfileFullName,
  resolveFontFamilyStack,
} from "./preference-options";
import { getConfigKey, setConfigKey } from "../vault/config";
import type { EventColor } from "$lib/calendar/types";
import {
  isDefaultProfileImageCrop,
  isProfileImageCrop,
  PROFILE_IMAGE_DEFAULT_CROP,
  type ProfileImageCrop,
} from "$lib/profile/identity";
import { getLocalization } from "$lib/i18n/translator.svelte";
import {
  DEFAULT_NOTES_PAGE_OPEN_MODE,
  isNotesPageOpenMode,
  type NotesPageOpenMode,
} from "$lib/notes/pages/open-mode";

const PROFILE_DISPLAY_NAME_CONFIG_KEY = "profile.displayName";
const PROFILE_FULL_NAME_CONFIG_KEY = "profile.fullName";
const PROFILE_IMAGE_PATH_CONFIG_KEY = "profile.imagePath";
const PROFILE_USE_IMAGE_CONFIG_KEY = "profile.useImage";
const PROFILE_COLOR_CONFIG_KEY = "profile.color";
const PROFILE_IMAGE_CROP_CONFIG_KEY = "profile.imageCrop";
const FONT_FAMILY_CONFIG_KEY = "preferences.fontFamilyId";
const FONT_SCALE_CONFIG_KEY = "preferences.fontScale";
const EVENT_TIMEZONE_DISPLAY_CONFIG_KEY = "preferences.eventTimezoneDisplay";
const CALENDAR_TIME_FORMAT_CONFIG_KEY = "preferences.calendarTimeFormat";
const CALENDAR_VIEW_MODE_CONFIG_KEY = "preferences.calendarViewMode";
const CALENDAR_DIM_PAST_EVENTS_CONFIG_KEY = "preferences.calendarDimPastEvents";
const FOCUS_IDLE_THRESHOLD_MINUTES_CONFIG_KEY = "preferences.focusIdleThresholdMinutes";
const FOCUS_IDLE_PAUSE_ON_EVENT_CREATE_CONFIG_KEY = "preferences.focusIdlePauseOnEventCreate";
const FOCUS_BREAK_FINISHED_REPEAT_SECONDS_CONFIG_KEY =
  "preferences.focusBreakFinishedRepeatSeconds";
const FOCUS_BREAK_END_WARNING_SECONDS_CONFIG_KEY =
  "preferences.focusBreakEndWarningSeconds";
const FOCUS_BREAK_END_ESC_PRESSES_CONFIG_KEY =
  "preferences.focusBreakEndEscPresses";
const FOCUS_BREAK_EXTENSION_LIMIT_CONFIG_KEY =
  "preferences.focusBreakExtensionLimit";
const FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES_CONFIG_KEY =
  "preferences.focusPauseNotificationIntervalMinutes";
const MUSIC_PAUSE_ON_POMODORO_PAUSE_CONFIG_KEY = "preferences.musicPauseOnPomodoroPause";
const NOTES_DEFAULT_OPEN_MODE_CONFIG_KEY = "preferences.notesDefaultOpenMode";
const NOTES_MENTION_NOTIFICATIONS_ENABLED_CONFIG_KEY =
  "preferences.notesMentionNotificationsEnabled";
const NOTES_REMINDER_NOTIFICATIONS_ENABLED_CONFIG_KEY =
  "preferences.notesReminderNotificationsEnabled";
const NOTES_USER_MENTION_NOTIFICATIONS_ENABLED_CONFIG_KEY =
  "preferences.notesUserMentionNotificationsEnabled";
const NOTES_TASK_MENTION_NOTIFICATIONS_ENABLED_CONFIG_KEY =
  "preferences.notesTaskMentionNotificationsEnabled";
const NOTES_NOTIFICATION_INCLUDE_CONTENT_CONFIG_KEY =
  "preferences.notesNotificationIncludeContent";
const NOTES_SHOW_PROPERTY_ICONS_CONFIG_KEY = "preferences.notesShowPropertyIcons";
const PROJECTS_SHOW_PROPERTY_ICONS_CONFIG_KEY = "preferences.projectsShowPropertyIcons";
const TITLE_BAR_VISIBILITY_CONFIG_KEY = "preferences.titleBarVisibility";
const CONTACTS_INVITE_TRUST_DEFAULT_CONFIG_KEY = "contacts.trustDefaults.invite";
const CONTACTS_MESSAGE_TRUST_DEFAULT_CONFIG_KEY = "contacts.trustDefaults.message";

export type EventTimezoneDisplay = "device" | "homeZone";
const DEFAULT_EVENT_TIMEZONE_DISPLAY: EventTimezoneDisplay = "device";

function loadSavedFontFamilyId(): FontFamilyId {
  const saved = getConfigKey<string | undefined>(FONT_FAMILY_CONFIG_KEY, undefined);
  if (saved && getFontFamilyById(saved)) return saved;
  return DEFAULT_FONT_FAMILY_ID;
}

function loadSavedFontScale(): number {
  const saved = getConfigKey<number | undefined>(FONT_SCALE_CONFIG_KEY, undefined);
  if (typeof saved !== "number" || !Number.isFinite(saved)) return DEFAULT_FONT_SCALE;
  return clampFontScale(saved);
}

function loadSavedEventTimezoneDisplay(): EventTimezoneDisplay {
  const saved = getConfigKey<string | undefined>(EVENT_TIMEZONE_DISPLAY_CONFIG_KEY, undefined);
  if (saved === "device" || saved === "homeZone") return saved;
  return DEFAULT_EVENT_TIMEZONE_DISPLAY;
}

function loadSavedProfileDisplayName(): string {
  const saved = getConfigKey<unknown>(PROFILE_DISPLAY_NAME_CONFIG_KEY, undefined);
  if (typeof saved !== "string") return DEFAULT_PROFILE_DISPLAY_NAME;
  const normalized = normalizeProfileDisplayName(saved);
  return normalized.ok ? normalized.value : DEFAULT_PROFILE_DISPLAY_NAME;
}

function loadSavedProfileFullName(): string {
  const saved = getConfigKey<unknown>(PROFILE_FULL_NAME_CONFIG_KEY, undefined);
  if (typeof saved !== "string") return DEFAULT_PROFILE_FULL_NAME;
  const normalized = normalizeProfileFullName(saved);
  return normalized.ok ? normalized.value : DEFAULT_PROFILE_FULL_NAME;
}

function loadSavedProfileImagePath(): string | null {
  const saved = getConfigKey<unknown>(PROFILE_IMAGE_PATH_CONFIG_KEY, undefined);
  return isProfileImagePath(saved) ? saved.trim() : DEFAULT_PROFILE_IMAGE_PATH;
}

function loadSavedProfileUseImage(): boolean {
  const saved = getConfigKey<unknown>(PROFILE_USE_IMAGE_CONFIG_KEY, undefined);
  return typeof saved === "boolean" ? saved : DEFAULT_PROFILE_USE_IMAGE;
}

/** Loads the profile color, choosing and saving a random palette slot the first time. */
function loadSavedProfileColor(): EventColor {
  const saved = getConfigKey<unknown>(PROFILE_COLOR_CONFIG_KEY, undefined);
  if (isProfileColor(saved)) return saved;
  const color = randomProfileColor();
  setConfigKey(PROFILE_COLOR_CONFIG_KEY, color);
  return color;
}

function loadSavedProfileImageCrop(): ProfileImageCrop {
  const saved = getConfigKey<unknown>(PROFILE_IMAGE_CROP_CONFIG_KEY, undefined);
  return isProfileImageCrop(saved) ? { x: saved.x, y: saved.y, zoom: saved.zoom } : { ...PROFILE_IMAGE_DEFAULT_CROP };
}

function loadSavedCalendarTimeFormat(): CalendarTimeFormat {
  const saved = getConfigKey<unknown>(CALENDAR_TIME_FORMAT_CONFIG_KEY, undefined);
  if (isCalendarTimeFormat(saved)) return saved;
  return DEFAULT_CALENDAR_TIME_FORMAT;
}

function loadSavedCalendarViewMode(): CalendarViewMode {
  const saved = getConfigKey<unknown>(CALENDAR_VIEW_MODE_CONFIG_KEY, undefined);
  if (isCalendarViewMode(saved)) return saved;
  return DEFAULT_CALENDAR_VIEW_MODE;
}

function loadSavedCalendarDimPastEvents(): boolean {
  const saved = getConfigKey<unknown>(CALENDAR_DIM_PAST_EVENTS_CONFIG_KEY, undefined);
  if (typeof saved === "boolean") return saved;
  return DEFAULT_CALENDAR_DIM_PAST_EVENTS;
}

function loadSavedFocusIdleThresholdMinutes(): number {
  const saved = getConfigKey<unknown>(FOCUS_IDLE_THRESHOLD_MINUTES_CONFIG_KEY, undefined);
  if (typeof saved === "number") return clampFocusIdleThresholdMinutes(saved);
  return DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES;
}

function loadSavedFocusIdlePauseOnEventCreate(): boolean {
  const saved = getConfigKey<unknown>(FOCUS_IDLE_PAUSE_ON_EVENT_CREATE_CONFIG_KEY, undefined);
  if (typeof saved === "boolean") return saved;
  return DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE;
}

function loadSavedFocusBreakFinishedRepeatSeconds(): FocusBreakSoundIntervalSeconds {
  const saved = getConfigKey<unknown>(FOCUS_BREAK_FINISHED_REPEAT_SECONDS_CONFIG_KEY, undefined);
  return parseFocusBreakSoundIntervalSeconds(
    saved,
    DEFAULT_FOCUS_BREAK_FINISHED_REPEAT_SECONDS,
  );
}

function loadSavedFocusBreakEndWarningSeconds(): FocusBreakSoundIntervalSeconds {
  const saved = getConfigKey<unknown>(FOCUS_BREAK_END_WARNING_SECONDS_CONFIG_KEY, undefined);
  return parseFocusBreakSoundIntervalSeconds(
    saved,
    DEFAULT_FOCUS_BREAK_END_WARNING_SECONDS,
  );
}

function loadSavedFocusBreakEndEscPresses(): FocusBreakEndEscPresses {
  const saved = getConfigKey<unknown>(FOCUS_BREAK_END_ESC_PRESSES_CONFIG_KEY, undefined);
  return parseFocusBreakEndEscPresses(saved, DEFAULT_FOCUS_BREAK_END_ESC_PRESSES);
}

function loadSavedFocusBreakExtensionLimit(): FocusBreakExtensionLimit {
  const saved = getConfigKey<unknown>(FOCUS_BREAK_EXTENSION_LIMIT_CONFIG_KEY, undefined);
  return parseFocusBreakExtensionLimit(saved, DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT);
}

function loadSavedFocusPauseNotificationIntervalMinutes():
  FocusPauseNotificationIntervalMinutes {
  const saved = getConfigKey<unknown>(
    FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES_CONFIG_KEY,
    undefined,
  );
  return parseFocusPauseNotificationIntervalMinutes(
    saved,
    DEFAULT_FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES,
  );
}

function loadSavedMusicPauseOnPomodoroPause(): boolean {
  const saved = getConfigKey<unknown>(MUSIC_PAUSE_ON_POMODORO_PAUSE_CONFIG_KEY, undefined);
  if (typeof saved === "boolean") return saved;
  return DEFAULT_MUSIC_PAUSE_ON_POMODORO_PAUSE;
}

function loadSavedBooleanPreference(key: string, fallback: boolean): boolean {
  const saved = getConfigKey<unknown>(key, undefined);
  if (typeof saved === "boolean") return saved;
  return fallback;
}

function loadSavedNotesDefaultOpenMode(): NotesPageOpenMode {
  const saved = getConfigKey<unknown>(NOTES_DEFAULT_OPEN_MODE_CONFIG_KEY, undefined);
  if (isNotesPageOpenMode(saved)) return saved;
  return DEFAULT_NOTES_PAGE_OPEN_MODE;
}

function loadSavedContactTrustDuration(key: string): ContactTrustDuration {
  const saved = getConfigKey<unknown>(key, undefined);
  return isContactTrustDuration(saved) ? saved : DEFAULT_CONTACT_TRUST_DURATION;
}

function loadSavedTitleBarVisibility(): TitleBarVisibility {
  const saved = getConfigKey<unknown>(TITLE_BAR_VISIBILITY_CONFIG_KEY, undefined);
  return parseTitleBarVisibility(saved);
}

let fontFamilyId = $state<FontFamilyId>(loadSavedFontFamilyId());
let fontScale = $state<number>(loadSavedFontScale());
let eventTimezoneDisplay = $state<EventTimezoneDisplay>(loadSavedEventTimezoneDisplay());
let profileDisplayName = $state<string>(loadSavedProfileDisplayName());
let profileFullName = $state<string>(loadSavedProfileFullName());
let profileImagePath = $state<string | null>(loadSavedProfileImagePath());
let profileUseImage = $state<boolean>(loadSavedProfileUseImage());
let profileColor = $state<EventColor>(loadSavedProfileColor());
let profileImageCrop = $state<ProfileImageCrop>(loadSavedProfileImageCrop());
let calendarTimeFormat = $state<CalendarTimeFormat>(loadSavedCalendarTimeFormat());
let calendarViewMode = $state<CalendarViewMode>(loadSavedCalendarViewMode());
let calendarDimPastEvents = $state<boolean>(loadSavedCalendarDimPastEvents());
let focusIdleThresholdMinutes = $state<number>(loadSavedFocusIdleThresholdMinutes());
let focusIdlePauseOnEventCreate = $state<boolean>(loadSavedFocusIdlePauseOnEventCreate());
let focusBreakFinishedRepeatSeconds = $state<FocusBreakSoundIntervalSeconds>(
  loadSavedFocusBreakFinishedRepeatSeconds(),
);
let focusBreakEndWarningSeconds = $state<FocusBreakSoundIntervalSeconds>(
  loadSavedFocusBreakEndWarningSeconds(),
);
let focusBreakEndEscPresses = $state<FocusBreakEndEscPresses>(
  loadSavedFocusBreakEndEscPresses(),
);
let focusBreakExtensionLimit = $state<FocusBreakExtensionLimit>(
  loadSavedFocusBreakExtensionLimit(),
);
let focusPauseNotificationIntervalMinutes = $state<FocusPauseNotificationIntervalMinutes>(
  loadSavedFocusPauseNotificationIntervalMinutes(),
);
let musicPauseOnPomodoroPause = $state<boolean>(loadSavedMusicPauseOnPomodoroPause());
let notesDefaultOpenMode = $state<NotesPageOpenMode>(loadSavedNotesDefaultOpenMode());
let notesMentionNotificationsEnabled = $state<boolean>(
  loadSavedBooleanPreference(
    NOTES_MENTION_NOTIFICATIONS_ENABLED_CONFIG_KEY,
    DEFAULT_NOTES_MENTION_NOTIFICATIONS_ENABLED,
  ),
);
let notesReminderNotificationsEnabled = $state<boolean>(
  loadSavedBooleanPreference(
    NOTES_REMINDER_NOTIFICATIONS_ENABLED_CONFIG_KEY,
    DEFAULT_NOTES_REMINDER_NOTIFICATIONS_ENABLED,
  ),
);
let notesUserMentionNotificationsEnabled = $state<boolean>(
  loadSavedBooleanPreference(
    NOTES_USER_MENTION_NOTIFICATIONS_ENABLED_CONFIG_KEY,
    DEFAULT_NOTES_USER_MENTION_NOTIFICATIONS_ENABLED,
  ),
);
let notesTaskMentionNotificationsEnabled = $state<boolean>(
  loadSavedBooleanPreference(
    NOTES_TASK_MENTION_NOTIFICATIONS_ENABLED_CONFIG_KEY,
    DEFAULT_NOTES_TASK_MENTION_NOTIFICATIONS_ENABLED,
  ),
);
let notesNotificationIncludeContent = $state<boolean>(
  loadSavedBooleanPreference(
    NOTES_NOTIFICATION_INCLUDE_CONTENT_CONFIG_KEY,
    DEFAULT_NOTES_NOTIFICATION_INCLUDE_CONTENT,
  ),
);
let notesShowPropertyIcons = $state<boolean>(
  loadSavedBooleanPreference(NOTES_SHOW_PROPERTY_ICONS_CONFIG_KEY, DEFAULT_NOTES_SHOW_PROPERTY_ICONS),
);
let projectsShowPropertyIcons = $state<boolean>(
  loadSavedBooleanPreference(PROJECTS_SHOW_PROPERTY_ICONS_CONFIG_KEY, DEFAULT_PROJECTS_SHOW_PROPERTY_ICONS),
);
let titleBarVisibility = $state<TitleBarVisibility>(loadSavedTitleBarVisibility());
let contactsInviteTrustDefault = $state<ContactTrustDuration>(
  loadSavedContactTrustDuration(CONTACTS_INVITE_TRUST_DEFAULT_CONFIG_KEY),
);
let contactsMessageTrustDefault = $state<ContactTrustDuration>(
  loadSavedContactTrustDuration(CONTACTS_MESSAGE_TRUST_DEFAULT_CONFIG_KEY),
);
const localization = getLocalization();

function applyPreferencesToDom(): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  root.style.setProperty("--font-family", resolveFontFamilyStack(fontFamilyId));
  root.style.setProperty("--font-scale", String(fontScale));
}

// Apply initial preferences on module load so first paint matches stored
// values. The CSS fallbacks keep the app readable if this runs late.
if (typeof document !== "undefined") {
  applyPreferencesToDom();
}

function setFontFamily(id: FontFamilyId): void {
  if (!getFontFamilyById(id)) return;
  fontFamilyId = id;
  applyPreferencesToDom();
  setConfigKey(FONT_FAMILY_CONFIG_KEY, id);
}

function setFontScale(value: number): void {
  const clamped = clampFontScale(value);
  fontScale = clamped;
  applyPreferencesToDom();
  setConfigKey(FONT_SCALE_CONFIG_KEY, clamped);
}

function setEventTimezoneDisplay(value: EventTimezoneDisplay): void {
  eventTimezoneDisplay = value;
  setConfigKey(EVENT_TIMEZONE_DISPLAY_CONFIG_KEY, value);
}

function setProfileDisplayName(value: string): boolean {
  const normalized = normalizeProfileDisplayName(value);
  if (!normalized.ok) return false;
  profileDisplayName = normalized.value;
  setConfigKey(
    PROFILE_DISPLAY_NAME_CONFIG_KEY,
    normalized.value ? normalized.value : undefined,
  );
  return true;
}

function setProfileFullName(value: string): boolean {
  const normalized = normalizeProfileFullName(value);
  if (!normalized.ok) return false;
  profileFullName = normalized.value;
  setConfigKey(
    PROFILE_FULL_NAME_CONFIG_KEY,
    normalized.value ? normalized.value : undefined,
  );
  return true;
}

/** Set the managed picture and its crop together; removing the picture resets the crop. */
function setProfileImagePath(
  value: string | null,
  crop: ProfileImageCrop = PROFILE_IMAGE_DEFAULT_CROP,
): boolean {
  if (value !== null && !isProfileImagePath(value)) return false;
  if (!isProfileImageCrop(crop)) return false;
  profileImagePath = value?.trim() ?? null;
  setConfigKey(PROFILE_IMAGE_PATH_CONFIG_KEY, profileImagePath ?? undefined);
  return setProfileImageCrop(profileImagePath === null ? PROFILE_IMAGE_DEFAULT_CROP : crop);
}

function setProfileImageCrop(value: ProfileImageCrop): boolean {
  if (!isProfileImageCrop(value)) return false;
  const crop = { x: value.x, y: value.y, zoom: value.zoom };
  profileImageCrop = crop;
  setConfigKey(PROFILE_IMAGE_CROP_CONFIG_KEY, isDefaultProfileImageCrop(crop) ? undefined : { ...crop });
  return true;
}

function setProfileUseImage(value: boolean): void {
  profileUseImage = value;
  setConfigKey(PROFILE_USE_IMAGE_CONFIG_KEY, value === DEFAULT_PROFILE_USE_IMAGE ? undefined : value);
}

function setProfileColor(value: EventColor): boolean {
  if (!isProfileColor(value)) return false;
  profileColor = value;
  setConfigKey(PROFILE_COLOR_CONFIG_KEY, value);
  return true;
}

function setContactsInviteTrustDefault(value: ContactTrustDuration): void {
  if (!isContactTrustDuration(value)) return;
  contactsInviteTrustDefault = value;
  setConfigKey(CONTACTS_INVITE_TRUST_DEFAULT_CONFIG_KEY, value === DEFAULT_CONTACT_TRUST_DURATION ? undefined : value);
}

function setContactsMessageTrustDefault(value: ContactTrustDuration): void {
  if (!isContactTrustDuration(value)) return;
  contactsMessageTrustDefault = value;
  setConfigKey(CONTACTS_MESSAGE_TRUST_DEFAULT_CONFIG_KEY, value === DEFAULT_CONTACT_TRUST_DURATION ? undefined : value);
}

function setCalendarTimeFormat(value: CalendarTimeFormat): void {
  if (!isCalendarTimeFormat(value)) return;
  calendarTimeFormat = value;
  setConfigKey(CALENDAR_TIME_FORMAT_CONFIG_KEY, value);
}

function setCalendarViewMode(value: CalendarViewMode): void {
  if (!isCalendarViewMode(value)) return;
  if (value === calendarViewMode) return;
  calendarViewMode = value;
  setConfigKey(CALENDAR_VIEW_MODE_CONFIG_KEY, value);
}

function setCalendarDimPastEvents(value: boolean): void {
  calendarDimPastEvents = value;
  setConfigKey(CALENDAR_DIM_PAST_EVENTS_CONFIG_KEY, value);
}

function setFocusIdleThresholdMinutes(value: number): void {
  const clamped = clampFocusIdleThresholdMinutes(value);
  focusIdleThresholdMinutes = clamped;
  setConfigKey(FOCUS_IDLE_THRESHOLD_MINUTES_CONFIG_KEY, clamped);
}

function setFocusIdlePauseOnEventCreate(value: boolean): void {
  focusIdlePauseOnEventCreate = value;
  setConfigKey(FOCUS_IDLE_PAUSE_ON_EVENT_CREATE_CONFIG_KEY, value);
}

function setFocusBreakFinishedRepeatSeconds(value: number): void {
  const parsed = parseFocusBreakSoundIntervalSeconds(
    value,
    DEFAULT_FOCUS_BREAK_FINISHED_REPEAT_SECONDS,
  );
  focusBreakFinishedRepeatSeconds = parsed;
  setConfigKey(FOCUS_BREAK_FINISHED_REPEAT_SECONDS_CONFIG_KEY, parsed);
}

function setFocusBreakEndWarningSeconds(value: number): void {
  const parsed = parseFocusBreakSoundIntervalSeconds(
    value,
    DEFAULT_FOCUS_BREAK_END_WARNING_SECONDS,
  );
  focusBreakEndWarningSeconds = parsed;
  setConfigKey(FOCUS_BREAK_END_WARNING_SECONDS_CONFIG_KEY, parsed);
}

function setFocusBreakEndEscPresses(value: number | null): void {
  const parsed = parseFocusBreakEndEscPresses(
    value,
    DEFAULT_FOCUS_BREAK_END_ESC_PRESSES,
  );
  focusBreakEndEscPresses = parsed;
  setConfigKey(FOCUS_BREAK_END_ESC_PRESSES_CONFIG_KEY, parsed);
}

function setFocusBreakExtensionLimit(value: number | null): void {
  const parsed = parseFocusBreakExtensionLimit(
    value,
    DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT,
  );
  focusBreakExtensionLimit = parsed;
  setConfigKey(FOCUS_BREAK_EXTENSION_LIMIT_CONFIG_KEY, parsed);
}

function setFocusPauseNotificationIntervalMinutes(value: number): void {
  const parsed = parseFocusPauseNotificationIntervalMinutes(
    value,
    DEFAULT_FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES,
  );
  focusPauseNotificationIntervalMinutes = parsed;
  setConfigKey(FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES_CONFIG_KEY, parsed);
}

function setMusicPauseOnPomodoroPause(value: boolean): void {
  musicPauseOnPomodoroPause = value;
  setConfigKey(MUSIC_PAUSE_ON_POMODORO_PAUSE_CONFIG_KEY, value);
}

function setNotesDefaultOpenMode(value: NotesPageOpenMode): void {
  if (!isNotesPageOpenMode(value)) return;
  notesDefaultOpenMode = value;
  setConfigKey(NOTES_DEFAULT_OPEN_MODE_CONFIG_KEY, value);
}

function setNotesMentionNotificationsEnabled(value: boolean): void {
  notesMentionNotificationsEnabled = value;
  setConfigKey(NOTES_MENTION_NOTIFICATIONS_ENABLED_CONFIG_KEY, value);
}

function setNotesReminderNotificationsEnabled(value: boolean): void {
  notesReminderNotificationsEnabled = value;
  setConfigKey(NOTES_REMINDER_NOTIFICATIONS_ENABLED_CONFIG_KEY, value);
}

function setNotesUserMentionNotificationsEnabled(value: boolean): void {
  notesUserMentionNotificationsEnabled = value;
  setConfigKey(NOTES_USER_MENTION_NOTIFICATIONS_ENABLED_CONFIG_KEY, value);
}

function setNotesTaskMentionNotificationsEnabled(value: boolean): void {
  notesTaskMentionNotificationsEnabled = value;
  setConfigKey(NOTES_TASK_MENTION_NOTIFICATIONS_ENABLED_CONFIG_KEY, value);
}

function setNotesNotificationIncludeContent(value: boolean): void {
  notesNotificationIncludeContent = value;
  setConfigKey(NOTES_NOTIFICATION_INCLUDE_CONTENT_CONFIG_KEY, value);
}

function setNotesShowPropertyIcons(value: boolean): void {
  notesShowPropertyIcons = value;
  setConfigKey(NOTES_SHOW_PROPERTY_ICONS_CONFIG_KEY, value);
}

function setProjectsShowPropertyIcons(value: boolean): void {
  projectsShowPropertyIcons = value;
  setConfigKey(PROJECTS_SHOW_PROPERTY_ICONS_CONFIG_KEY, value);
}

function setTitleBarControlVisible(id: TitleBarControlId, visible: boolean): void {
  if (!isTitleBarControlId(id)) return;
  titleBarVisibility = { ...titleBarVisibility, [id]: visible };
  setConfigKey(TITLE_BAR_VISIBILITY_CONFIG_KEY, titleBarVisibility);
}

function toggleTitleBarControl(id: TitleBarControlId): void {
  setTitleBarControlVisible(id, !titleBarVisibility[id]);
}

function resetTitleBarVisibility(): void {
  titleBarVisibility = { ...DEFAULT_TITLE_BAR_VISIBILITY };
  setConfigKey(TITLE_BAR_VISIBILITY_CONFIG_KEY, titleBarVisibility);
}

/**
 * Access app-wide user preferences. Returns getters so Svelte's reactivity
 * picks up changes in consuming components.
 */
export function getPreferences() {
  return {
    get fontFamilyId(): FontFamilyId {
      return fontFamilyId;
    },
    get fontScale(): number {
      return fontScale;
    },
    /** List of curated font family options for building a picker. */
    get fontFamilies(): readonly (typeof FONT_FAMILIES)[number][] {
      return FONT_FAMILIES;
    },
    get eventTimezoneDisplay(): EventTimezoneDisplay {
      return eventTimezoneDisplay;
    },
    get profileDisplayName(): string {
      return profileDisplayName;
    },
    get profileFullName(): string {
      return profileFullName;
    },
    get profileImagePath(): string | null {
      return profileImagePath;
    },
    get profileUseImage(): boolean {
      return profileUseImage;
    },
    /** Picture the avatar shows, or null when there is none or the color is preferred. */
    get profileAvatarImagePath(): string | null {
      return profileUseImage ? profileImagePath : null;
    },
    /** Theme palette slot behind the avatar initials. */
    get profileColor(): EventColor {
      return profileColor;
    },
    /** Square crop applied to the profile picture wherever the avatar renders. */
    get profileImageCrop(): ProfileImageCrop {
      return profileImageCrop;
    },
    get languagePreference(): LanguagePreference {
      return localization.languagePreference;
    },
    get languagePreferences(): readonly (typeof LANGUAGE_PREFERENCES)[number][] {
      return LANGUAGE_PREFERENCES;
    },
    get calendarTimeFormat(): CalendarTimeFormat {
      return calendarTimeFormat;
    },
    get calendarViewMode(): CalendarViewMode {
      return calendarViewMode;
    },
    get calendarDimPastEvents(): boolean {
      return calendarDimPastEvents;
    },
    get focusIdleThresholdMinutes(): number {
      return focusIdleThresholdMinutes;
    },
    get focusIdlePauseOnEventCreate(): boolean {
      return focusIdlePauseOnEventCreate;
    },
    get focusBreakFinishedRepeatSeconds(): FocusBreakSoundIntervalSeconds {
      return focusBreakFinishedRepeatSeconds;
    },
    get focusBreakEndWarningSeconds(): FocusBreakSoundIntervalSeconds {
      return focusBreakEndWarningSeconds;
    },
    get focusBreakEndEscPresses(): FocusBreakEndEscPresses {
      return focusBreakEndEscPresses;
    },
    get focusBreakExtensionLimit(): FocusBreakExtensionLimit {
      return focusBreakExtensionLimit;
    },
    get focusPauseNotificationIntervalMinutes(): FocusPauseNotificationIntervalMinutes {
      return focusPauseNotificationIntervalMinutes;
    },
    get musicPauseOnPomodoroPause(): boolean {
      return musicPauseOnPomodoroPause;
    },
    get notesDefaultOpenMode(): NotesPageOpenMode {
      return notesDefaultOpenMode;
    },
    get notesMentionNotificationsEnabled(): boolean {
      return notesMentionNotificationsEnabled;
    },
    get notesReminderNotificationsEnabled(): boolean {
      return notesReminderNotificationsEnabled;
    },
    get notesUserMentionNotificationsEnabled(): boolean {
      return notesUserMentionNotificationsEnabled;
    },
    get notesTaskMentionNotificationsEnabled(): boolean {
      return notesTaskMentionNotificationsEnabled;
    },
    get notesNotificationIncludeContent(): boolean {
      return notesNotificationIncludeContent;
    },
    /** Whether Notes database column headers show their property type icon. */
    get notesShowPropertyIcons(): boolean {
      return notesShowPropertyIcons;
    },
    /** Whether Projects List column headers show their property type icon. */
    get projectsShowPropertyIcons(): boolean {
      return projectsShowPropertyIcons;
    },
    get titleBarVisibility(): TitleBarVisibility {
      return titleBarVisibility;
    },
    /** Trust scope every new contact starts with for inviting the local person. */
    get contactsInviteTrustDefault(): ContactTrustDuration {
      return contactsInviteTrustDefault;
    },
    /** Trust scope every new contact starts with for messaging the local person directly. */
    get contactsMessageTrustDefault(): ContactTrustDuration {
      return contactsMessageTrustDefault;
    },
    setFontFamily,
    setFontScale,
    setProfileDisplayName,
    setProfileFullName,
    setProfileImagePath,
    setProfileUseImage,
    setProfileColor,
    setProfileImageCrop,
    setLanguagePreference: localization.setLanguagePreference,
    setEventTimezoneDisplay,
    setCalendarTimeFormat,
    setCalendarViewMode,
    setCalendarDimPastEvents,
    setFocusIdleThresholdMinutes,
    setFocusIdlePauseOnEventCreate,
    setFocusBreakFinishedRepeatSeconds,
    setFocusBreakEndWarningSeconds,
    setFocusBreakEndEscPresses,
    setFocusBreakExtensionLimit,
    setFocusPauseNotificationIntervalMinutes,
    setMusicPauseOnPomodoroPause,
    setNotesDefaultOpenMode,
    setNotesMentionNotificationsEnabled,
    setNotesReminderNotificationsEnabled,
    setNotesUserMentionNotificationsEnabled,
    setNotesTaskMentionNotificationsEnabled,
    setNotesNotificationIncludeContent,
    setNotesShowPropertyIcons,
    setProjectsShowPropertyIcons,
    setTitleBarControlVisible,
    toggleTitleBarControl,
    setContactsInviteTrustDefault,
    setContactsMessageTrustDefault,
    resetFontFamily() {
      setFontFamily(DEFAULT_FONT_FAMILY_ID);
    },
    resetFontScale() {
      setFontScale(DEFAULT_FONT_SCALE);
    },
    resetEventTimezoneDisplay() {
      setEventTimezoneDisplay(DEFAULT_EVENT_TIMEZONE_DISPLAY);
    },
    resetCalendarTimeFormat() {
      setCalendarTimeFormat(DEFAULT_CALENDAR_TIME_FORMAT);
    },
    resetCalendarViewMode() {
      setCalendarViewMode(DEFAULT_CALENDAR_VIEW_MODE);
    },
    resetCalendarDimPastEvents() {
      setCalendarDimPastEvents(DEFAULT_CALENDAR_DIM_PAST_EVENTS);
    },
    resetFocusIdleThresholdMinutes() {
      setFocusIdleThresholdMinutes(DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES);
    },
    resetFocusIdlePauseOnEventCreate() {
      setFocusIdlePauseOnEventCreate(DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE);
    },
    resetFocusBreakFinishedRepeatSeconds() {
      setFocusBreakFinishedRepeatSeconds(DEFAULT_FOCUS_BREAK_FINISHED_REPEAT_SECONDS);
    },
    resetFocusBreakEndWarningSeconds() {
      setFocusBreakEndWarningSeconds(DEFAULT_FOCUS_BREAK_END_WARNING_SECONDS);
    },
    resetFocusBreakEndEscPresses() {
      setFocusBreakEndEscPresses(DEFAULT_FOCUS_BREAK_END_ESC_PRESSES);
    },
    resetFocusBreakExtensionLimit() {
      setFocusBreakExtensionLimit(DEFAULT_FOCUS_BREAK_EXTENSION_LIMIT);
    },
    resetFocusPauseNotificationIntervalMinutes() {
      setFocusPauseNotificationIntervalMinutes(
        DEFAULT_FOCUS_PAUSE_NOTIFICATION_INTERVAL_MINUTES,
      );
    },
    resetMusicPauseOnPomodoroPause() {
      setMusicPauseOnPomodoroPause(DEFAULT_MUSIC_PAUSE_ON_POMODORO_PAUSE);
    },
    resetNotesDefaultOpenMode() {
      setNotesDefaultOpenMode(DEFAULT_NOTES_PAGE_OPEN_MODE);
    },
    resetNotesMentionNotificationsEnabled() {
      setNotesMentionNotificationsEnabled(DEFAULT_NOTES_MENTION_NOTIFICATIONS_ENABLED);
    },
    resetNotesReminderNotificationsEnabled() {
      setNotesReminderNotificationsEnabled(DEFAULT_NOTES_REMINDER_NOTIFICATIONS_ENABLED);
    },
    resetNotesUserMentionNotificationsEnabled() {
      setNotesUserMentionNotificationsEnabled(DEFAULT_NOTES_USER_MENTION_NOTIFICATIONS_ENABLED);
    },
    resetNotesTaskMentionNotificationsEnabled() {
      setNotesTaskMentionNotificationsEnabled(DEFAULT_NOTES_TASK_MENTION_NOTIFICATIONS_ENABLED);
    },
    resetNotesNotificationIncludeContent() {
      setNotesNotificationIncludeContent(DEFAULT_NOTES_NOTIFICATION_INCLUDE_CONTENT);
    },
    resetNotesShowPropertyIcons() {
      setNotesShowPropertyIcons(DEFAULT_NOTES_SHOW_PROPERTY_ICONS);
    },
    resetProjectsShowPropertyIcons() {
      setProjectsShowPropertyIcons(DEFAULT_PROJECTS_SHOW_PROPERTY_ICONS);
    },
    resetTitleBarVisibility,
  };
}
