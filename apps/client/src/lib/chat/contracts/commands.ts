import type {
  ApprovalDecisionKind,
  ChatAttachmentId,
  ChatCheckpointId,
  ChatCommandId,
  ChatRequestId,
  ChatThreadId,
  ChatTurnId,
  ProjectWorkingFolderId,
  ContinuationGroupId,
  ModelId,
  ProviderInstanceId,
  ProviderRequestId,
  ProviderSessionId,
  ProviderThreadId,
  RepositoryKind,
  TurnModeSnapshot,
  VersionedJson,
} from "./common";
import type { ModelOptionSelection } from "./provider";

export interface ChatCommandContext {
  clientCommandId: ChatCommandId;
  expectedThreadRevision: number | null;
}

export interface VerifiedWorkspaceContext {
  workingFolderId: ProjectWorkingFolderId;
  canonicalPath: string;
  repositoryKind: RepositoryKind;
  repositoryIdentity: string | null;
  projectConfiguration: ProjectConfigurationAccess;
}

/** Whether a provider loads the workspace's project configuration that can run commands. */
export type ProjectConfigurationAccess = "trusted" | "restricted";

export interface PromptAttachmentReference {
  attachmentId: ChatAttachmentId;
  kind: string;
  displayName: string;
  managedRelativePath: string;
  mimeType: string | null;
  byteSize: number;
  localPath: string | null;
  textContent: string | null;
}

export interface WorkspaceMentionReference {
  relativePath: string;
  kind: string;
}

export interface StartSessionRequest {
  threadId: ChatThreadId;
  workspace: VerifiedWorkspaceContext;
  providerInstanceId: ProviderInstanceId;
  modes: TurnModeSnapshot;
  modelId: ModelId | null;
  modelOptions: ModelOptionSelection[];
  /** Instructions the provider places at system level when the session opens. */
  developerInstructions: string | null;
}

export interface ResumeSessionRequest {
  threadId: ChatThreadId;
  workspace: VerifiedWorkspaceContext;
  providerInstanceId: ProviderInstanceId;
  providerThreadId: ProviderThreadId;
  continuationGroupId: ContinuationGroupId;
  resumeCursor: VersionedJson;
  modes: TurnModeSnapshot;
  /** Instructions the provider places at system level when the session resumes. */
  developerInstructions: string | null;
  /** Whether the native session already received `developerInstructions` on an earlier turn. */
  developerInstructionsDelivered: boolean;
}

export interface SendTurnRequest {
  command: ChatCommandContext;
  sessionId: ProviderSessionId;
  turnId: ChatTurnId;
  prompt: string;
  attachments: PromptAttachmentReference[];
  mentions: WorkspaceMentionReference[];
  modelId: ModelId | null;
  modelOptions: ModelOptionSelection[];
  modes: TurnModeSnapshot;
  developerInstructions: string | null;
}

export interface SteerTurnRequest {
  command: ChatCommandContext;
  sessionId: ProviderSessionId;
  turnId: ChatTurnId;
  prompt: string;
}

export interface InterruptTurnRequest {
  command: ChatCommandContext;
  sessionId: ProviderSessionId;
  turnId: ChatTurnId;
}

export interface ApprovalDecision {
  kind: ApprovalDecisionKind;
  providerOptionId: string | null;
  updatedToolInput: VersionedJson | null;
}

export interface ResolveApprovalRequest {
  command: ChatCommandContext;
  sessionId: ProviderSessionId;
  requestId: ChatRequestId;
  providerRequestId: ProviderRequestId;
  decision: ApprovalDecision;
}

export interface UserInputAnswer {
  questionId: string;
  selectedOptionIds: string[];
  freeFormText: string | null;
  /** Managed attachments for the answer; the app passes them to the provider as `Attached:` lines. */
  attachmentIds?: ChatAttachmentId[];
}

export interface ResolveUserInputRequest {
  command: ChatCommandContext;
  sessionId: ProviderSessionId;
  requestId: ChatRequestId;
  providerRequestId: ProviderRequestId;
  answers: UserInputAnswer[];
}

export interface RollbackRequest {
  command: ChatCommandContext;
  sessionId: ProviderSessionId;
  checkpointId: ChatCheckpointId | null;
  providerCursor: VersionedJson | null;
  targetTurnId: ChatTurnId | null;
}

export interface ReadHistoryRequest {
  sessionId: ProviderSessionId;
  cursor: string | null;
  limit: number;
}

export interface StopSessionRequest {
  sessionId: ProviderSessionId;
  force: boolean;
}

export interface ContinuationGroupRequest {
  providerInstanceId: ProviderInstanceId;
  normalizedProviderHome: string | null;
  accountIdentity: string | null;
  serverIdentity: string | null;
  providerFields: Record<string, string>;
}
