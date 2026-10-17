import type {
  ChatCheckpointId,
  ChatThreadId,
  ChatTurnId,
  ProjectWorkingFolderId,
  UtcTimestamp,
} from "./common";

export type ChatInspectorTab = "plan" | "files" | "sourceControl" | "browser" | "review" | "terminal";
export type ChatChangedFileStatus = "added" | "modified" | "deleted" | "renamed" | "type_changed" | "unknown";

export type ReviewDiffSource =
  | { kind: "working_tree"; mode: "staged" | "unstaged" | "all" }
  | { kind: "checkpoint"; range: "turn" | "thread"; turnId: ChatTurnId | null }
  | { kind: "commit"; revision: string }
  | { kind: "branch"; baseRef: string | null; headRef: string; comparison: "merge_base" | "direct" }
  | { kind: "provider_turn"; turnId: ChatTurnId }
  | {
    kind: "change_request";
    provider: "github" | "gitlab" | "azure_devops" | "bitbucket";
    repositorySlug: string;
    number: number;
  };

export interface OpenChatReviewRequest {
  threadId: ChatThreadId | null;
  workingFolderId: ProjectWorkingFolderId;
  executionEnvironmentId: string | null;
  source: ReviewDiffSource;
  ignoreWhitespace: boolean;
  contextLines: number;
  preferredRelativePath: string | null;
}

export interface ChatReviewTotalsRead {
  files: number;
  additions: number;
  deletions: number;
}

export interface ChatReviewFileFlags {
  binary: boolean;
  submodule: boolean;
  conflict: boolean;
  modeOnly: boolean;
  pureRename: boolean;
  untracked: boolean;
  symlink: boolean;
  providerReported: boolean;
  gitObserved: boolean;
  readOnly: boolean;
}

export type ChatReviewFileAction = "stage" | "unstage" | "discard" | "comment" | "openEditor";

export interface ChatReviewFileCapabilities {
  stage: boolean;
  unstage: boolean;
  discard: boolean;
  comment: boolean;
  openEditor: boolean;
}

export interface ChatReviewFileRead {
  fileId: string;
  relativePath: string;
  previousRelativePath: string | null;
  status: ChatChangedFileStatus;
  additions: number | null;
  deletions: number | null;
  flags: ChatReviewFileFlags;
  capabilities: ChatReviewFileCapabilities;
  capabilityReasons: Partial<Record<ChatReviewFileAction, string>>;
}

export type ChatReviewPatchState = "complete" | "partial" | "binary" | "oversized_hunk" | "unavailable";

export interface ChatReviewPatchHunkRead {
  hunkId: string;
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
  state: ChatReviewPatchState;
}

export interface ChatReviewPatchRead {
  fileId: string;
  patch: string | null;
  hunks: ChatReviewPatchHunkRead[];
  continuationCursor: string | null;
  state: ChatReviewPatchState;
}

export interface ChatReviewSnapshotRead {
  snapshotId: string;
  reviewRevision: string;
  source: ReviewDiffSource;
  sourceLabel: string;
  files: ChatReviewFileRead[];
  totals: ChatReviewTotalsRead;
  preferredPatch: ChatReviewPatchRead | null;
  freshness: "current" | "outdated";
}

export interface ReadChatReviewPatchesRequest {
  threadId: ChatThreadId | null;
  workingFolderId: ProjectWorkingFolderId;
  executionEnvironmentId: string | null;
  snapshotId: string;
  reviewRevision: string;
  fileIds: string[];
  continuationCursor: string | null;
  byteLimit?: number | null;
}

export interface ChatReviewPatchPageRead {
  patches: ChatReviewPatchRead[];
  continuationCursor: string | null;
}

export interface ApplyChatReviewActionRequest {
  threadId: ChatThreadId | null;
  workingFolderId: ProjectWorkingFolderId;
  executionEnvironmentId: string | null;
  snapshotId: string;
  expectedReviewRevision: string;
  operation: "stage" | "unstage" | "discard";
  fileId: string | null;
  hunkIds: string[];
  confirmed: boolean;
  clientOperationId: string;
}

export interface ApplyChatReviewActionResult {
  snapshot: ChatReviewSnapshotRead;
}

export interface GitChangedPathRead {
  relativePath: string;
  originalRelativePath: string | null;
  indexStatus: string;
  worktreeStatus: string;
  untracked: boolean;
  ignored: boolean;
}

export interface GitStatusRead {
  branch: string | null;
  detached: boolean;
  upstream: string | null;
  ahead: number;
  behind: number;
  files: GitChangedPathRead[];
}

export interface GitRemoteRead {
  name: string;
  fetchUrl: string | null;
  pushUrl: string | null;
}

export interface GitBranchRead {
  name: string;
  current: boolean;
  upstream: string | null;
  ahead: number;
  behind: number;
}

export interface GitWorktreeRead {
  path: string;
  head: string;
  branch: string | null;
  bare: boolean;
  detached: boolean;
  locked: boolean;
  prunable: boolean;
}

export interface ChatExecutionEnvironmentRead {
  id: string;
  workingFolderId: ProjectWorkingFolderId;
  kind: "current_folder" | "worktree";
  displayName: string;
  lifecycleState: string;
  branchName: string | null;
  baseReference: string | null;
  remoteName: string | null;
  cleanupState: string | null;
  localPath: string | null;
  createdAt: UtcTimestamp;
  updatedAt: UtcTimestamp;
}

export interface CreateChatWorktreeRequest {
  environmentId: string;
  workingFolderId: ProjectWorkingFolderId;
  displayName: string;
  branchName: string;
  baseReference: string;
  remoteName: string | null;
  fetchRemote: boolean;
}

export interface HostedSourceControlRead {
  kind: "github" | "gitlab" | "azure_devops" | "bitbucket";
  label: string;
  detectedForRepository: boolean;
  remoteName: string | null;
  repositorySlug: string | null;
  status: string;
  version: string | null;
  unavailableReason: string | null;
  configurationHint: string | null;
}

export interface HostedChangeRequestRead {
  providerKind: HostedSourceControlRead["kind"];
  number: number;
  title: string;
  url: string;
  state: string;
  baseBranch: string;
  headBranch: string;
  author: string | null;
  draft: boolean;
}

export interface CreateHostedChangeRequest {
  workingFolderId: ProjectWorkingFolderId;
  executionEnvironmentId: string | null;
  providerKind: HostedSourceControlRead["kind"];
  repositorySlug: string;
  title: string;
  body: string;
  baseBranch: string;
  headBranch: string;
  draft: boolean;
}

export interface BrowserTabBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface BrowserTabRead {
  threadId: ChatThreadId;
  tabId: string;
  currentUrl: string;
  title: string;
  visible: boolean;
  loading: boolean;
  viewportWidth: number;
  viewportHeight: number;
  externalOrigin: boolean;
}

export type ChatReviewCommentState = "open" | "resolved";
export type ChatReviewCommentSourceKind = ReviewDiffSource["kind"] | "file";

export interface ChatReviewCommentRead {
  id: string;
  threadId: ChatThreadId;
  relativePath: string;
  contentRevision: string;
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
  selectedText: string;
  commentText: string;
  state: ChatReviewCommentState;
  createdAt: UtcTimestamp;
  updatedAt: UtcTimestamp;
  resolvedAt: UtcTimestamp | null;
  sourceKind?: ChatReviewCommentSourceKind;
  sourceData?: ReviewDiffSource;
  snapshotId?: string | null;
  reviewRevision?: string | null;
  selectionSide?: "file" | "old" | "new";
  previousRelativePath?: string | null;
  applicability?: "current" | "outdated" | "source_unavailable";
}

export interface CreateChatReviewCommentRequest {
  id: string;
  threadId: ChatThreadId;
  relativePath: string;
  contentRevision: string;
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
  selectedText: string;
  commentText: string;
  sourceKind?: ChatReviewCommentSourceKind;
  sourceData?: ReviewDiffSource;
  snapshotId?: string | null;
  reviewRevision?: string | null;
  fileId?: string | null;
  selectionSide?: "file" | "old" | "new";
  previousRelativePath?: string | null;
}

export interface ProjectWorkingFolderFileEntry {
  relativePath: string;
  displayName: string;
  kind: "file" | "directory";
  ignored: boolean;
  byteSize: number | null;
}

export interface ProjectWorkingFolderDirectoryRead {
  relativePath: string;
  entries: ProjectWorkingFolderFileEntry[];
  truncated: boolean;
}

export interface ProjectWorkingFolderFilePreview {
  relativePath: string;
  displayName: string;
  text: string | null;
  lineCount: number | null;
  byteSize: number;
  binary: boolean;
  oversized: boolean;
  contentRevision: string | null;
}

export type ChatWorkspaceObserverMode = "native" | "polling" | "unavailable";

export interface ChatWorkspaceObserverStatusRead {
  workingFolderId: ProjectWorkingFolderId;
  executionEnvironmentId: string | null;
  generation: number;
  mode: ChatWorkspaceObserverMode;
  degradedReason: string | null;
}

export interface ChatWorkspaceRename {
  previousRelativePath: string;
  relativePath: string;
}

export interface ChatWorkspaceChangeBatch {
  workingFolderId: ProjectWorkingFolderId;
  executionEnvironmentId: string | null;
  generation: number;
  relativePaths: string[];
  affectedParentDirectories: string[];
  renames: ChatWorkspaceRename[];
  gitMetadataChanged: boolean;
  overflowed: boolean;
  degradedReason: string | null;
}

export interface ChatChangedFileRead {
  relativePath: string;
  previousRelativePath: string | null;
  status: ChatChangedFileStatus;
  additions: number | null;
  deletions: number | null;
  binary: boolean;
  providerReported: boolean;
  gitObserved: boolean;
}

export interface ChatRestorePreviewRead {
  previewId: string;
  checkpointId: ChatCheckpointId;
  expiresAt: UtcTimestamp;
  files: ChatChangedFileRead[];
  stagedChanges: boolean;
  providerRollback: "supported" | "unsupported";
  warnings: string[];
}

export interface ChatRestoreResultRead {
  checkpointId: ChatCheckpointId;
  revertedTurnIds: ChatTurnId[];
  providerHistoryAction: "rolled_back" | "fork_required";
  recoveryState: "complete" | "recovered" | "recovery_required";
  threadRevision: number;
}

export interface ChatTerminalRead {
  id: string;
  threadId: ChatThreadId;
  workingFolderId: ProjectWorkingFolderId;
  name: string;
  shell: string;
  columns: number;
  rows: number;
  running: boolean;
  exitCode: number | null;
  generation: number;
  lastSequence: number;
}

export interface ChatTerminalPanelLayout {
  placement: "inspector" | "bottom";
  terminalNames: string[];
  selectedIndex: number | null;
  splitDirection: "horizontal" | "vertical";
  splitSizes: number[];
}

export interface ChatTerminalLayoutRead {
  threadId: ChatThreadId;
  schemaVersion: number;
  groups: ChatTerminalPanelLayout[];
  updatedAt: UtcTimestamp | null;
}

export interface ChatTerminalOutputChunk {
  terminalId: string;
  generation: number;
  sequence: number;
  dataBase64: string;
  replay: boolean;
}

export interface ChatTerminalSnapshotRead {
  terminal: ChatTerminalRead;
  scrollback: ChatTerminalOutputChunk[];
}

export interface ChatTerminalCloseResult {
  closed: boolean;
  confirmationRequired: boolean;
}
