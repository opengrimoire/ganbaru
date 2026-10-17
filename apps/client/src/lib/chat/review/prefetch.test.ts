import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatReviewSnapshotRead, OpenChatReviewRequest } from "$lib/chat/contracts";

const api = vi.hoisted(() => ({
  openReview: vi.fn<(request: OpenChatReviewRequest) => Promise<ChatReviewSnapshotRead>>(),
}));

vi.mock("$lib/api/chat", () => ({
  openChatReview: api.openReview,
}));

describe("Chat review prefetch", () => {
  beforeEach(() => {
    api.openReview.mockReset();
    vi.resetModules();
  });

  it("shares one immutable review between intent and panel opening", async () => {
    api.openReview.mockResolvedValue(snapshot());
    const runtime = await import("./prefetch");
    const request = immutableRequest();

    runtime.prefetchChatReview(request);
    await runtime.openChatReviewSingleFlight(request);

    expect(api.openReview).toHaveBeenCalledTimes(1);
  });

  it("does not retain a settled working-tree review", async () => {
    api.openReview.mockResolvedValue(snapshot());
    const runtime = await import("./prefetch");
    const request: OpenChatReviewRequest = {
      ...immutableRequest(),
      source: { kind: "working_tree", mode: "all" },
    };

    await runtime.openChatReviewSingleFlight(request);
    await runtime.openChatReviewSingleFlight(request);

    expect(api.openReview).toHaveBeenCalledTimes(2);
  });

  it("allows only one speculative request at a time", async () => {
    let resolveFirst!: (value: ChatReviewSnapshotRead) => void;
    const pendingReview = new Promise<ChatReviewSnapshotRead>((resolve) => {
      resolveFirst = resolve;
    });
    api.openReview.mockReturnValue(pendingReview);
    const runtime = await import("./prefetch");

    runtime.prefetchChatReview(immutableRequest());
    runtime.prefetchChatReview({ ...immutableRequest(), preferredRelativePath: "src/other.ts" });

    expect(api.openReview).toHaveBeenCalledTimes(1);
    resolveFirst(snapshot());
    await pendingReview;
  });
});

function immutableRequest(): OpenChatReviewRequest {
  return {
    threadId: "thread-1",
    workingFolderId: "folder-1",
    executionEnvironmentId: null,
    source: { kind: "provider_turn", turnId: "turn-1" },
    ignoreWhitespace: false,
    contextLines: 3,
    preferredRelativePath: "src/main.ts",
  };
}

function snapshot(): ChatReviewSnapshotRead {
  return {
    snapshotId: "snapshot-1",
    reviewRevision: "revision-1",
    source: { kind: "provider_turn", turnId: "turn-1" },
    sourceLabel: "Agent turn",
    files: [],
    totals: { files: 0, additions: 0, deletions: 0 },
    preferredPatch: null,
    freshness: "current",
  };
}
