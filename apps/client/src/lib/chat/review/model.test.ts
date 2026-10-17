import { describe, expect, it } from "vitest";
import {
  appendReviewPatchPages,
  findReviewSearchMatches,
  resolveReviewDiffStyle,
  resolveReviewLayout,
  selectedReviewHunkIds,
} from "./model";
import type { ChatReviewFileRead, ChatReviewPatchRead } from "$lib/chat/contracts";

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
    providerReported: false,
    gitObserved: true,
    readOnly: false,
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
