// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatReviewFileRead,
  ChatReviewPatchPageRead,
  ChatReviewPatchRead,
  ChatReviewSnapshotRead,
  ReadChatReviewPatchesRequest,
  ReviewDiffSource,
} from "$lib/chat/contracts";
import { ChatReviewSession, reviewSourceFallback, type ReviewSessionScope } from "./session.svelte";

const api = vi.hoisted(() => ({
  readChatReviewPatches: vi.fn<(request: ReadChatReviewPatchesRequest) => Promise<ChatReviewPatchPageRead>>(),
}));
vi.mock("$lib/api/chat", () => api);

const checkpoint: ReviewDiffSource = { kind: "checkpoint", range: "turn", turnId: "turn-1" };

describe("review source recovery", () => {
  it("uses structured reasons even when diagnostic wording changes", () => {
    expect(reviewSourceFallback({ code: "not_found", details: { reason: "checkpoint_pair" }, message: "Changed wording" }, checkpoint))
      .toEqual({ kind: "provider_turn", turnId: "turn-1" });
    expect(reviewSourceFallback({ code: "not_found", details: { reason: "checkpoint_pair" } }, { ...checkpoint, turnId: null }))
      .toEqual({ kind: "working_tree", mode: "all" });
    expect(reviewSourceFallback({ code: "not_found", details: { reason: "provider_turn" } }, { kind: "provider_turn", turnId: "turn-1" }))
      .toEqual({ kind: "working_tree", mode: "all" });
  });

  it.each([
    null, "A settled checkpoint pair is not available for this review",
    { code: "not_found", message: "A settled checkpoint pair is not available for this review" },
    { code: "permission", details: { reason: "checkpoint_pair" } },
    { code: "not_found", details: { reason: "provider_turn" } },
    { code: "not_found", details: null },
    { code: "not_found", details: "checkpoint_pair" },
  ])("does not hide unrelated or malformed errors: %j", (error) => {
    expect(reviewSourceFallback(error, checkpoint)).toBeNull();
  });
});

function reviewFile(fileId: string): ChatReviewFileRead {
  return {
    fileId,
    relativePath: fileId,
    previousRelativePath: null,
    status: "modified",
    additions: 1,
    deletions: 0,
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
}

function reviewPatch(fileId: string): ChatReviewPatchRead {
  return { fileId, patch: "@@ -1 +1 @@\n-a\n+b", hunks: [], continuationCursor: null, state: "complete" };
}

describe("review patch loading", () => {
  const scope: ReviewSessionScope = {
    threadId: "thread-1",
    workingFolderId: "folder-1",
    executionEnvironmentId: null,
    source: { kind: "working_tree", mode: "all" },
    ignoreWhitespace: false,
    selectedRelativePath: "a",
  };
  const snapshot: ChatReviewSnapshotRead = {
    snapshotId: "snapshot-1",
    reviewRevision: "revision-1",
    source: scope.source,
    sourceLabel: "Working tree",
    files: [reviewFile("a"), reviewFile("b")],
    totals: { files: 2, additions: 2, deletions: 0 },
    preferredPatch: null,
    freshness: "current",
    authoredPaths: "not_needed",
  };

  beforeEach(() => {
    vi.resetAllMocks();
  });

  function createSession(): ChatReviewSession {
    return new ChatReviewSession({
      scope: () => scope,
      hasPendingEdit: () => false,
      onSelectedFile: () => {},
      onSourceFallback: () => {},
      onSelectionReset: () => {},
      onError: () => {},
      timeoutMessage: () => "Timed out",
    });
  }

  it("releases a file whose patch request a newer request superseded, so selecting it again loads it", async () => {
    const pages = new Map<string, (page: ChatReviewPatchPageRead) => void>();
    api.readChatReviewPatches.mockImplementation((request) => new Promise((resolve) => {
      pages.set(request.fileIds.join(","), resolve);
    }));
    const session = createSession();
    session.applySnapshot(snapshot);

    const first = session.ensurePatches(["a"]);
    const second = session.ensurePatches(["b"]);
    pages.get("a")?.({ patches: [reviewPatch("a")], continuationCursor: null });
    pages.get("b")?.({ patches: [reviewPatch("b")], continuationCursor: null });
    await Promise.all([first, second]);

    expect(session.loadingPatchIds).toEqual([]);
    api.readChatReviewPatches.mockResolvedValue({ patches: [reviewPatch("a")], continuationCursor: null });
    await session.ensurePatches(["a"]);
    expect(session.patches.map((patch) => patch.fileId)).toContain("a");
  });
});
