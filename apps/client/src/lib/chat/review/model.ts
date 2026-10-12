import type {
  ChatChangedFileRead,
  ChatReviewCommentRead,
  ChatReviewFileRead,
  ChatReviewPatchHunkRead,
  ChatReviewPatchRead,
  ChatReviewSnapshotRead,
  ReviewDiffSource,
} from "$lib/chat/contracts";
import type { ReviewLineSelection } from "./diff-runtime";

export type ReviewLayoutPreference = "auto" | "continuous" | "file";
export type ReviewResolvedLayout = Exclude<ReviewLayoutPreference, "auto">;
export type ReviewDiffPreference = "auto" | "unified" | "split";
export type ReviewResolvedDiffStyle = Exclude<ReviewDiffPreference, "auto">;

export interface ReviewSearchMatch {
  fileId: string;
  relativePath: string;
  lineNumber: number;
  side: "deletions" | "additions";
  text: string;
}

/** Appends unseen patch pages while preserving the backend page order for every file. */
export function appendReviewPatchPages(
  current: readonly ChatReviewPatchRead[],
  incoming: readonly ChatReviewPatchRead[],
): ChatReviewPatchRead[] {
  if (incoming.length === 0) return [...current];
  const seen = new Set(current.map(reviewPatchPageKey));
  const next = [...current];
  for (const patch of incoming) {
    const key = reviewPatchPageKey(patch);
    if (seen.has(key)) continue;
    seen.add(key);
    next.push(patch);
  }
  return next;
}

function reviewPatchPageKey(patch: ChatReviewPatchRead): string {
  return `${patch.fileId}\u0000${patch.continuationCursor ?? "<complete>"}`;
}

export const REVIEW_CONTINUOUS_MIN_WIDTH_PX = 720;
export const REVIEW_CONTINUOUS_MAX_FILES = 200;
export const REVIEW_CONTINUOUS_MAX_CHANGED_LINES = 50_000;
export const REVIEW_SPLIT_MIN_WIDTH_PX = 720;

/** Resolves the review flow without relying on viewport classes. */
export function resolveReviewLayout(
  preference: ReviewLayoutPreference,
  availableWidth: number,
  fileCount: number,
  changedLines: number,
): ReviewResolvedLayout {
  if (preference === "file") return "file";
  if (fileCount > REVIEW_CONTINUOUS_MAX_FILES || changedLines > REVIEW_CONTINUOUS_MAX_CHANGED_LINES) {
    return "file";
  }
  if (preference === "continuous") return "continuous";
  return availableWidth >= REVIEW_CONTINUOUS_MIN_WIDTH_PX
    ? "continuous"
    : "file";
}

/** Resolves split diff rendering from the measured review content width. */
export function resolveReviewDiffStyle(
  preference: ReviewDiffPreference,
  availableWidth: number,
): ReviewResolvedDiffStyle {
  if (preference !== "auto") return preference;
  return availableWidth >= REVIEW_SPLIT_MIN_WIDTH_PX ? "split" : "unified";
}

/** Selects the file retained across immutable snapshot refreshes. */
export function retainReviewFile(
  snapshot: ChatReviewSnapshotRead,
  fileId: string | null,
  relativePath: string | null,
): ChatReviewFileRead | null {
  return snapshot.files.find((file) => file.fileId === fileId)
    ?? snapshot.files.find((file) => file.relativePath === relativePath)
    ?? snapshot.files[0]
    ?? null;
}

/** Returns the reviewed hunk identifiers intersecting the current line selection. */
export function selectedReviewHunkIds(
  patch: ChatReviewPatchRead | null,
  range: { start: number; end: number; side?: "deletions" | "additions"; endSide?: "deletions" | "additions" } | null,
): string[] {
  if (!patch || !range) return [];
  const start = Math.min(range.start, range.end);
  const end = Math.max(range.start, range.end);
  const sides = new Set([range.side ?? "additions", range.endSide ?? range.side ?? "additions"]);
  return patch.hunks
    .filter((hunk) => hunk.state === "complete" && hunkIntersects(hunk, sides, start, end))
    .map((hunk) => hunk.hunkId);
}

function hunkIntersects(
  hunk: ChatReviewPatchHunkRead,
  sides: ReadonlySet<"deletions" | "additions">,
  start: number,
  end: number,
): boolean {
  return (sides.has("deletions") && rangesIntersect(start, end, hunk.oldStart, hunk.oldCount))
    || (sides.has("additions") && rangesIntersect(start, end, hunk.newStart, hunk.newCount));
}

function rangesIntersect(start: number, end: number, hunkStart: number, hunkCount: number): boolean {
  if (hunkCount <= 0) return false;
  return start <= hunkStart + hunkCount - 1 && end >= hunkStart;
}

/** Finds patch text without constructing rendered DOM nodes. */
export function findReviewSearchMatches(
  files: readonly ChatReviewFileRead[],
  patches: readonly ChatReviewPatchRead[],
  query: string,
  limit = 1_000,
): ReviewSearchMatch[] {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return [];
  const filesById = new Map(files.map((file) => [file.fileId, file]));
  const matches: ReviewSearchMatch[] = [];
  for (const patch of patches) {
    const file = filesById.get(patch.fileId);
    if (!file || !patch.patch) continue;
    let oldLine = 0;
    let newLine = 0;
    for (const rawLine of patch.patch.split("\n")) {
      const hunk = /^@@ -(?<old>\d+)(?:,\d+)? \+(?<next>\d+)(?:,\d+)? @@/.exec(rawLine);
      if (hunk?.groups) {
        oldLine = Number.parseInt(hunk.groups.old ?? "0", 10);
        newLine = Number.parseInt(hunk.groups.next ?? "0", 10);
        continue;
      }
      const marker = rawLine[0];
      if (rawLine.startsWith("+++") || rawLine.startsWith("---")) continue;
      if (marker !== "+" && marker !== "-" && marker !== " ") continue;
      const text = rawLine.slice(1);
      if (text.toLocaleLowerCase().includes(needle)) {
        matches.push({
          fileId: patch.fileId,
          relativePath: file.relativePath,
          lineNumber: marker === "-" ? oldLine : newLine,
          side: marker === "-" ? "deletions" : "additions",
          text,
        });
        if (matches.length >= limit) return matches;
      }
      if (marker !== "+") oldLine += 1;
      if (marker !== "-") newLine += 1;
    }
  }
  return matches;
}

/** Returns a stable source key for request cancellation and local state. */
export function reviewSourceKey(source: ReviewDiffSource): string {
  switch (source.kind) {
    case "working_tree": return `${source.kind}:${source.mode}`;
    case "checkpoint": return `${source.kind}:${source.range}:${source.turnId ?? ""}`;
    case "commit": return `${source.kind}:${source.revision}`;
    case "branch": return `${source.kind}:${source.baseRef ?? ""}:${source.headRef}:${source.comparison}`;
    case "provider_turn": return `${source.kind}:${source.turnId}`;
    case "change_request": return `${source.kind}:${source.provider}:${source.repositorySlug}:${source.number}`;
  }
}

/** Reports whether a line selection crosses old and new diff sides. */
export function reviewSelectionUsesMultipleSides(
  selection: ReviewLineSelection | null,
): boolean {
  if (!selection) return false;
  const startSide = selection.range.side ?? "additions";
  return (selection.range.endSide ?? startSide) !== startSide;
}

/** Compares canonical Review source identities. */
export function reviewSourcesEqual(
  left: ReviewDiffSource,
  right: ReviewDiffSource,
): boolean {
  switch (left.kind) {
    case "working_tree":
      return right.kind === "working_tree" && left.mode === right.mode;
    case "checkpoint":
      return right.kind === "checkpoint"
        && left.range === right.range
        && left.turnId === right.turnId;
    case "commit":
      return right.kind === "commit" && left.revision === right.revision;
    case "branch":
      return right.kind === "branch"
        && left.baseRef === right.baseRef
        && left.headRef === right.headRef
        && left.comparison === right.comparison;
    case "provider_turn":
      return right.kind === "provider_turn" && left.turnId === right.turnId;
    case "change_request":
      return right.kind === "change_request"
        && left.provider === right.provider
        && left.repositorySlug === right.repositorySlug
        && left.number === right.number;
  }
}

/** Reports whether a persisted comment still targets the rendered snapshot. */
export function reviewCommentMatchesSnapshot(
  comment: ChatReviewCommentRead,
  snapshot: ChatReviewSnapshotRead,
): boolean {
  return (comment.selectionSide === "old" || comment.selectionSide === "new")
    && comment.sourceData !== undefined
    && comment.reviewRevision === snapshot.reviewRevision
    && reviewSourcesEqual(comment.sourceData, snapshot.source);
}

/** Reports whether a comment no longer applies to the active snapshot. */
export function reviewCommentIsOutdated(
  comment: ChatReviewCommentRead,
  snapshot: ChatReviewSnapshotRead | null,
): boolean {
  if (comment.applicability === "source_unavailable") return true;
  return snapshot
    ? !reviewCommentMatchesSnapshot(comment, snapshot)
    : comment.applicability === "outdated";
}

/** Reports whether every file in a working-tree snapshot supports an operation. */
export function reviewSourceSupportsOperation(
  snapshot: ChatReviewSnapshotRead | null,
  operation: "stage" | "unstage" | "discard",
): boolean {
  if (!snapshot || snapshot.source.kind !== "working_tree" || snapshot.files.length === 0) {
    return false;
  }
  const modeSupports = operation === "stage"
    ? snapshot.source.mode === "unstaged" || snapshot.source.mode === "all"
    : operation === "unstage"
      ? snapshot.source.mode === "staged"
      : snapshot.source.mode === "unstaged";
  return modeSupports && snapshot.files.every((file) => file.capabilities[operation]);
}

/** Reports whether a file action should be shown, including disabled actions with a reason. */
export function reviewFileActionVisible(
  file: ChatReviewFileRead,
  action: "stage" | "unstage" | "discard" | "openEditor",
): boolean {
  return file.capabilities[action] || Boolean(file.capabilityReasons[action]);
}

/** Adapts a Review file for the shared changed-file tree. */
export function reviewFileAsChangedFile(file: ChatReviewFileRead): ChatChangedFileRead {
  return {
    relativePath: file.relativePath,
    previousRelativePath: file.previousRelativePath,
    status: file.status,
    additions: file.additions,
    deletions: file.deletions,
    binary: file.flags.binary,
    providerReported: file.flags.providerReported,
    gitObserved: file.flags.gitObserved,
  };
}

/** Produces a stable compact revision for memoized diff rendering. */
export function stableReviewHash(value: string): number {
  let hash = 2_166_136_261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16_777_619);
  }
  return hash >>> 0;
}
