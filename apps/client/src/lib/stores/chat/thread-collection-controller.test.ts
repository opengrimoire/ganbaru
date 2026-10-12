import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatThreadShellRead } from "$lib/chat/contracts";
import { ChatThreadCollectionController } from "./thread-collection-controller.svelte";

const api = vi.hoisted(() => ({
  listThreadWindow: vi.fn<
    (workingFolderId: string | null, archived: boolean, limit: number) => Promise<ChatThreadShellRead[]>
  >(),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  listChatThreadWindow: api.listThreadWindow,
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

function createController(): ChatThreadCollectionController {
  return new ChatThreadCollectionController({
    selectedThreadId: () => null,
    selectThread: vi.fn(),
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
