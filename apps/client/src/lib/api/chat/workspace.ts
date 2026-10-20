import { Channel, invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { withWorkingFolderTrust } from "$lib/api/working-folder-trust";
import { translate } from "$lib/i18n/translator.svelte";
import type {
  ChatAttachmentRead,
  ProjectWorkingFolderDirectoryRead,
  ProjectWorkingFolderFilePreview,
  ChatTerminalCloseResult,
  ChatTerminalRead,
  ChatTerminalLayoutRead,
  ChatTerminalPanelLayout,
  ChatTerminalSnapshotRead,
  ChatReviewCommentRead,
  ChatReviewSnapshotRead,
  ChatReviewPatchPageRead,
  OpenChatReviewRequest,
  ReadChatReviewPatchesRequest,
  ApplyChatReviewActionRequest,
  ApplyChatReviewActionResult,
  ChatExecutionEnvironmentRead,
  CreateChatWorktreeRequest,
  CreateChatReviewCommentRequest,
  ChatRestorePreviewRead,
  ChatRestoreResultRead,
  ChatThreadId,
  ProjectWorkingFolderId,
  GitStatusRead,
  BrowserTabBounds,
  BrowserTabRead,
  HostedChangeRequestRead,
  CreateHostedChangeRequest,
  HostedSourceControlRead,
} from "$lib/chat/contracts";
import {
  parseChatAttachmentRead,
  parseProjectWorkingFolderDirectory,
  parseProjectWorkingFolderFilePreview,
  parseChatTerminalCloseResult,
  parseChatTerminalLayout,
  parseChatTerminalSnapshot,
  parseChatTerminals,
  parseChatReviewComment,
  parseChatReviewComments,
  parseChatReviewSnapshot,
  parseChatReviewPatchPage,
  parseChatExecutionEnvironment,
  parseChatExecutionEnvironments,
  parseChatRestorePreview,
  parseChatRestoreResult,
  parseGitStatus,
  parseBrowserTabRead,
  parseBrowserTabs,
  parseHostedSourceControls,
  parseHostedChangeRequest,
  parseHostedChangeRequests,
} from "$lib/chat/validation";

export async function listChatReviewComments(
  threadId: ChatThreadId,
  includeResolved: boolean,
): Promise<ChatReviewCommentRead[]> {
  return parseChatReviewComments(await invoke<unknown>("chat_list_review_comments", {
    dbUrl: await ensureDbUrl(), threadId, includeResolved,
  }));
}

export async function createChatReviewComment(
  request: CreateChatReviewCommentRequest,
): Promise<ChatReviewCommentRead> {
  return parseChatReviewComment(await invoke<unknown>("chat_create_review_comment", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function setChatReviewCommentResolved(
  threadId: ChatThreadId,
  commentId: string,
  resolved: boolean,
): Promise<ChatReviewCommentRead> {
  return parseChatReviewComment(await invoke<unknown>("chat_set_review_comment_resolved", {
    dbUrl: await ensureDbUrl(), threadId, commentId, resolved,
  }));
}

export async function attachChatReviewComment(
  threadId: ChatThreadId,
  commentId: string,
  attachmentId: string,
): Promise<ChatAttachmentRead> {
  return parseChatAttachmentRead(await invoke<unknown>("chat_attach_review_comment", {
    dbUrl: await ensureDbUrl(), request: { threadId, commentId, attachmentId },
  }));
}

export async function openChatReview(
  request: OpenChatReviewRequest,
): Promise<ChatReviewSnapshotRead> {
  return parseChatReviewSnapshot(await invoke<unknown>("chat_open_review", {
    dbUrl: await ensureDbUrl(),
    request,
  }));
}

export async function readChatReviewPatches(
  request: ReadChatReviewPatchesRequest,
): Promise<ChatReviewPatchPageRead> {
  return parseChatReviewPatchPage(await invoke<unknown>("chat_read_review_patches", {
    dbUrl: await ensureDbUrl(),
    request,
  }));
}

export async function applyChatReviewAction(
  request: ApplyChatReviewActionRequest,
): Promise<ApplyChatReviewActionResult> {
  const value = await withWorkingFolderTrust(request.workingFolderId, async () => invoke<unknown>("chat_apply_review_action", {
    dbUrl: await ensureDbUrl(),
    request,
  }));
  if (typeof value !== "object" || value === null || !("snapshot" in value)) {
    throw new Error("Invalid Chat review action result");
  }
  return { snapshot: parseChatReviewSnapshot(value.snapshot) };
}

export async function listChatExecutionEnvironments(
  workingFolderId: ProjectWorkingFolderId,
): Promise<ChatExecutionEnvironmentRead[]> {
  return parseChatExecutionEnvironments(await invoke<unknown>("chat_list_execution_environments", {
    dbUrl: await ensureDbUrl(), workingFolderId,
  }));
}

export async function createChatWorktreeEnvironment(
  request: CreateChatWorktreeRequest,
): Promise<ChatExecutionEnvironmentRead> {
  return withWorkingFolderTrust(request.workingFolderId, async () => parseChatExecutionEnvironment(await invoke<unknown>("chat_create_worktree_environment", {
    dbUrl: await ensureDbUrl(), request,
  })));
}

export async function removeChatWorktreeEnvironment(
  workingFolderId: ProjectWorkingFolderId,
  environmentId: string,
  confirmed: boolean,
): Promise<void> {
  await invoke("chat_remove_worktree_environment", {
    dbUrl: await ensureDbUrl(), workingFolderId, environmentId, confirmed,
  });
}

export async function readChatThreadExecutionEnvironment(threadId: ChatThreadId): Promise<string | null> {
  const value = await invoke<unknown>("chat_read_thread_execution_environment", {
    dbUrl: await ensureDbUrl(), threadId,
  });
  if (value === null) return null;
  if (typeof value !== "string" || value.length === 0) {
    throw new Error("Chat execution environment response is invalid");
  }
  return value;
}

export async function discoverHostedSourceControl(
  workingFolderId: ProjectWorkingFolderId,
  executionEnvironmentId: string | null = null,
): Promise<HostedSourceControlRead[]> {
  return parseHostedSourceControls(await invoke<unknown>("chat_discover_source_control", {
    dbUrl: await ensureDbUrl(), workingFolderId, executionEnvironmentId,
  }));
}

export async function listHostedChangeRequests(
  workingFolderId: ProjectWorkingFolderId,
  executionEnvironmentId: string | null,
  providerKind: HostedSourceControlRead["kind"],
  repositorySlug: string,
  limit = 50,
): Promise<HostedChangeRequestRead[]> {
  return parseHostedChangeRequests(await invoke<unknown>("chat_list_hosted_change_requests", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    executionEnvironmentId,
    providerKind,
    repositorySlug,
    limit,
  }));
}

export async function createHostedChangeRequest(
  request: CreateHostedChangeRequest,
): Promise<HostedChangeRequestRead> {
  return withWorkingFolderTrust(request.workingFolderId, async () => parseHostedChangeRequest(await invoke<unknown>("chat_create_hosted_change_request", {
    dbUrl: await ensureDbUrl(), request,
  })));
}

export async function checkoutHostedChangeRequest(
  workingFolderId: ProjectWorkingFolderId,
  executionEnvironmentId: string | null,
  providerKind: HostedSourceControlRead["kind"],
  reference: string,
  remoteName: string | null,
): Promise<GitStatusRead> {
  return withWorkingFolderTrust(workingFolderId, async () => parseGitStatus(await invoke<unknown>("chat_checkout_hosted_change_request", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    executionEnvironmentId,
    providerKind,
    reference,
    remoteName,
  })));
}

export async function configureBitbucketCredential(
  repositorySlug: string,
  username: string,
  token: string,
): Promise<void> {
  await invoke("chat_configure_bitbucket_credential", {
    request: { repositorySlug, username, token },
  });
}

export async function removeBitbucketCredential(repositorySlug: string): Promise<boolean> {
  return invoke<boolean>("chat_remove_bitbucket_credential", { repositorySlug });
}

export async function readBrowserStatus(threadId: ChatThreadId): Promise<BrowserTabRead[]> {
  return parseBrowserTabs(await invoke<unknown>("chat_browser_status", {
    dbUrl: await ensureDbUrl(), threadId,
  }));
}

/** An external site the user lets the model use in one Chat thread. */
export interface PreviewModelOriginGrant {
  threadId: string;
  origin: string;
}

function parsePreviewModelOriginGrants(value: unknown): PreviewModelOriginGrant[] {
  if (!Array.isArray(value)) throw new Error("Invalid browser site access list");
  return value.map((entry: unknown) => {
    if (typeof entry !== "object" || entry === null) throw new Error("Invalid browser site access");
    const { threadId, origin } = entry as Record<string, unknown>;
    if (typeof threadId !== "string" || typeof origin !== "string") {
      throw new Error("Invalid browser site access");
    }
    return { threadId, origin };
  });
}

/** Lists every external site the model may use, across threads on this device. */
export async function readPreviewModelOrigins(): Promise<PreviewModelOriginGrant[]> {
  return parsePreviewModelOriginGrants(await invoke<unknown>("chat_browser_model_origins"));
}

/** Lets the model use the site the tab shows now, and returns that site's origin. */
export async function allowPreviewModelOrigin(threadId: ChatThreadId, tabId: string): Promise<string> {
  const origin = await invoke<unknown>("chat_browser_allow_model_origin", { threadId, tabId });
  if (typeof origin !== "string") throw new Error("Invalid browser site access");
  return origin;
}

/** Event emitted when a Chat thread gets a new pending browser preview approval. */
export const CHAT_BROWSER_APPROVAL_EVENT = "chat://browser-approval";

/** Page capabilities a preview page can ask the user for. */
export const PREVIEW_PAGE_PERMISSIONS = [
  "camera",
  "microphone",
  "cameraAndMicrophone",
  "screenCapture",
  "location",
  "notifications",
  "clipboard",
  "pointerLock",
  "protectedMedia",
  "storageAccess",
  "sensors",
  "fileAccess",
  "localFonts",
  "midi",
  "windowManagement",
] as const;

/** A page capability a preview page can ask the user for. */
export type PreviewPagePermission = (typeof PREVIEW_PAGE_PERMISSIONS)[number];

/** Page dialogs a preview page can open. */
export const PREVIEW_PAGE_DIALOG_KINDS = ["alert", "confirm", "prompt", "beforeUnload"] as const;

/** A page dialog a preview page can open. */
export type PreviewPageDialogKind = (typeof PREVIEW_PAGE_DIALOG_KINDS)[number];

/** An approval the coding agent or a preview page is waiting on in the Browser panel. */
export type PreviewApproval =
  | { kind: "navigation"; origin: string }
  | { kind: "network" }
  | { kind: "permission"; requestId: number; origin: string; permission: PreviewPagePermission }
  | {
    kind: "dialog";
    requestId: number;
    origin: string;
    dialogKind: PreviewPageDialogKind;
    message: string;
    defaultText: string;
  };

function isOneOf<T extends string>(values: readonly T[], value: unknown): value is T {
  return typeof value === "string" && (values as readonly string[]).includes(value);
}

function isRequestId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}

/**
 * Validates the pending approval the backend returns.
 *
 * @param value Untrusted command result.
 * @returns The approval, or `null` when nothing is waiting.
 * @throws When the result is not a known approval shape.
 */
export function parsePreviewApproval(value: unknown): PreviewApproval | null {
  if (value === null) return null;
  if (typeof value !== "object") throw new Error("Invalid browser approval");
  const { kind, origin, requestId, permission, dialogKind, message, defaultText } =
    value as Record<string, unknown>;
  if (kind === "network") return { kind };
  if (typeof origin !== "string") throw new Error("Invalid browser approval");
  if (kind === "navigation") return { kind, origin };
  if (kind === "permission" && isRequestId(requestId) && isOneOf(PREVIEW_PAGE_PERMISSIONS, permission)) {
    return { kind, requestId, origin, permission };
  }
  if (
    kind === "dialog"
    && isRequestId(requestId)
    && isOneOf(PREVIEW_PAGE_DIALOG_KINDS, dialogKind)
    && typeof message === "string"
    && typeof defaultText === "string"
  ) {
    return { kind, requestId, origin, dialogKind, message, defaultText };
  }
  throw new Error("Invalid browser approval");
}

/** Reads the approval a thread's Browser panel should show, if any. */
export async function readPendingBrowserApproval(threadId: ChatThreadId): Promise<PreviewApproval | null> {
  return parsePreviewApproval(await invoke<unknown>("chat_browser_pending_approval", { threadId }));
}

/**
 * Allows or dismisses a thread's pending approval. Allowing a navigation opens it and returns
 * the updated tab.
 *
 * @param threadId Thread whose Browser panel shows the approval.
 * @param allow Allow the request, or accept the page dialog.
 * @param page The page request being answered and, for an accepted prompt, its text.
 */
export async function resolveBrowserApproval(
  threadId: ChatThreadId,
  allow: boolean,
  page?: { requestId: number; text?: string },
): Promise<BrowserTabRead | null> {
  const value = await invoke<unknown>("chat_browser_resolve_approval", {
    dbUrl: await ensureDbUrl(),
    threadId,
    allow,
    requestId: page?.requestId ?? null,
    text: page?.text ?? null,
  });
  return value === null ? null : parseBrowserTabRead(value);
}

/** Clears preview cookies, storage, and caches for every working folder in the active vault. */
export async function clearBrowserPreviewData(): Promise<void> {
  await invoke("chat_browser_clear_data");
}

/** Stops the model from using a site in a thread. */
export async function revokePreviewModelOrigin(threadId: string, origin: string): Promise<void> {
  await invoke("chat_browser_revoke_model_origin", { threadId, origin });
}

export interface DiscoveredPreviewServer {
  url: string;
  sourceLabel: string;
}

export async function discoverPreviewServers(threadId: ChatThreadId): Promise<DiscoveredPreviewServer[]> {
  const value = await invoke<unknown>("chat_browser_discover_servers", {
    dbUrl: await ensureDbUrl(), threadId,
  });
  if (!Array.isArray(value)) throw new Error("Invalid preview server list");
  return value.map((entry) => {
    if (typeof entry !== "object" || entry === null) throw new Error("Invalid preview server");
    const record = entry as Record<string, unknown>;
    if (typeof record.url !== "string" || typeof record.sourceLabel !== "string") {
      throw new Error("Invalid preview server");
    }
    return { url: record.url, sourceLabel: record.sourceLabel };
  });
}

export async function openBrowserTab(request: {
  threadId: ChatThreadId;
  tabId: string;
  url: string;
  bounds: BrowserTabBounds;
  externalNavigationConfirmed: boolean;
}): Promise<BrowserTabRead> {
  return parseBrowserTabRead(await invoke<unknown>("chat_browser_open", {
    dbUrl: await ensureDbUrl(), request,
  }));
}

export async function navigateBrowserTab(
  threadId: ChatThreadId,
  tabId: string,
  url: string,
  externalNavigationConfirmed: boolean,
): Promise<BrowserTabRead> {
  return parseBrowserTabRead(await invoke<unknown>("chat_browser_navigate", {
    dbUrl: await ensureDbUrl(), threadId, tabId, url, externalNavigationConfirmed,
  }));
}

export async function resizeBrowserTab(
  threadId: ChatThreadId,
  tabId: string,
  bounds: BrowserTabBounds,
): Promise<BrowserTabRead> {
  return parseBrowserTabRead(await invoke<unknown>("chat_browser_resize", {
    dbUrl: await ensureDbUrl(), threadId, tabId, bounds,
  }));
}

export async function setBrowserTabVisible(
  threadId: ChatThreadId,
  tabId: string,
  visible: boolean,
): Promise<BrowserTabRead> {
  return parseBrowserTabRead(await invoke<unknown>("chat_browser_set_visible", {
    dbUrl: await ensureDbUrl(), threadId, tabId, visible,
  }));
}

export async function browserBack(threadId: ChatThreadId, tabId: string): Promise<void> {
  await invoke("chat_browser_back", { threadId, tabId });
}

export async function browserForward(threadId: ChatThreadId, tabId: string): Promise<void> {
  await invoke("chat_browser_forward", { threadId, tabId });
}

export async function refreshBrowserTab(threadId: ChatThreadId, tabId: string): Promise<void> {
  await invoke("chat_browser_refresh", { threadId, tabId });
}

export interface BrowserArtifactRead {
  id: string;
  displayName: string;
  mimeType: string;
  byteSize: number;
  resourceUri: string;
}

function parseBrowserArtifactRead(value: unknown): BrowserArtifactRead {
  if (typeof value !== "object" || value === null) throw new Error("Invalid browser artifact");
  const record = value as Record<string, unknown>;
  if (typeof record.id !== "string" || typeof record.displayName !== "string"
    || typeof record.mimeType !== "string" || typeof record.byteSize !== "number"
    || typeof record.resourceUri !== "string") {
    throw new Error("Invalid browser artifact");
  }
  return {
    id: record.id,
    displayName: record.displayName,
    mimeType: record.mimeType,
    byteSize: record.byteSize,
    resourceUri: record.resourceUri,
  };
}

export async function captureBrowserScreenshot(threadId: ChatThreadId, tabId: string): Promise<BrowserArtifactRead> {
  return parseBrowserArtifactRead(await invoke<unknown>("chat_browser_screenshot", {
    dbUrl: await ensureDbUrl(), threadId, tabId,
  }));
}

export async function startBrowserRecording(threadId: ChatThreadId, tabId: string): Promise<void> {
  await invoke("chat_browser_recording_start", { threadId, tabId, approved: true });
}

export async function stopBrowserRecording(threadId: ChatThreadId, tabId: string): Promise<BrowserArtifactRead> {
  return parseBrowserArtifactRead(await invoke<unknown>("chat_browser_recording_stop", {
    dbUrl: await ensureDbUrl(), threadId, tabId,
  }));
}

export async function closeBrowserTab(threadId: ChatThreadId, tabId: string): Promise<void> {
  await invoke("chat_browser_close", { dbUrl: await ensureDbUrl(), threadId, tabId });
}

export async function readGitStatus(workingFolderId: ProjectWorkingFolderId, executionEnvironmentId: string | null = null): Promise<GitStatusRead> {
  return parseGitStatus(await invoke<unknown>("chat_git_status", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    executionEnvironmentId,
  }));
}

export async function stageGitPaths(
  workingFolderId: ProjectWorkingFolderId,
  paths: string[],
  executionEnvironmentId: string | null = null,
): Promise<GitStatusRead> {
  return withWorkingFolderTrust(workingFolderId, async () => parseGitStatus(await invoke<unknown>("chat_git_stage", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    paths,
    executionEnvironmentId,
  })));
}

export async function unstageGitPaths(
  workingFolderId: ProjectWorkingFolderId,
  paths: string[],
  executionEnvironmentId: string | null = null,
): Promise<GitStatusRead> {
  return withWorkingFolderTrust(workingFolderId, async () => parseGitStatus(await invoke<unknown>("chat_git_unstage", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    paths,
    executionEnvironmentId,
  })));
}

export async function commitGitChanges(
  workingFolderId: ProjectWorkingFolderId,
  message: string,
  executionEnvironmentId: string | null = null,
): Promise<GitStatusRead> {
  return withWorkingFolderTrust(workingFolderId, async () => parseGitStatus(await invoke<unknown>("chat_git_commit", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    message,
    executionEnvironmentId,
  })));
}

export async function fetchGitRemote(
  workingFolderId: ProjectWorkingFolderId,
  remote: string | null = null,
  executionEnvironmentId: string | null = null,
): Promise<GitStatusRead> {
  return withWorkingFolderTrust(workingFolderId, async () => parseGitStatus(await invoke<unknown>("chat_git_fetch", {
    dbUrl: await ensureDbUrl(), workingFolderId, remote, executionEnvironmentId,
  })));
}

export async function pullGitBranch(
  workingFolderId: ProjectWorkingFolderId,
  remote: string | null = null,
  branch: string | null = null,
  executionEnvironmentId: string | null = null,
): Promise<GitStatusRead> {
  return withWorkingFolderTrust(workingFolderId, async () => parseGitStatus(await invoke<unknown>("chat_git_pull", {
    dbUrl: await ensureDbUrl(), workingFolderId, remote, branch, executionEnvironmentId,
  })));
}

export async function pushGitBranch(
  workingFolderId: ProjectWorkingFolderId,
  remote: string | null = null,
  branch: string | null = null,
  forceWithLease = false,
  destructiveConfirmed = false,
  executionEnvironmentId: string | null = null,
): Promise<GitStatusRead> {
  return withWorkingFolderTrust(workingFolderId, async () => parseGitStatus(await invoke<unknown>("chat_git_push", {
    dbUrl: await ensureDbUrl(),
    request: { workingFolderId, remote, branch, forceWithLease, destructiveConfirmed, executionEnvironmentId },
  })));
}

export async function initializeGitRepository(
  workingFolderId: ProjectWorkingFolderId,
  initialBranch: string | null,
  executionEnvironmentId: string | null = null,
): Promise<GitStatusRead> {
  return withWorkingFolderTrust(workingFolderId, async () => parseGitStatus(await invoke<unknown>("chat_git_initialize", {
    dbUrl: await ensureDbUrl(), workingFolderId, initialBranch, executionEnvironmentId,
  })));
}

export async function cloneGitRepository(
  workingFolderId: ProjectWorkingFolderId,
  remoteUrl: string,
  remoteName = "origin",
  executionEnvironmentId: string | null = null,
): Promise<GitStatusRead> {
  return withWorkingFolderTrust(workingFolderId, async () => parseGitStatus(await invoke<unknown>("chat_git_clone", {
    dbUrl: await ensureDbUrl(), workingFolderId, remoteUrl, remoteName, executionEnvironmentId,
  })));
}

export async function discardGitPaths(
  workingFolderId: ProjectWorkingFolderId,
  paths: string[],
  destructiveConfirmed: boolean,
  executionEnvironmentId: string | null = null,
): Promise<GitStatusRead> {
  return withWorkingFolderTrust(workingFolderId, async () => parseGitStatus(await invoke<unknown>("chat_git_discard", {
    dbUrl: await ensureDbUrl(), workingFolderId, paths, destructiveConfirmed, executionEnvironmentId,
  })));
}

export async function readChatTerminalLayout(threadId: ChatThreadId): Promise<ChatTerminalLayoutRead> {
  return parseChatTerminalLayout(await invoke<unknown>("chat_read_terminal_layout", {
    dbUrl: await ensureDbUrl(), threadId,
  }));
}

export async function saveChatTerminalPanelLayout(
  threadId: ChatThreadId,
  panel: ChatTerminalPanelLayout,
): Promise<ChatTerminalLayoutRead> {
  return parseChatTerminalLayout(await invoke<unknown>("chat_save_terminal_panel_layout", {
    dbUrl: await ensureDbUrl(), threadId, panel,
  }));
}

export async function listProjectWorkingFolderDirectory(
  workingFolderId: ProjectWorkingFolderId,
  relativePath: string,
  includeIgnored = false,
  executionEnvironmentId: string | null = null,
): Promise<ProjectWorkingFolderDirectoryRead> {
  return parseProjectWorkingFolderDirectory(await invoke<unknown>("projects_list_working_folder_directory", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    relativePath,
    includeIgnored,
    executionEnvironmentId,
  }));
}

export async function previewProjectWorkingFolderFile(
  workingFolderId: ProjectWorkingFolderId,
  relativePath: string,
  executionEnvironmentId: string | null = null,
): Promise<ProjectWorkingFolderFilePreview> {
  return parseProjectWorkingFolderFilePreview(await invoke<unknown>("projects_preview_working_folder_file", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    relativePath,
    executionEnvironmentId,
  }));
}

export async function saveProjectWorkingFolderFile(request: {
  workingFolderId: ProjectWorkingFolderId;
  relativePath: string;
  contents: string;
  expectedRevision: string;
  executionEnvironmentId?: string | null;
}): Promise<ProjectWorkingFolderFilePreview> {
  return parseProjectWorkingFolderFilePreview(await invoke<unknown>("projects_save_working_folder_file", {
    dbUrl: await ensureDbUrl(),
    request,
  }));
}

export async function saveProjectWorkingFolderFileCopy(request: {
  workingFolderId: ProjectWorkingFolderId;
  sourceRelativePath: string;
  targetRelativePath: string;
  contents: string;
  executionEnvironmentId?: string | null;
}): Promise<ProjectWorkingFolderFilePreview> {
  return parseProjectWorkingFolderFilePreview(await invoke<unknown>("projects_save_working_folder_file_copy", {
    dbUrl: await ensureDbUrl(),
    request,
  }));
}

export async function recreateProjectWorkingFolderFile(request: {
  workingFolderId: ProjectWorkingFolderId;
  relativePath: string;
  contents: string;
  confirmed: boolean;
  executionEnvironmentId?: string | null;
}): Promise<ProjectWorkingFolderFilePreview> {
  return parseProjectWorkingFolderFilePreview(await invoke<unknown>("projects_recreate_working_folder_file", {
    dbUrl: await ensureDbUrl(),
    request,
  }));
}

/**
 * Opens a workspace file with the system default handler. Documents open directly; a type that
 * could run as a program asks in a native dialog first, and declining rejects with a `cancelled`
 * Chat error.
 */
export async function openProjectWorkingFolderFile(
  workingFolderId: ProjectWorkingFolderId,
  relativePath: string,
  executionEnvironmentId: string | null = null,
): Promise<void> {
  await invoke("projects_open_working_folder_file", {
    dbUrl: await ensureDbUrl(),
    workingFolderId,
    relativePath,
    executionEnvironmentId,
    prompt: externalOpenPrompt(),
  });
}

/** Localized text for the native open confirmation; Rust appends the file path. */
function externalOpenPrompt() {
  return {
    title: translate("chat.inspector.openExternallyTitle"),
    message: translate("chat.inspector.openExternallyMessage"),
    allowLabel: translate("chat.inspector.openExternallyAllow"),
    cancelLabel: translate("chat.inspector.openExternallyCancel"),
  };
}

export async function listChatTerminals(
  threadId: ChatThreadId,
  workingFolderId: ProjectWorkingFolderId,
): Promise<ChatTerminalRead[]> {
  return parseChatTerminals(await invoke<unknown>("chat_list_terminals", {
    dbUrl: await ensureDbUrl(),
    threadId,
    workingFolderId,
  }));
}

/** Localized text for the native terminal start dialog; Rust appends the folder and shell. */
function terminalStartPrompt() {
  return {
    title: translate("chat.inspector.terminalStartTitle"),
    message: translate("chat.inspector.terminalStartMessage"),
    allowLabel: translate("chat.inspector.terminalStartAllow"),
    cancelLabel: translate("chat.inspector.terminalStartCancel"),
  };
}

/**
 * Binds a thread to its working folder before terminal requests. An unsaved draft thread is
 * reserved natively; a saved thread is only checked against the folder.
 */
export async function reserveChatDraftThread(
  threadId: ChatThreadId,
  workingFolderId: ProjectWorkingFolderId,
): Promise<void> {
  await invoke<unknown>("chat_reserve_draft_thread", {
    dbUrl: await ensureDbUrl(),
    threadId,
    workingFolderId,
  });
}

/**
 * Starts a terminal. The first terminal for a folder and environment in each app launch asks
 * for a native confirmation; declining rejects with a `cancelled` Chat error.
 */
export async function createChatTerminal(request: {
  terminalId: string;
  threadId: ChatThreadId;
  workingFolderId: ProjectWorkingFolderId;
  columns: number;
  rows: number;
}): Promise<ChatTerminalSnapshotRead> {
  return parseChatTerminalSnapshot(await invoke<unknown>("chat_terminal_create", {
    dbUrl: await ensureDbUrl(),
    request: { ...request, startPrompt: terminalStartPrompt() },
  }));
}

export async function readChatTerminalSnapshot(
  terminalId: string,
  threadId: ChatThreadId,
  workingFolderId: ProjectWorkingFolderId,
): Promise<ChatTerminalSnapshotRead> {
  return parseChatTerminalSnapshot(await invoke<unknown>("chat_terminal_snapshot", {
    dbUrl: await ensureDbUrl(),
    terminalId,
    threadId,
    workingFolderId,
  }));
}

/**
 * Subscribes one terminal view to live output and returns the scrollback snapshot taken at attach time.
 *
 * Live chunks arrive only on `channel` and stop once the view holds a full window of unacknowledged
 * bytes, so the view must acknowledge what it writes.
 *
 * @param subscriptionId - Random identifier for this view, used to acknowledge and detach.
 * @param channel - Receives live output chunks for this view only.
 * @returns The terminal state and scrollback at the moment the subscription began.
 */
export async function attachChatTerminal(
  terminalId: string,
  threadId: ChatThreadId,
  workingFolderId: ProjectWorkingFolderId,
  subscriptionId: string,
  channel: Channel<unknown>,
): Promise<ChatTerminalSnapshotRead> {
  return parseChatTerminalSnapshot(await invoke<unknown>("chat_terminal_attach", {
    dbUrl: await ensureDbUrl(),
    terminalId,
    threadId,
    workingFolderId,
    subscriptionId,
    channel,
  }));
}

/**
 * Returns delivery credit for live output bytes the view has finished writing.
 *
 * @param bytes - Decoded output bytes received on the subscription since the last acknowledgement.
 */
export async function acknowledgeChatTerminalOutput(
  terminalId: string,
  subscriptionId: string,
  bytes: number,
): Promise<void> {
  await invoke("chat_terminal_acknowledge", { terminalId, subscriptionId, bytes });
}

/** Ends one view's live output subscription; detaching from a closed terminal succeeds. */
export async function detachChatTerminal(terminalId: string, subscriptionId: string): Promise<void> {
  await invoke("chat_terminal_detach", { terminalId, subscriptionId });
}

export async function writeChatTerminal(
  terminalId: string,
  threadId: ChatThreadId,
  workingFolderId: ProjectWorkingFolderId,
  text: string,
): Promise<void> {
  await invoke("chat_terminal_input", {
    dbUrl: await ensureDbUrl(),
    request: { terminalId, threadId, workingFolderId, text },
  });
}

export async function resizeChatTerminal(
  terminalId: string,
  threadId: ChatThreadId,
  workingFolderId: ProjectWorkingFolderId,
  columns: number,
  rows: number,
): Promise<void> {
  await invoke("chat_terminal_resize", {
    dbUrl: await ensureDbUrl(),
    request: { terminalId, threadId, workingFolderId, columns, rows },
  });
}

export async function closeChatTerminal(
  terminalId: string,
  threadId: ChatThreadId,
  workingFolderId: ProjectWorkingFolderId,
  confirmed: boolean,
): Promise<ChatTerminalCloseResult> {
  return parseChatTerminalCloseResult(await invoke<unknown>("chat_terminal_close", {
    dbUrl: await ensureDbUrl(),
    terminalId,
    threadId,
    workingFolderId,
    confirmed,
  }));
}

export async function previewChatCheckpointRestore(
  threadId: ChatThreadId,
  checkpointId: string,
): Promise<ChatRestorePreviewRead> {
  return parseChatRestorePreview(await invoke<unknown>("chat_preview_checkpoint_restore", {
    dbUrl: await ensureDbUrl(),
    request: { threadId, checkpointId },
  }));
}

export async function executeChatCheckpointRestore(request: {
  command: { clientCommandId: string; expectedThreadRevision: number | null };
  threadId: ChatThreadId;
  previewId: string;
  confirmed: boolean;
}): Promise<ChatRestoreResultRead> {
  return parseChatRestoreResult(await invoke<unknown>("chat_execute_checkpoint_restore", {
    dbUrl: await ensureDbUrl(),
    request,
  }));
}
