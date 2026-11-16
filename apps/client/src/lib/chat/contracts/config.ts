import type {
  ProjectWorkingFolderId,
  CredentialReferenceId,
  InteractionMode,
  ModelId,
  ProviderFamilyId,
  ProviderInstanceId,
  SafetyMode,
  VersionedJson,
} from "./common";
import type { ModelOptionSelection } from "./provider";

export const CHAT_SEND_KEYS = ["enter", "mod_enter"] as const;
export type ChatSendKey = (typeof CHAT_SEND_KEYS)[number];

/** How much vertical space the Chat timeline and channel feed give each row. */
export const CHAT_TIMELINE_DENSITIES = ["comfortable", "compact"] as const;
export type ChatTimelineDensity = (typeof CHAT_TIMELINE_DENSITIES)[number];

export interface ChatPortableProviderConfig {
  schemaVersion: number;
  instanceId: ProviderInstanceId;
  familyId: ProviderFamilyId;
  label: string;
  enabled: boolean;
  launchArguments: string[];
  environment: Record<string, string>;
  credentialReferences: Record<string, CredentialReferenceId>;
  visibleModelIds: ModelId[];
  favoriteModelIds: ModelId[];
  providerConfig: VersionedJson;
  [unknownField: string]: unknown;
}

export interface RememberedComposerSelection {
  workingFolderId: ProjectWorkingFolderId;
  providerInstanceId: ProviderInstanceId;
  modelId: ModelId | null;
  providerManagedModel: boolean;
  modelOptions: ModelOptionSelection[];
  safetyMode: SafetyMode;
  interactionMode: InteractionMode;
}

/** Smallest, default, and largest width, in CSS pixels, of a resizable Chat side column. */
export interface ChatPanelWidthBounds {
  minimum: number;
  default: number;
  maximum: number;
}

/** Width bounds of the column workspace inspector. */
export const CHAT_INSPECTOR_WIDTH_BOUNDS: ChatPanelWidthBounds = { minimum: 240, default: 520, maximum: 960 };

/** Width bounds of the reply thread column beside the channel feed. */
export const CHAT_REPLY_THREAD_WIDTH_BOUNDS: ChatPanelWidthBounds = { minimum: 280, default: 440, maximum: 800 };

/** Device-local widths of the resizable Chat side columns, in CSS pixels. */
export interface ChatPanelPreferences {
  inspectorWidthPx: number;
  replyThreadWidthPx: number;
}

/**
 * Chat behavior, stored as `chat.behavior.*` preferences. Fields are portable vault preferences
 * that replicate, except `confirmMultilineTerminalPaste` and `notificationIncludeContent`, which
 * are stored per device.
 */
export interface ChatBehaviorPreferences {
  sendKey: ChatSendKey;
  restoreLastSelectedThread: boolean;
  showReasoningSummaries: boolean;
  automaticallyFoldSettledWork: boolean;
  timelineDensity: ChatTimelineDensity;
  terminalScrollbackLines: number;
  idleSessionTimeoutSeconds: number;
  confirmMultilineTerminalPaste: boolean;
  /** Sends a system notification when work waits for an answer or an approval. */
  notifyWaitingForYou: boolean;
  /** Sends a system notification when work is ready for review. */
  notifyReviewReady: boolean;
  /** Sends a system notification when work fails. */
  notifyFailures: boolean;
  /** Announces attention changes of reply threads that are not selected to screen readers. */
  announceBackgroundWork: boolean;
  /** Includes the failure reason in notifications; stored per device for lock screen privacy. */
  notificationIncludeContent: boolean;
}

/**
 * Chat settings as one read: the device-local fields (providers, remembered selections,
 * working folder provider preferences, panels) stay in this device's preference file and never
 * replicate; `behavior` is portable apart from its per-device fields.
 */
export interface ChatConfiguration {
  providers: ChatPortableProviderConfig[];
  automaticProviderSetupDisabled: ProviderFamilyId[];
  rememberedSelections: RememberedComposerSelection[];
  workingFolderProviderPreferences: Record<ProjectWorkingFolderId, ProviderInstanceId>;
  panels: ChatPanelPreferences;
  behavior: ChatBehaviorPreferences;
}
