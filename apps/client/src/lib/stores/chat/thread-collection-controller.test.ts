import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatThreadShellRead } from "$lib/chat/contracts";
import { ChatThreadCollectionController } from "./thread-collection-controller.svelte";

const api = vi.hoisted(() => ({
  listThreadWindow: vi.fn<
    (workingFolderId: string | null, archived: boolean, limit: number) => Promise<ChatThreadShellRead[]>
  >(),
  renameThread: vi.fn<(threadId: string, title: string, revision: number) => Promise<ChatThreadShellRead>>(),
  deleteThread: vi.fn<(threadId: string, revision: number, title: string) => Promise<void>>(),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  listChatThreadWindow: api.listThreadWindow,
  renameChatThread: api.renameThread,
  deleteChatThreadPermanently: api.deleteThread,
}));

describe("ChatThreadCollectionController archived threads", () => {
  beforeEach(() => {
    api.listThreadWindow.mockReset();
  });

  it("drops an archive load that resolves after the window was reset", async () => {
    const stale = deferred<ChatThreadShellRead[]>();
    api.listThreadWindow.mockReturnValueOnce(stale.promise);
    const controller = createController();

    const staleLoad = controller.ensureArchived();
    controller.resetWindow();
    stale.resolve([archivedThread("stale-thread")]);
    await staleLoad;

    expect(controller.archivedThreads).toEqual([]);
    expect(controller.archivedThreadsLoading).toBe(false);

    api.listThreadWindow.mockResolvedValueOnce([archivedThread("current-thread")]);
    await controller.ensureArchived();

    expect(api.listThreadWindow).toHaveBeenCalledTimes(2);
    expect(controller.archivedThreads.map((thread) => thread.id)).toEqual(["current-thread"]);
  });

  it("keeps a fresh load's loading state when a stale load settles later", async () => {
    const stale = deferred<ChatThreadShellRead[]>();
    const fresh = deferred<ChatThreadShellRead[]>();
    api.listThreadWindow.mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise);
    const controller = createController();

    const staleLoad = controller.ensureArchived();
    controller.reset();
    const freshLoad = controller.ensureArchived();
    stale.resolve([archivedThread("stale-thread")]);
    await staleLoad;

    expect(controller.archivedThreadsLoading).toBe(true);
    expect(controller.archivedThreads).toEqual([]);

    fresh.resolve([archivedThread("fresh-thread")]);
    await freshLoad;

    expect(controller.archivedThreadsLoading).toBe(false);
    expect(controller.archivedThreads.map((thread) => thread.id)).toEqual(["fresh-thread"]);
  });
});

describe("ChatThreadCollectionController changes for other windows", () => {
  beforeEach(() => {
    api.renameThread.mockReset();
    api.deleteThread.mockReset();
  });

  it("reports a renamed thread after storing it", async () => {
    const onChanged = vi.fn();
    const controller = createController({ onChanged });
    const thread = { ...archivedThread("thread-1"), archivedAt: null };
    controller.upsert(thread);
    api.renameThread.mockResolvedValueOnce({ ...thread, title: "Renamed", revision: 2 });

    await controller.rename(thread, "Renamed");

    expect(controller.activeThreads.map((entry) => entry.title)).toEqual(["Renamed"]);
    expect(onChanged).toHaveBeenCalledWith(["thread-1"]);
  });

  it("does not report a rename the backend refused", async () => {
    const onChanged = vi.fn();
    const controller = createController({ onChanged });
    const thread = { ...archivedThread("thread-1"), archivedAt: null };
    api.renameThread.mockRejectedValueOnce({ code: "stale_revision" });

    await expect(controller.rename(thread, "Renamed")).rejects.toEqual({ code: "stale_revision" });

    expect(onChanged).not.toHaveBeenCalled();
  });

  it("removes a deleted thread, clears its selection, and reports it", async () => {
    const onChanged = vi.fn();
    const selectThread = vi.fn();
    const controller = createController({ onChanged, selectThread, selectedThreadId: () => "thread-1" });
    controller.upsert(archivedThread("thread-1"));
    controller.upsert(archivedThread("thread-2"));
    api.deleteThread.mockResolvedValueOnce(undefined);

    await controller.delete(archivedThread("thread-1"));

    expect(controller.archivedThreads.map((thread) => thread.id)).toEqual(["thread-2"]);
    expect(selectThread).toHaveBeenCalledWith(null);
    expect(onChanged).toHaveBeenCalledWith(["thread-1"]);
  });

  it("removes a thread another window deleted without reporting it again", () => {
    const onChanged = vi.fn();
    const selectThread = vi.fn();
    const controller = createController({ onChanged, selectThread, selectedThreadId: () => "thread-2" });
    controller.upsert(archivedThread("thread-1"));

    controller.remove("thread-1");

    expect(controller.archivedThreads).toEqual([]);
    expect(selectThread).not.toHaveBeenCalled();
    expect(onChanged).not.toHaveBeenCalled();
  });
});

function createController(
  overrides: Partial<ConstructorParameters<typeof ChatThreadCollectionController>[0]> = {},
): ChatThreadCollectionController {
  return new ChatThreadCollectionController({
    selectedThreadId: () => null,
    selectThread: vi.fn(),
    onChanged: vi.fn(),
    ...overrides,
  });
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

function archivedThread(id: string): ChatThreadShellRead {
  return {
    id,
    workingFolderId: "working-folder-1",
    executionEnvironmentId: "current-folder:working-folder-1",
    scratchGenerationId: null,
    projectId: "project-1",
    title: "Thread",
    providerFamilyId: "codex",
    providerInstanceId: "codex-1",
    providerThreadId: `provider-${id}`,
    modelId: "gpt-5.6-sol",
    modelOptions: [],
    modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
    state: "active",
    latestTurnState: "active",
    latestPreview: null,
    messageCount: 1,
    revision: 1,
    lastEventSequence: 0,
    lastActivityAt: "2026-08-04T12:00:00.000Z",
    unreadAt: null,
    archivedAt: "2026-08-04T13:00:00.000Z",
  };
}
