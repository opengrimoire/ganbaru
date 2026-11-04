// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatReviewPatchPageRead,
  ChatReviewSnapshotRead,
  OpenChatReviewRequest,
} from "$lib/chat/contracts";
import { getChat } from "$lib/stores/chat.svelte";
import ChatReviewPanel from "./ChatReviewPanel.svelte";

const api = vi.hoisted(() => ({
  openReview: vi.fn<(request: OpenChatReviewRequest) => Promise<ChatReviewSnapshotRead>>(),
  readPatches: vi.fn<() => Promise<ChatReviewPatchPageRead>>(),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  openChatReview: api.openReview,
  readChatReviewPatches: api.readPatches,
}));

class ResizeObserverMock implements ResizeObserver {
  constructor(_callback: ResizeObserverCallback) {}

  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

describe("ChatReviewPanel", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    api.openReview.mockReset();
    api.readPatches.mockReset();
    const chat = getChat();
    chat.selectedThreadId = null;
    chat.selectedWorkingFolderId = "folder-1";
    chat.draftWorkingFolderId = null;
    chat.selectedExecutionEnvironmentId = null;
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("reports one failed patch request without entering a reactive retry loop", async () => {
    api.openReview.mockResolvedValue(snapshot());
    api.readPatches.mockRejectedValue(new Error("Patch transport failed"));
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReviewPanel, {
      target,
      props: { source: { kind: "working_tree", mode: "all" } },
    });

    await vi.waitFor(() => expect(api.readPatches).toHaveBeenCalledTimes(1));
    await Promise.resolve();
    await tick();
    await Promise.resolve();
    await tick();

    expect(api.openReview).toHaveBeenCalledTimes(1);
    expect(api.readPatches).toHaveBeenCalledTimes(1);
    expect(target.querySelector('[role="alert"]')?.textContent).toContain("Patch transport failed");
    expect(target.querySelector(".review-content [data-loading]")).toBeNull();
  });

  it("shows a too large notice instead of a load failure for files left out of the snapshot", async () => {
    const base = snapshot();
    const [file] = base.files;
    api.openReview.mockResolvedValue({
      ...base,
      files: [{
        ...file,
        additions: null,
        deletions: null,
        flags: { ...file.flags, tooLarge: true },
      }],
    });
    api.readPatches.mockResolvedValue({
      patches: [{
        fileId: file.fileId,
        patch: null,
        hunks: [],
        continuationCursor: null,
        state: "unavailable",
      }],
      continuationCursor: null,
    });
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReviewPanel, {
      target,
      props: { source: { kind: "working_tree", mode: "all" } },
    });

    await vi.waitFor(() => expect(target?.textContent).toContain("too large to show in Review"));

    expect(target.querySelector('[role="alert"]')).toBeNull();
  });

  it("opens provider changes against the hidden session that produced them", async () => {
    const chat = getChat();
    chat.selectedThreadId = "current-thread";
    chat.selectedWorkingFolderId = "current-folder";
    chat.selectedExecutionEnvironmentId = "current-environment";
    const source = { kind: "provider_turn", turnId: "turn-older" } as const;
    api.openReview.mockResolvedValue({ ...snapshot(), source });
    api.readPatches.mockResolvedValue({
      patches: [],
      continuationCursor: null,
    });
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReviewPanel, {
      target,
      props: {
        source,
        sourceThreadId: "older-thread",
        sourceWorkingFolderId: "older-folder",
        sourceExecutionEnvironmentId: "older-environment",
      },
    });

    await vi.waitFor(() => expect(api.openReview).toHaveBeenCalled());

    expect(api.openReview).toHaveBeenCalledWith(expect.objectContaining({
      threadId: "older-thread",
      workingFolderId: "older-folder",
      executionEnvironmentId: "older-environment",
      source,
    }));
  });
});

function snapshot(): ChatReviewSnapshotRead {
  return {
    snapshotId: "snapshot-1",
    reviewRevision: "revision-1",
    source: { kind: "working_tree", mode: "all" },
    sourceLabel: "All changes",
    files: [{
      fileId: "file-1",
      relativePath: "hello.py",
      previousRelativePath: null,
      status: "added",
      additions: 1,
      deletions: 0,
      flags: {
        binary: false,
        submodule: false,
        conflict: false,
        modeOnly: false,
        pureRename: false,
        untracked: true,
        symlink: false,
        tooLarge: false,
        providerReported: true,
        gitObserved: true,
        readOnly: false,
      },
      capabilities: {
        stage: true,
        unstage: false,
        discard: true,
        comment: false,
        openEditor: true,
      },
      capabilityReasons: {},
    }],
    totals: { files: 1, additions: 1, deletions: 0 },
    preferredPatch: null,
    freshness: "current",
  };
}
