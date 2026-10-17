import {
  type ChatChangedFileRead,
  type ChatExecutionEnvironmentRead,
  type ChatReviewFileAction,
  type ChatReviewFileRead,
  type ChatReviewPatchPageRead,
  type ChatReviewPatchRead,
  type ChatReviewSnapshotRead,
  type ChatReviewCommentRead,
  type ChatRestorePreviewRead,
  type ChatRestoreResultRead,
  type ChatTerminalLayoutRead,
  type ChatTerminalPanelLayout,
  type ChatTerminalCloseResult,
  type ChatTerminalOutputChunk,
  type ChatTerminalRead,
  type ChatTerminalSnapshotRead,
  type GitChangedPathRead,
  type GitStatusRead,
  type HostedChangeRequestRead,
  type HostedSourceControlRead,
  type ProjectWorkingFolderDirectoryRead,
  type ProjectWorkingFolderFileEntry,
  type ProjectWorkingFolderFilePreview,
  type ChatWorkspaceChangeBatch,
  type ChatWorkspaceObserverStatusRead,
  type ChatWorkspaceRename,
  type BrowserTabRead,
  type ReviewDiffSource,
} from "../contracts";
import {
  readBoolean,
  readEnum,
  readIdentifier,
  readNonNegativeSafeInteger,
  readNullable,
  readRecord,
  readSafeInteger,
  readString,
  readUtcTimestamp,
} from "./readers";

const FILE_KINDS = ["file", "directory"] as const;
const FILE_STATUSES = ["added", "modified", "deleted", "renamed", "type_changed", "unknown"] as const;
const REVIEW_STATES = ["open", "resolved"] as const;
const REVIEW_SOURCE_KINDS = ["working_tree", "checkpoint", "commit", "branch", "provider_turn", "change_request"] as const;
const REVIEW_COMMENT_SOURCE_KINDS = ["file", ...REVIEW_SOURCE_KINDS] as const;
const REVIEW_PATCH_STATES = ["complete", "partial", "binary", "oversized_hunk", "unavailable"] as const;
const REVIEW_FRESHNESS = ["current", "outdated"] as const;
const EXECUTION_ENVIRONMENT_KINDS = ["current_folder", "worktree"] as const;
const HOSTED_SOURCE_CONTROL_KINDS = ["github", "gitlab", "azure_devops", "bitbucket"] as const;
const TERMINAL_PLACEMENTS = ["inspector", "bottom"] as const;
const TERMINAL_SPLIT_DIRECTIONS = ["horizontal", "vertical"] as const;
const WORKSPACE_OBSERVER_MODES = ["native", "polling", "unavailable"] as const;

const MAX_RELATIVE_PATH_CHARS = 4_096;
const MAX_DIRECTORY_ENTRIES = 5_000;
const MAX_FILE_PREVIEW_CHARS = 1024 * 1024;
const MAX_OBSERVER_PATHS = 512;
const MAX_REVIEW_FILES = 100_000;
const MAX_REVIEW_PATCHES = 64;
const MAX_REVIEW_HUNKS = 2_048;
const MAX_REVIEW_PATCH_CHARS = 4 * 1024 * 1024;
const MAX_REVIEW_COMMENTS = 256;
const MAX_REVIEW_SELECTION_CHARS = 1024 * 1024;
const MAX_REVIEW_COMMENT_CHARS = 65_536;
const MAX_REVIEW_METADATA_CHARS = 4_096;

function array<T>(value: unknown, label: string, parse: (entry: unknown, label: string) => T): T[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  return value.map((entry, index) => parse(entry, `${label}[${index}]`));
}

function boundedArray<T>(
  value: unknown,
  label: string,
  maximumLength: number,
  parse: (entry: unknown, label: string) => T,
): T[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  if (value.length > maximumLength) {
    throw new Error(`${label} must contain at most ${maximumLength} entries`);
  }
  return value.map((entry, index) => parse(entry, `${label}[${index}]`));
}

function boundedString(value: unknown, label: string, maximumLength: number): string {
  const parsed = readString(value, label);
  if (parsed.length > maximumLength) {
    throw new Error(`${label} must contain at most ${maximumLength} characters`);
  }
  return parsed;
}

function relativePath(value: unknown, label: string): string {
  return boundedString(value, label, MAX_RELATIVE_PATH_CHARS);
}

function parseWorkspaceFileEntry(value: unknown, label: string): ProjectWorkingFolderFileEntry {
  const record = readRecord(value, label);
  return {
    relativePath: relativePath(record.relativePath, `${label}.relativePath`),
    displayName: boundedString(record.displayName, `${label}.displayName`, MAX_RELATIVE_PATH_CHARS),
    kind: readEnum(record.kind, FILE_KINDS, `${label}.kind`),
    ignored: readBoolean(record.ignored, `${label}.ignored`),
    byteSize: readNullable(record.byteSize, `${label}.byteSize`, readNonNegativeSafeInteger),
  };
}

export function parseProjectWorkingFolderDirectory(value: unknown): ProjectWorkingFolderDirectoryRead {
  const record = readRecord(value, "workspaceDirectory");
  return {
    relativePath: relativePath(record.relativePath, "workspaceDirectory.relativePath"),
    entries: boundedArray(
      record.entries,
      "workspaceDirectory.entries",
      MAX_DIRECTORY_ENTRIES,
      parseWorkspaceFileEntry,
    ),
    truncated: readBoolean(record.truncated, "workspaceDirectory.truncated"),
  };
}

export function parseProjectWorkingFolderFilePreview(value: unknown): ProjectWorkingFolderFilePreview {
  const record = readRecord(value, "workspaceFilePreview");
  return {
    relativePath: relativePath(record.relativePath, "workspaceFilePreview.relativePath"),
    displayName: boundedString(
      record.displayName,
      "workspaceFilePreview.displayName",
      MAX_RELATIVE_PATH_CHARS,
    ),
    text: readNullable(record.text, "workspaceFilePreview.text", (entry, entryLabel) =>
      boundedString(entry, entryLabel, MAX_FILE_PREVIEW_CHARS),
    ),
    lineCount: readNullable(record.lineCount, "workspaceFilePreview.lineCount", readNonNegativeSafeInteger),
    byteSize: readNonNegativeSafeInteger(record.byteSize, "workspaceFilePreview.byteSize"),
    binary: readBoolean(record.binary, "workspaceFilePreview.binary"),
    oversized: readBoolean(record.oversized, "workspaceFilePreview.oversized"),
    contentRevision: readNullable(record.contentRevision, "workspaceFilePreview.contentRevision", readString),
  };
}

function parseWorkspaceRename(value: unknown, label: string): ChatWorkspaceRename {
  const record = readRecord(value, label);
  return {
    previousRelativePath: relativePath(record.previousRelativePath, `${label}.previousRelativePath`),
    relativePath: relativePath(record.relativePath, `${label}.relativePath`),
  };
}

export function parseChatWorkspaceObserverStatus(value: unknown): ChatWorkspaceObserverStatusRead {
  const record = readRecord(value, "workspaceObserverStatus");
  return {
    workingFolderId: readIdentifier(record.workingFolderId, "workspaceObserverStatus.workingFolderId"),
    executionEnvironmentId: readNullable(
      record.executionEnvironmentId,
      "workspaceObserverStatus.executionEnvironmentId",
      readString,
    ),
    generation: readNonNegativeSafeInteger(record.generation, "workspaceObserverStatus.generation"),
    mode: readEnum(record.mode, WORKSPACE_OBSERVER_MODES, "workspaceObserverStatus.mode"),
    degradedReason: readNullable(record.degradedReason, "workspaceObserverStatus.degradedReason", readString),
  };
}

export function parseChatWorkspaceChangeBatch(value: unknown): ChatWorkspaceChangeBatch {
  const record = readRecord(value, "workspaceChangeBatch");
  return {
    workingFolderId: readIdentifier(record.workingFolderId, "workspaceChangeBatch.workingFolderId"),
    executionEnvironmentId: readNullable(
      record.executionEnvironmentId,
      "workspaceChangeBatch.executionEnvironmentId",
      readString,
    ),
    generation: readNonNegativeSafeInteger(record.generation, "workspaceChangeBatch.generation"),
    relativePaths: boundedArray(
      record.relativePaths,
      "workspaceChangeBatch.relativePaths",
      MAX_OBSERVER_PATHS,
      relativePath,
    ),
    affectedParentDirectories: boundedArray(
      record.affectedParentDirectories,
      "workspaceChangeBatch.affectedParentDirectories",
      MAX_OBSERVER_PATHS,
      relativePath,
    ),
    renames: boundedArray(
      record.renames,
      "workspaceChangeBatch.renames",
      MAX_OBSERVER_PATHS,
      parseWorkspaceRename,
    ),
    gitMetadataChanged: readBoolean(record.gitMetadataChanged, "workspaceChangeBatch.gitMetadataChanged"),
    overflowed: readBoolean(record.overflowed, "workspaceChangeBatch.overflowed"),
    degradedReason: readNullable(record.degradedReason, "workspaceChangeBatch.degradedReason", readString),
  };
}

export function parseChatReviewComment(value: unknown, label = "reviewComment"): ChatReviewCommentRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    threadId: readIdentifier(record.threadId, `${label}.threadId`),
    relativePath: relativePath(record.relativePath, `${label}.relativePath`),
    contentRevision: boundedString(
      record.contentRevision,
      `${label}.contentRevision`,
      MAX_REVIEW_METADATA_CHARS,
    ),
    startLine: readNonNegativeSafeInteger(record.startLine, `${label}.startLine`),
    startColumn: readNonNegativeSafeInteger(record.startColumn, `${label}.startColumn`),
    endLine: readNonNegativeSafeInteger(record.endLine, `${label}.endLine`),
    endColumn: readNonNegativeSafeInteger(record.endColumn, `${label}.endColumn`),
    selectedText: boundedString(
      record.selectedText,
      `${label}.selectedText`,
      MAX_REVIEW_SELECTION_CHARS,
    ),
    commentText: boundedString(
      record.commentText,
      `${label}.commentText`,
      MAX_REVIEW_COMMENT_CHARS,
    ),
    state: readEnum(record.state, REVIEW_STATES, `${label}.state`),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
    resolvedAt: readNullable(record.resolvedAt, `${label}.resolvedAt`, readUtcTimestamp),
    ...(record.sourceKind === undefined ? {} : { sourceKind: readEnum(record.sourceKind, REVIEW_COMMENT_SOURCE_KINDS, `${label}.sourceKind`) }),
    ...(record.sourceData === undefined ? {} : { sourceData: parseReviewDiffSource(record.sourceData, `${label}.sourceData`) }),
    ...(record.snapshotId === undefined ? {} : { snapshotId: readNullable(record.snapshotId, `${label}.snapshotId`, readIdentifier) }),
    ...(record.reviewRevision === undefined ? {} : { reviewRevision: readNullable(record.reviewRevision, `${label}.reviewRevision`, (entry, entryLabel) => boundedString(entry, entryLabel, MAX_REVIEW_METADATA_CHARS)) }),
    ...(record.selectionSide === undefined ? {} : { selectionSide: readEnum(record.selectionSide, ["file", "old", "new"] as const, `${label}.selectionSide`) }),
    ...(record.previousRelativePath === undefined ? {} : { previousRelativePath: readNullable(record.previousRelativePath, `${label}.previousRelativePath`, relativePath) }),
    ...(record.applicability === undefined ? {} : { applicability: readEnum(record.applicability, ["current", "outdated", "source_unavailable"] as const, `${label}.applicability`) }),
  };
}

export function parseChatReviewComments(value: unknown): ChatReviewCommentRead[] {
  return boundedArray(value, "reviewComments", MAX_REVIEW_COMMENTS, parseChatReviewComment);
}

function parseReviewDiffSource(value: unknown, label = "reviewSource"): ReviewDiffSource {
  const record = readRecord(value, label);
  const kind = readEnum(record.kind, REVIEW_SOURCE_KINDS, `${label}.kind`);
  switch (kind) {
    case "working_tree":
      return { kind, mode: readEnum(record.mode, ["staged", "unstaged", "all"] as const, `${label}.mode`) };
    case "checkpoint":
      return {
        kind,
        range: readEnum(record.range, ["turn", "thread"] as const, `${label}.range`),
        turnId: readNullable(record.turnId, `${label}.turnId`, readIdentifier),
      };
    case "commit":
      return {
        kind,
        revision: boundedString(record.revision, `${label}.revision`, MAX_REVIEW_METADATA_CHARS),
      };
    case "branch":
      return {
        kind,
        baseRef: readNullable(record.baseRef, `${label}.baseRef`, (entry, entryLabel) =>
          boundedString(entry, entryLabel, MAX_REVIEW_METADATA_CHARS),
        ),
        headRef: boundedString(record.headRef, `${label}.headRef`, MAX_REVIEW_METADATA_CHARS),
        comparison: readEnum(record.comparison, ["merge_base", "direct"] as const, `${label}.comparison`),
      };
    case "provider_turn":
      return { kind, turnId: readIdentifier(record.turnId, `${label}.turnId`) };
    case "change_request":
      return {
        kind,
        provider: readEnum(record.provider, HOSTED_SOURCE_CONTROL_KINDS, `${label}.provider`),
        repositorySlug: boundedString(
          record.repositorySlug,
          `${label}.repositorySlug`,
          MAX_REVIEW_METADATA_CHARS,
        ),
        number: readNonNegativeSafeInteger(record.number, `${label}.number`),
      };
  }
}

function parseReviewCapabilityReasons(
  value: unknown,
  label: string,
): Partial<Record<ChatReviewFileAction, string>> {
  const record = readRecord(value, label);
  const result: Partial<Record<ChatReviewFileAction, string>> = {};
  for (const action of ["stage", "unstage", "discard", "comment", "openEditor"] as const) {
    if (record[action] !== undefined) result[action] = readString(record[action], `${label}.${action}`);
  }
  return result;
}

function parseReviewFile(value: unknown, label: string): ChatReviewFileRead {
  const record = readRecord(value, label);
  const flags = readRecord(record.flags, `${label}.flags`);
  const capabilities = readRecord(record.capabilities, `${label}.capabilities`);
  return {
    fileId: readIdentifier(record.fileId, `${label}.fileId`),
    relativePath: relativePath(record.relativePath, `${label}.relativePath`),
    previousRelativePath: readNullable(
      record.previousRelativePath,
      `${label}.previousRelativePath`,
      relativePath,
    ),
    status: readEnum(record.status, FILE_STATUSES, `${label}.status`),
    additions: readNullable(record.additions, `${label}.additions`, readNonNegativeSafeInteger),
    deletions: readNullable(record.deletions, `${label}.deletions`, readNonNegativeSafeInteger),
    flags: {
      binary: readBoolean(flags.binary, `${label}.flags.binary`),
      submodule: readBoolean(flags.submodule, `${label}.flags.submodule`),
      conflict: readBoolean(flags.conflict, `${label}.flags.conflict`),
      modeOnly: readBoolean(flags.modeOnly, `${label}.flags.modeOnly`),
      pureRename: readBoolean(flags.pureRename, `${label}.flags.pureRename`),
      untracked: readBoolean(flags.untracked, `${label}.flags.untracked`),
      symlink: readBoolean(flags.symlink, `${label}.flags.symlink`),
      providerReported: readBoolean(flags.providerReported, `${label}.flags.providerReported`),
      gitObserved: readBoolean(flags.gitObserved, `${label}.flags.gitObserved`),
      readOnly: readBoolean(flags.readOnly, `${label}.flags.readOnly`),
    },
    capabilities: {
      stage: readBoolean(capabilities.stage, `${label}.capabilities.stage`),
      unstage: readBoolean(capabilities.unstage, `${label}.capabilities.unstage`),
      discard: readBoolean(capabilities.discard, `${label}.capabilities.discard`),
      comment: readBoolean(capabilities.comment, `${label}.capabilities.comment`),
      openEditor: readBoolean(capabilities.openEditor, `${label}.capabilities.openEditor`),
    },
    capabilityReasons: parseReviewCapabilityReasons(record.capabilityReasons, `${label}.capabilityReasons`),
  };
}

function parseReviewPatch(value: unknown, label: string): ChatReviewPatchRead {
  const record = readRecord(value, label);
  return {
    fileId: readIdentifier(record.fileId, `${label}.fileId`),
    patch: readNullable(record.patch, `${label}.patch`, (entry, entryLabel) =>
      boundedString(entry, entryLabel, MAX_REVIEW_PATCH_CHARS),
    ),
    hunks: boundedArray(record.hunks, `${label}.hunks`, MAX_REVIEW_HUNKS, (entry, entryLabel) => {
      const hunk = readRecord(entry, entryLabel);
      return {
        hunkId: readIdentifier(hunk.hunkId, `${entryLabel}.hunkId`),
        oldStart: readNonNegativeSafeInteger(hunk.oldStart, `${entryLabel}.oldStart`),
        oldCount: readNonNegativeSafeInteger(hunk.oldCount, `${entryLabel}.oldCount`),
        newStart: readNonNegativeSafeInteger(hunk.newStart, `${entryLabel}.newStart`),
        newCount: readNonNegativeSafeInteger(hunk.newCount, `${entryLabel}.newCount`),
        state: readEnum(hunk.state, REVIEW_PATCH_STATES, `${entryLabel}.state`),
      };
    }),
    continuationCursor: readNullable(record.continuationCursor, `${label}.continuationCursor`, (entry, entryLabel) =>
      boundedString(entry, entryLabel, MAX_REVIEW_METADATA_CHARS),
    ),
    state: readEnum(record.state, REVIEW_PATCH_STATES, `${label}.state`),
  };
}

export function parseChatReviewSnapshot(value: unknown): ChatReviewSnapshotRead {
  const record = readRecord(value, "reviewSnapshot");
  const totals = readRecord(record.totals, "reviewSnapshot.totals");
  return {
    snapshotId: readIdentifier(record.snapshotId, "reviewSnapshot.snapshotId"),
    reviewRevision: boundedString(
      record.reviewRevision,
      "reviewSnapshot.reviewRevision",
      MAX_REVIEW_METADATA_CHARS,
    ),
    source: parseReviewDiffSource(record.source, "reviewSnapshot.source"),
    sourceLabel: boundedString(
      record.sourceLabel,
      "reviewSnapshot.sourceLabel",
      MAX_REVIEW_METADATA_CHARS,
    ),
    files: boundedArray(record.files, "reviewSnapshot.files", MAX_REVIEW_FILES, parseReviewFile),
    totals: {
      files: readNonNegativeSafeInteger(totals.files, "reviewSnapshot.totals.files"),
      additions: readNonNegativeSafeInteger(totals.additions, "reviewSnapshot.totals.additions"),
      deletions: readNonNegativeSafeInteger(totals.deletions, "reviewSnapshot.totals.deletions"),
    },
    preferredPatch: readNullable(record.preferredPatch, "reviewSnapshot.preferredPatch", parseReviewPatch),
    freshness: readEnum(record.freshness, REVIEW_FRESHNESS, "reviewSnapshot.freshness"),
  };
}

export function parseChatReviewPatchPage(value: unknown): ChatReviewPatchPageRead {
  const record = readRecord(value, "reviewPatchPage");
  return {
    patches: boundedArray(
      record.patches,
      "reviewPatchPage.patches",
      MAX_REVIEW_PATCHES,
      parseReviewPatch,
    ),
    continuationCursor: readNullable(
      record.continuationCursor,
      "reviewPatchPage.continuationCursor",
      (entry, entryLabel) => boundedString(entry, entryLabel, MAX_REVIEW_METADATA_CHARS),
    ),
  };
}

function parseChangedFile(value: unknown, label: string): ChatChangedFileRead {
  const record = readRecord(value, label);
  return {
    relativePath: readString(record.relativePath, `${label}.relativePath`),
    previousRelativePath: readNullable(record.previousRelativePath, `${label}.previousRelativePath`, readString),
    status: readEnum(record.status, FILE_STATUSES, `${label}.status`),
    additions: readNullable(record.additions, `${label}.additions`, readNonNegativeSafeInteger),
    deletions: readNullable(record.deletions, `${label}.deletions`, readNonNegativeSafeInteger),
    binary: readBoolean(record.binary, `${label}.binary`),
    providerReported: readBoolean(record.providerReported, `${label}.providerReported`),
    gitObserved: readBoolean(record.gitObserved, `${label}.gitObserved`),
  };
}

export function parseChatTerminal(value: unknown, label = "terminal"): ChatTerminalRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    threadId: readIdentifier(record.threadId, `${label}.threadId`),
    workingFolderId: readIdentifier(record.workingFolderId, `${label}.workingFolderId`),
    name: readString(record.name, `${label}.name`),
    shell: readString(record.shell, `${label}.shell`),
    columns: readNonNegativeSafeInteger(record.columns, `${label}.columns`),
    rows: readNonNegativeSafeInteger(record.rows, `${label}.rows`),
    running: readBoolean(record.running, `${label}.running`),
    exitCode: readNullable(record.exitCode, `${label}.exitCode`, readSafeInteger),
    generation: readNonNegativeSafeInteger(record.generation, `${label}.generation`),
    lastSequence: readNonNegativeSafeInteger(record.lastSequence, `${label}.lastSequence`),
  };
}

function parseChatTerminalPanelLayout(value: unknown, label: string): ChatTerminalPanelLayout {
  const record = readRecord(value, label);
  return {
    placement: readEnum(record.placement, TERMINAL_PLACEMENTS, `${label}.placement`),
    terminalNames: array(record.terminalNames, `${label}.terminalNames`, readString),
    selectedIndex: readNullable(record.selectedIndex, `${label}.selectedIndex`, readNonNegativeSafeInteger),
    splitDirection: readEnum(record.splitDirection, TERMINAL_SPLIT_DIRECTIONS, `${label}.splitDirection`),
    splitSizes: array(record.splitSizes, `${label}.splitSizes`, readNonNegativeSafeInteger),
  };
}

export function parseChatTerminalLayout(value: unknown): ChatTerminalLayoutRead {
  const record = readRecord(value, "terminalLayout");
  return {
    threadId: readIdentifier(record.threadId, "terminalLayout.threadId"),
    schemaVersion: readNonNegativeSafeInteger(record.schemaVersion, "terminalLayout.schemaVersion"),
    groups: array(record.groups, "terminalLayout.groups", parseChatTerminalPanelLayout),
    updatedAt: readNullable(record.updatedAt, "terminalLayout.updatedAt", readUtcTimestamp),
  };
}

export function parseChatTerminalOutput(value: unknown, label = "terminalOutput"): ChatTerminalOutputChunk {
  const record = readRecord(value, label);
  return {
    terminalId: readIdentifier(record.terminalId, `${label}.terminalId`),
    generation: readNonNegativeSafeInteger(record.generation, `${label}.generation`),
    sequence: readNonNegativeSafeInteger(record.sequence, `${label}.sequence`),
    dataBase64: readString(record.dataBase64, `${label}.dataBase64`),
    replay: readBoolean(record.replay, `${label}.replay`),
  };
}

export function parseChatTerminalSnapshot(value: unknown): ChatTerminalSnapshotRead {
  const record = readRecord(value, "terminalSnapshot");
  return {
    terminal: parseChatTerminal(record.terminal, "terminalSnapshot.terminal"),
    scrollback: array(record.scrollback, "terminalSnapshot.scrollback", parseChatTerminalOutput),
  };
}

export function parseChatTerminalCloseResult(value: unknown): ChatTerminalCloseResult {
  const record = readRecord(value, "terminalClose");
  return {
    closed: readBoolean(record.closed, "terminalClose.closed"),
    confirmationRequired: readBoolean(record.confirmationRequired, "terminalClose.confirmationRequired"),
  };
}

export function parseChatTerminals(value: unknown): ChatTerminalRead[] {
  return array(value, "terminals", parseChatTerminal);
}

export function parseChatRestorePreview(value: unknown): ChatRestorePreviewRead {
  const record = readRecord(value, "restorePreview");
  return {
    previewId: readIdentifier(record.previewId, "restorePreview.previewId"),
    checkpointId: readIdentifier(record.checkpointId, "restorePreview.checkpointId"),
    expiresAt: readUtcTimestamp(record.expiresAt, "restorePreview.expiresAt"),
    files: array(record.files, "restorePreview.files", parseChangedFile),
    stagedChanges: readBoolean(record.stagedChanges, "restorePreview.stagedChanges"),
    providerRollback: readEnum(record.providerRollback, ["supported", "unsupported"] as const, "restorePreview.providerRollback"),
    warnings: array(record.warnings, "restorePreview.warnings", readString),
  };
}

export function parseChatRestoreResult(value: unknown): ChatRestoreResultRead {
  const record = readRecord(value, "restoreResult");
  return {
    checkpointId: readIdentifier(record.checkpointId, "restoreResult.checkpointId"),
    revertedTurnIds: array(record.revertedTurnIds, "restoreResult.revertedTurnIds", readIdentifier),
    providerHistoryAction: readEnum(record.providerHistoryAction, ["rolled_back", "fork_required"] as const, "restoreResult.providerHistoryAction"),
    recoveryState: readEnum(record.recoveryState, ["complete", "recovered", "recovery_required"] as const, "restoreResult.recoveryState"),
    threadRevision: readNonNegativeSafeInteger(record.threadRevision, "restoreResult.threadRevision"),
  };
}

function parseGitChangedPath(value: unknown, label: string): GitChangedPathRead {
  const record = readRecord(value, label);
  return {
    relativePath: readString(record.relativePath, `${label}.relativePath`),
    originalRelativePath: readNullable(record.originalRelativePath, `${label}.originalRelativePath`, readString),
    indexStatus: readString(record.indexStatus, `${label}.indexStatus`),
    worktreeStatus: readString(record.worktreeStatus, `${label}.worktreeStatus`),
    untracked: readBoolean(record.untracked, `${label}.untracked`),
    ignored: readBoolean(record.ignored, `${label}.ignored`),
  };
}

export function parseGitStatus(value: unknown): GitStatusRead {
  const record = readRecord(value, "gitStatus");
  return {
    branch: readNullable(record.branch, "gitStatus.branch", readString),
    detached: readBoolean(record.detached, "gitStatus.detached"),
    upstream: readNullable(record.upstream, "gitStatus.upstream", readString),
    ahead: readNonNegativeSafeInteger(record.ahead, "gitStatus.ahead"),
    behind: readNonNegativeSafeInteger(record.behind, "gitStatus.behind"),
    files: array(record.files, "gitStatus.files", parseGitChangedPath),
  };
}

export function parseChatExecutionEnvironment(
  value: unknown,
  label = "executionEnvironment",
): ChatExecutionEnvironmentRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    workingFolderId: readIdentifier(record.workingFolderId, `${label}.workingFolderId`),
    kind: readEnum(record.kind, EXECUTION_ENVIRONMENT_KINDS, `${label}.kind`),
    displayName: readString(record.displayName, `${label}.displayName`),
    lifecycleState: readString(record.lifecycleState, `${label}.lifecycleState`),
    branchName: readNullable(record.branchName, `${label}.branchName`, readString),
    baseReference: readNullable(record.baseReference, `${label}.baseReference`, readString),
    remoteName: readNullable(record.remoteName, `${label}.remoteName`, readString),
    cleanupState: readNullable(record.cleanupState, `${label}.cleanupState`, readString),
    localPath: readNullable(record.localPath, `${label}.localPath`, readString),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
  };
}

export function parseChatExecutionEnvironments(value: unknown): ChatExecutionEnvironmentRead[] {
  return array(value, "executionEnvironments", parseChatExecutionEnvironment);
}

export function parseHostedSourceControls(value: unknown): HostedSourceControlRead[] {
  return array(value, "hostedSourceControls", (entry, label) => {
    const record = readRecord(entry, label);
    return {
      kind: readEnum(record.kind, HOSTED_SOURCE_CONTROL_KINDS, `${label}.kind`),
      label: readString(record.label, `${label}.label`),
      detectedForRepository: readBoolean(record.detectedForRepository, `${label}.detectedForRepository`),
      remoteName: readNullable(record.remoteName, `${label}.remoteName`, readString),
      repositorySlug: readNullable(record.repositorySlug, `${label}.repositorySlug`, readString),
      status: readString(record.status, `${label}.status`),
      version: readNullable(record.version, `${label}.version`, readString),
      unavailableReason: readNullable(record.unavailableReason, `${label}.unavailableReason`, readString),
      configurationHint: readNullable(record.configurationHint, `${label}.configurationHint`, readString),
    };
  });
}

export function parseHostedChangeRequest(
  value: unknown,
  label = "hostedChangeRequest",
): HostedChangeRequestRead {
  const record = readRecord(value, label);
  return {
    providerKind: readEnum(record.providerKind, HOSTED_SOURCE_CONTROL_KINDS, `${label}.providerKind`),
    number: readNonNegativeSafeInteger(record.number, `${label}.number`),
    title: readString(record.title, `${label}.title`),
    url: readString(record.url, `${label}.url`),
    state: readString(record.state, `${label}.state`),
    baseBranch: readString(record.baseBranch, `${label}.baseBranch`),
    headBranch: readString(record.headBranch, `${label}.headBranch`),
    author: readNullable(record.author, `${label}.author`, readString),
    draft: readBoolean(record.draft, `${label}.draft`),
  };
}

export function parseHostedChangeRequests(value: unknown): HostedChangeRequestRead[] {
  return array(value, "hostedChangeRequests", parseHostedChangeRequest);
}

function parseBrowserTab(value: unknown, label: string): BrowserTabRead {
  const record = readRecord(value, label);
  return {
    threadId: readIdentifier(record.threadId, `${label}.threadId`),
    tabId: readIdentifier(record.tabId, `${label}.tabId`),
    currentUrl: readString(record.currentUrl, `${label}.currentUrl`),
    title: readString(record.title, `${label}.title`),
    visible: readBoolean(record.visible, `${label}.visible`),
    loading: readBoolean(record.loading, `${label}.loading`),
    viewportWidth: readNonNegativeSafeInteger(record.viewportWidth, `${label}.viewportWidth`),
    viewportHeight: readNonNegativeSafeInteger(record.viewportHeight, `${label}.viewportHeight`),
    externalOrigin: readBoolean(record.externalOrigin, `${label}.externalOrigin`),
  };
}

export function parseBrowserTabs(value: unknown): BrowserTabRead[] {
  return array(value, "browserTabs", parseBrowserTab);
}

export function parseBrowserTabRead(value: unknown): BrowserTabRead {
  return parseBrowserTab(value, "browserTab");
}
