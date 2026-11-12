import type {
  ChatChangedFileRead,
  ChatReviewCommentRead,
  ChatReviewFileRead,
  ChatReviewPatchHunkRead,
  ChatReviewPatchRead,
  ChatReviewSnapshotRead,
  ChatWorkAssignmentId,
  ChatWorkAssignmentState,
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
    case "assignment": return `${source.kind}:${source.assignmentId}`;
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
    case "assignment":
      return right.kind === "assignment" && left.assignmentId === right.assignmentId;
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

/** The open reply thread's work assignment, as the review source picker needs it. */
export interface ReviewSourceAssignment {
  id: ChatWorkAssignmentId;
  state: ChatWorkAssignmentState;
}

/** One entry of the review source picker, keyed by `reviewSourceKey`. */
export interface ReviewSourceOption {
  value: string;
  label: string;
}

/** What the review source picker offers. */
export interface ReviewSourceOptionsInput {
  /** The source the panel shows now. */
  current: ReviewDiffSource;
  /** The Chat thread the panel reviews, which offers checkpoint sources when present. */
  threadId: string | null;
  /** The open reply thread's work assignment, which offers its whole change set when present. */
  assignment: ReviewSourceAssignment | null;
  /** Names a source in the interface language. */
  label: (source: ReviewDiffSource) => string;
}

/** Details of a review that cannot open here because its assignment ran on another device. */
export interface ReviewExecutedElsewhere {
  /** The device that ran the work, or null when the vault does not record it. */
  executingDeviceId: string | null;
  /** Distinct changed file names from the replicated review updates, capped by the native side. */
  changedFiles: string[];
  /** Whether more files changed than `changedFiles` lists. */
  changedFilesTruncated: boolean;
}

/** Upper bound on replicated changed file names accepted from an executed elsewhere failure. */
export const REVIEW_EXECUTED_ELSEWHERE_MAX_FILES = 200;

const ASSIGNMENT_SOURCE_PREFIX = "assignment:";

/**
 * Chooses the source a review panel opens on when nothing picked one: work that waits for review shows all of its
 * changes, a Chat thread shows its latest turn, and otherwise the working tree.
 *
 * @param threadId The Chat thread the panel reviews, if any.
 * @param assignment The open reply thread's work assignment, if any.
 */
export function defaultReviewSource(
  threadId: string | null,
  assignment: ReviewSourceAssignment | null,
): ReviewDiffSource {
  if (assignment?.state === "ready_for_review") return { kind: "assignment", assignmentId: assignment.id };
  return threadId
    ? { kind: "checkpoint", range: "turn", turnId: null }
    : { kind: "working_tree", mode: "all" };
}

/**
 * Lists the sources the review picker offers, without repeating a key: the assignment's changes, the current turn
 * and the entire thread for a Chat thread, the working tree modes, and the current source when it is none of those.
 *
 * @param input The current source and the context that decides which sources apply.
 */
export function reviewSourceOptions(input: ReviewSourceOptionsInput): ReviewSourceOption[] {
  const options: ReviewSourceOption[] = [];
  const seen = new Set<string>();
  const add = (source: ReviewDiffSource): void => {
    const value = reviewSourceKey(source);
    if (seen.has(value)) return;
    seen.add(value);
    options.push({ value, label: input.label(source) });
  };
  const { current } = input;
  if (input.assignment) add({ kind: "assignment", assignmentId: input.assignment.id });
  if (current.kind === "checkpoint" && current.range === "turn" && current.turnId) add(current);
  if (input.threadId) {
    add({ kind: "checkpoint", range: "turn", turnId: null });
    add({ kind: "checkpoint", range: "thread", turnId: null });
  }
  add({ kind: "working_tree", mode: "staged" });
  add({ kind: "working_tree", mode: "unstaged" });
  add({ kind: "working_tree", mode: "all" });
  if (current.kind !== "checkpoint" && current.kind !== "working_tree") add(current);
  return options;
}

/**
 * Turns a picker key back into the source it names. Keys of custom sources, such as a commit or a specific turn,
 * return null so the current source stays.
 *
 * @param value An option key from `reviewSourceOptions`.
 */
export function parseReviewSourceKey(value: string): ReviewDiffSource | null {
  switch (value) {
    case "checkpoint:turn:": return { kind: "checkpoint", range: "turn", turnId: null };
    case "checkpoint:thread:": return { kind: "checkpoint", range: "thread", turnId: null };
    case "working_tree:staged": return { kind: "working_tree", mode: "staged" };
    case "working_tree:unstaged": return { kind: "working_tree", mode: "unstaged" };
    case "working_tree:all": return { kind: "working_tree", mode: "all" };
  }
  if (value.startsWith(ASSIGNMENT_SOURCE_PREFIX)) {
    const assignmentId = value.slice(ASSIGNMENT_SOURCE_PREFIX.length);
    return assignmentId ? { kind: "assignment", assignmentId } : null;
  }
  return null;
}

/**
 * Reads the details of a review failure that names an assignment executed on another device. Returns null for
 * every other failure, and for details that do not have the expected shape.
 *
 * @param error The rejected value of a review open request.
 */
export function reviewExecutedElsewhere(error: unknown): ReviewExecutedElsewhere | null {
  const record = unknownRecord(error);
  const details = unknownRecord(record?.details) ?? unknownRecord(unknownRecord(record?.error)?.details);
  if (!details || details.reason !== "executed_elsewhere") return null;
  const { executingDeviceId, changedFiles, changedFilesTruncated } = details;
  if (executingDeviceId !== null && executingDeviceId !== undefined && typeof executingDeviceId !== "string") return null;
  if (!Array.isArray(changedFiles) || changedFiles.length > REVIEW_EXECUTED_ELSEWHERE_MAX_FILES) return null;
  if (!changedFiles.every((path): path is string => typeof path === "string" && path.length > 0)) return null;
  return {
    executingDeviceId: executingDeviceId || null,
    changedFiles,
    changedFilesTruncated: changedFilesTruncated === true,
  };
}

/**
 * Counts the files a snapshot marks as brought in by Git operations rather than by the reviewed work. Only a
 * snapshot whose filter ran can mark any.
 *
 * @param snapshot The review snapshot.
 */
export function reviewGitImportedCount(snapshot: ChatReviewSnapshotRead | null): number {
  if (snapshot?.authoredPaths !== "computed") return 0;
  return snapshot.files.filter((file) => !file.flags.authored).length;
}

/**
 * Returns the files a review shows: every file when Git-imported files are shown or none are marked, otherwise
 * only the files the reviewed work changed.
 *
 * @param snapshot The review snapshot.
 * @param showGitImported Whether the person chose to show Git-imported files.
 */
export function reviewVisibleFiles(
  snapshot: ChatReviewSnapshotRead | null,
  showGitImported: boolean,
): ChatReviewFileRead[] {
  if (!snapshot) return [];
  if (showGitImported || reviewGitImportedCount(snapshot) === 0) return snapshot.files;
  return snapshot.files.filter((file) => file.flags.authored);
}

function unknownRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}
