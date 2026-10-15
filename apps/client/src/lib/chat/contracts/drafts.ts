import type {
  ChatAttachmentId,
  ChatThreadId,
  ProjectWorkingFolderId,
  InteractionMode,
  ProviderInstanceId,
  SafetyMode,
  UtcTimestamp,
  VersionedJson,
} from "./common";

export interface ChatDraftMention {
  relativePath: string;
  kind: "file" | "directory";
  ignored: boolean;
}

export interface ChatDraftRead {
  id: string;
  workingFolderId: ProjectWorkingFolderId;
  threadId: ChatThreadId | null;
  text: string;
  richContent: VersionedJson | null;
  attachmentIds: ChatAttachmentId[];
  mentions: VersionedJson;
  providerInstanceId: ProviderInstanceId | null;
  modelSelection: VersionedJson | null;
  safetyMode: SafetyMode | null;
  interactionMode: InteractionMode | null;
  sentSnapshot: VersionedJson | null;
  updatedAt: UtcTimestamp;
}

/** The stored draft a save was based on, so a save cannot silently replace edits from another window. */
export type ChatDraftBase =
  | { kind: "any" }
  | { kind: "none" }
  | { kind: "saved"; updatedAt: UtcTimestamp };

export type SaveChatDraftRequest = Omit<ChatDraftRead, "updatedAt"> & { base: ChatDraftBase };
