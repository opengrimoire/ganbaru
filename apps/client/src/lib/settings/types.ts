/**
 * Shared types for the Settings modal. Kept outside the component so external
 * launchers, such as the title bar and the Settings launcher store, type their requests against the
 * same identifier set.
 */
export const SETTINGS_SECTION_IDS = [
  "appearance",
  "profile",
  "contacts",
  "calendars",
  "projects",
  "notes",
  "chat",
  "focus",
  "music",
  "distractions",
  "data",
  "updates",
  "shortcuts",
  "about",
] as const;

export type SectionId = (typeof SETTINGS_SECTION_IDS)[number];

export type DistractionsSettingsTab = "limits" | "browser" | "mobile" | "desktop";
export type ChatSettingsSubsection = "teammates" | "providers" | "permissions" | "behavior";
export type ContactsSettingsTab = "card" | "contacts" | "invitations" | "blocked";

/** Unsaved Settings edit that must be confirmed before leaving its section or closing Settings. */
export type SettingsDraftKind = "teammate" | "profile";

export type DistractionsLimitEditorTarget =
  | { mode: "create" }
  | { mode: "edit"; limitId: string };

export const NOTES_TRANSFER_OPERATIONS = [
  "html-import",
  "notion-api-import",
  "notion-export-import",
  "json-graph-export",
] as const;

export type NotesTransferOperation = (typeof NOTES_TRANSFER_OPERATIONS)[number];

export type ChatProviderSetupTarget =
  | { mode: "create"; familyId: string }
  | { mode: "edit"; instanceId: string };

export const SETTINGS_DETAIL_KINDS = ["distractions-limit", "notes-transfer", "chat-provider"] as const;
export type SettingsDetailKind = (typeof SETTINGS_DETAIL_KINDS)[number];
