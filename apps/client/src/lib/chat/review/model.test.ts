import { describe, expect, it } from "vitest";
import {
  appendReviewPatchPages,
  defaultReviewSource,
  findReviewSearchMatches,
  parseReviewSourceKey,
  resolveReviewDiffStyle,
  resolveReviewLayout,
  reviewExecutedElsewhere,
  reviewGitImportedCount,
  reviewSourceKey,
  reviewSourceOptions,
  reviewVisibleFiles,
  selectedReviewHunkIds,
} from "./model";
import { reviewSourceFallback } from "./session.svelte";
import type {
  ChatReviewFileRead,
  ChatReviewPatchRead,
  ChatReviewSnapshotRead,
  ReviewDiffSource,
} from "$lib/chat/contracts";

const file: ChatReviewFileRead = {
  fileId: "file:src/app.ts",
  relativePath: "src/app.ts",
  previousRelativePath: null,
  status: "modified",
  additions: 2,
  deletions: 1,
  flags: {
    binary: false,
    submodule: false,
    conflict: false,
    modeOnly: false,
    pureRename: false,
    untracked: false,
    symlink: false,
    tooLarge: false,
    providerReported: false,
    gitObserved: true,
    readOnly: false,
    authored: true,
  },
  capabilities: { stage: true, unstage: false, discard: true, comment: true, openEditor: true },
  capabilityReasons: {},
};

const patch: ChatReviewPatchRead = {
  fileId: file.fileId,
  patch: "diff --git a/src/app.ts b/src/app.ts\n--- a/src/app.ts\n+++ b/src/app.ts\n@@ -4,2 +4,3 @@\n-old\n+new value\n context",
  hunks: [{ hunkId: "hunk:1", oldStart: 4, oldCount: 2, newStart: 4, newCount: 3, state: "complete" }],
  continuationCursor: null,
  state: "complete",
};

describe("review model", () => {
  it("uses continuous review only when the measured workload fits", () => {
    expect(resolveReviewLayout("auto", 900, 20, 2_000)).toBe("continuous");
    expect(resolveReviewLayout("auto", 600, 20, 2_000)).toBe("file");
    expect(resolveReviewLayout("auto", 900, 201, 2_000)).toBe("file");
    expect(resolveReviewLayout("continuous", 300, 20, 2_000)).toBe("continuous");
    expect(resolveReviewLayout("continuous", 900, 1_000, 100_000)).toBe("file");
  });

  it("keeps split rendering behind a fit-based width", () => {
    expect(resolveReviewDiffStyle("auto", 719)).toBe("unified");
    expect(resolveReviewDiffStyle("auto", 720)).toBe("split");
  });

  it("maps a selected line range to Rust-owned hunk identifiers", () => {
    expect(selectedReviewHunkIds(patch, { start: 5, end: 5, side: "additions" })).toEqual(["hunk:1"]);
    expect(selectedReviewHunkIds(patch, { start: 40, end: 40, side: "additions" })).toEqual([]);
  });

  it("searches patch content using old and new line coordinates", () => {
    expect(findReviewSearchMatches([file], [patch], "new value")).toMatchObject([
      { fileId: file.fileId, lineNumber: 4, side: "additions" },
    ]);
    expect(findReviewSearchMatches([file], [patch], "src/app.ts")).toEqual([]);
  });

  it("keeps continuation pages ordered and ignores repeated page cursors", () => {
    const first = { ...patch, continuationCursor: `${file.fileId}/1`, state: "partial" as const };
    const second = {
      ...patch,
      patch: "@@ -8 +8 @@\n-before\n+after",
      hunks: [{ ...patch.hunks[0], hunkId: "hunk:2", oldStart: 8, newStart: 8 }],
      continuationCursor: null,
      state: "partial" as const,
    };

    expect(appendReviewPatchPages([first], [first, second, second])).toEqual([first, second]);
  });
});

const sourceLabel = (source: ReviewDiffSource): string => reviewSourceKey(source);
const readyAssignment = { id: "assignment-1", state: "ready_for_review" } as const;

function snapshotWith(files: ChatReviewFileRead[], authoredPaths: ChatReviewSnapshotRead["authoredPaths"]): ChatReviewSnapshotRead {
  return {
    snapshotId: "snapshot-1",
    reviewRevision: "revision-1",
    source: { kind: "assignment", assignmentId: "assignment-1" },
    sourceLabel: "Assignment",
    files,
    totals: { files: files.length, additions: 0, deletions: 0 },
    preferredPatch: null,
    freshness: "current",
    authoredPaths,
  };
}

describe("review sources", () => {
  it("offers the assignment's changes first and opens on them while the work waits for review", () => {
    const current = defaultReviewSource("thread-1", readyAssignment);
    expect(current).toEqual({ kind: "assignment", assignmentId: "assignment-1" });
    const options = reviewSourceOptions({ current, threadId: "thread-1", assignment: readyAssignment, label: sourceLabel });
    expect(options.map((option) => option.value)).toEqual([
      "assignment:assignment-1",
      "checkpoint:turn:",
      "checkpoint:thread:",
      "working_tree:staged",
      "working_tree:unstaged",
      "working_tree:all",
    ]);
  });

  it("keeps the turn and working tree defaults for work that does not wait for review", () => {
    const working = { id: "assignment-1", state: "working" } as const;
    expect(defaultReviewSource("thread-1", working)).toEqual({ kind: "checkpoint", range: "turn", turnId: null });
    expect(defaultReviewSource(null, working)).toEqual({ kind: "working_tree", mode: "all" });
    expect(defaultReviewSource(null, null)).toEqual({ kind: "working_tree", mode: "all" });
    const options = reviewSourceOptions({
      current: { kind: "working_tree", mode: "all" },
      threadId: null,
      assignment: working,
      label: sourceLabel,
    });
    expect(options[0]?.value).toBe("assignment:assignment-1");
  });

  it("leaves the assignment out without a reply thread assignment and keeps custom sources last", () => {
    const options = reviewSourceOptions({
      current: { kind: "commit", revision: "abc123" },
      threadId: null,
      assignment: null,
      label: sourceLabel,
    });
    expect(options.map((option) => option.value)).toEqual([
      "working_tree:staged",
      "working_tree:unstaged",
      "working_tree:all",
      "commit:abc123",
    ]);
  });

  it("parses fixed and assignment keys and leaves custom keys unchanged", () => {
    expect(parseReviewSourceKey("assignment:assignment-1")).toEqual({ kind: "assignment", assignmentId: "assignment-1" });
    expect(parseReviewSourceKey("assignment:")).toBeNull();
    expect(parseReviewSourceKey("checkpoint:thread:")).toEqual({ kind: "checkpoint", range: "thread", turnId: null });
    expect(parseReviewSourceKey("working_tree:unstaged")).toEqual({ kind: "working_tree", mode: "unstaged" });
    expect(parseReviewSourceKey("checkpoint:turn:turn-1")).toBeNull();
    expect(parseReviewSourceKey("commit:abc123")).toBeNull();
    for (const option of reviewSourceOptions({ current: { kind: "working_tree", mode: "all" }, threadId: "thread-1", assignment: readyAssignment, label: sourceLabel })) {
      const parsed = parseReviewSourceKey(option.value);
      expect(parsed && reviewSourceKey(parsed)).toBe(option.value);
    }
  });
});

describe("executed elsewhere reviews", () => {
  const elsewhere = {
    code: "not_found",
    message: "The assignment ran on another device",
    details: {
      reason: "executed_elsewhere",
      executingDeviceId: "device-2",
      changedFiles: ["src/app.ts", "README.md"],
      changedFilesTruncated: false,
    },
  };

  it("reads the device and changed file names without falling back to another source", () => {
    expect(reviewExecutedElsewhere(elsewhere)).toEqual({
      executingDeviceId: "device-2",
      changedFiles: ["src/app.ts", "README.md"],
      changedFilesTruncated: false,
    });
    expect(reviewSourceFallback(elsewhere, { kind: "assignment", assignmentId: "assignment-1" })).toBeNull();
  });

  it("accepts an unknown device and reads nested details", () => {
    expect(reviewExecutedElsewhere({
      error: { details: { reason: "executed_elsewhere", executingDeviceId: null, changedFiles: [], changedFilesTruncated: true } },
    })).toEqual({ executingDeviceId: null, changedFiles: [], changedFilesTruncated: true });
  });

  it("ignores other reasons and malformed details", () => {
    expect(reviewExecutedElsewhere({ code: "not_found", details: { reason: "assignment_checkpoints" } })).toBeNull();
    expect(reviewExecutedElsewhere({ details: { ...elsewhere.details, changedFiles: [3] } })).toBeNull();
    expect(reviewExecutedElsewhere({ details: { ...elsewhere.details, executingDeviceId: 7 } })).toBeNull();
    expect(reviewExecutedElsewhere({ details: { ...elsewhere.details, changedFiles: Array.from({ length: 201 }, (_, index) => `f${index}`) } })).toBeNull();
    expect(reviewExecutedElsewhere("executed_elsewhere")).toBeNull();
  });
});

describe("Git-imported review files", () => {
  const imported: ChatReviewFileRead = {
    ...file,
    fileId: "file:vendor/lib.ts",
    relativePath: "vendor/lib.ts",
    flags: { ...file.flags, authored: false },
  };

  it("hides files the filter marked as brought in by Git until they are shown", () => {
    const snapshot = snapshotWith([file, imported], "computed");
    expect(reviewGitImportedCount(snapshot)).toBe(1);
    expect(reviewVisibleFiles(snapshot, false)).toEqual([file]);
    expect(reviewVisibleFiles(snapshot, true)).toEqual([file, imported]);
  });

  it("shows every file when the filter did not run", () => {
    for (const status of ["not_needed", "unavailable"] as const) {
      const snapshot = snapshotWith([file, imported], status);
      expect(reviewGitImportedCount(snapshot)).toBe(0);
      expect(reviewVisibleFiles(snapshot, false)).toEqual([file, imported]);
    }
    expect(reviewVisibleFiles(null, false)).toEqual([]);
  });
});
