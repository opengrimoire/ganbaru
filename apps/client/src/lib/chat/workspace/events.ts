import type { ReviewDiffSource } from "$lib/chat/contracts";

export const CHAT_OPEN_WORKSPACE_PANEL_EVENT = "ganbaru-ai:chat-open-workspace-panel";

export interface ChatOpenReviewDetail {
  source: ReviewDiffSource;
  relativePath: string | null;
  sourceThreadId?: string;
  sourceWorkingFolderId?: string;
}

export interface ChatOpenFileDetail {
  relativePath: string;
}

export type ChatWorkspaceRequest =
  | { source: "review"; detail: ChatOpenReviewDetail | null }
  | { source: "file"; detail: ChatOpenFileDetail };

const WORKSPACE_EVENT_SOURCES = {
  "ganbaru-ai:chat-open-review": "review",
  "ganbaru-ai:chat-open-file": "file",
} as const;

/** Converts a Chat workspace event into its validated panel request. */
export function chatWorkspaceRequest(
  eventType: string,
  detail: unknown,
): ChatWorkspaceRequest | null {
  const source = WORKSPACE_EVENT_SOURCES[eventType as keyof typeof WORKSPACE_EVENT_SOURCES];
  if (source === "review") {
    if (detail === null || detail === undefined) return { source, detail: null };
    return isChatOpenReviewDetail(detail) ? { source, detail } : null;
  }
  if (source === "file") return isChatOpenFileDetail(detail) ? { source, detail } : null;
  return null;
}

/** Validates the internal event used to route workspace tools into a panel. */
export function isChatWorkspaceRequest(value: unknown): value is ChatWorkspaceRequest {
  if (!isRecord(value)) return false;
  if (value.source === "review") {
    return value.detail === null || isChatOpenReviewDetail(value.detail);
  }
  if (value.source === "file") return isChatOpenFileDetail(value.detail);
  return false;
}

export function isChatOpenReviewDetail(value: unknown): value is ChatOpenReviewDetail {
  if (!isRecord(value)) return false;
  return isReviewDiffSource(value.source)
    && (value.relativePath === null || typeof value.relativePath === "string")
    && (value.sourceThreadId === undefined || typeof value.sourceThreadId === "string")
    && (value.sourceWorkingFolderId === undefined || typeof value.sourceWorkingFolderId === "string")
    && (value.sourceThreadId === undefined) === (value.sourceWorkingFolderId === undefined);
}

export function isChatOpenFileDetail(value: unknown): value is ChatOpenFileDetail {
  return isRecord(value) && typeof value.relativePath === "string";
}

export function isReviewDiffSource(value: unknown): value is ReviewDiffSource {
  if (!isRecord(value)) return false;
  switch (value.kind) {
    case "working_tree":
      return value.mode === "staged" || value.mode === "unstaged" || value.mode === "all";
    case "checkpoint":
      return (value.range === "turn" || value.range === "thread")
        && (value.turnId === null || typeof value.turnId === "string");
    case "commit":
      return typeof value.revision === "string";
    case "branch":
      return (value.baseRef === null || typeof value.baseRef === "string")
        && typeof value.headRef === "string"
        && (value.comparison === "merge_base" || value.comparison === "direct");
    case "provider_turn":
      return typeof value.turnId === "string";
    case "change_request":
      return (value.provider === "github"
        || value.provider === "gitlab"
        || value.provider === "azure_devops"
        || value.provider === "bitbucket")
        && typeof value.repositorySlug === "string"
        && typeof value.number === "number"
        && Number.isSafeInteger(value.number)
        && value.number > 0;
    default:
      return false;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
