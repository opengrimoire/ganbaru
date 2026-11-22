// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatRestorePreviewRead, ChatThreadShellRead } from "$lib/chat/contracts";
import type { RestoreChatCheckpointOptions } from "$lib/chat/review/checkpoint-restoration";
import { getChat } from "$lib/stores/chat.svelte";
import { getProjects } from "$lib/stores/projects.svelte";
import ChatWorkspace from "./ChatWorkspace.svelte";

const restoreCalls = vi.hoisted(() => [] as RestoreChatCheckpointOptions[]);

vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

vi.mock("$lib/chat/review/checkpoint-restoration", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/chat/review/checkpoint-restoration")>(),
  restoreChatCheckpoint: vi.fn((options: RestoreChatCheckpointOptions) => {
    restoreCalls.push(options);
    return new Promise<void>(() => {});
  }),
}));

const TIMESTAMP = "2026-08-12T20:00:00.000Z";
const EXPIRES_AT = "2099-01-01T00:00:00.000Z";

/** A direct thread with the given id. */
function thread(id: string): ChatThreadShellRead {
  return {
    id,
    workingFolderId: "working-folder:test",
    executionEnvironmentId: "current-folder:working-folder:test",
    scratchGenerationId: null,
    projectId: "project:test",
    title: id,
    providerFamilyId: "codex",
    providerInstanceId: "codex",
    providerThreadId: null,
    modelId: null,
    modelOptions: [],
    modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
    state: "idle",
    latestTurnState: null,
    latestPreview: null,
    messageCount: 1,
    revision: 1,
    lastEventSequence: 1,
    lastActivityAt: TIMESTAMP,
    unreadAt: null,
    archivedAt: null,
  };
}

/** A restore preview for the given checkpoint. */
function preview(checkpointId: string): ChatRestorePreviewRead {
  return {
    previewId: `preview:${checkpointId}`,
    checkpointId,
    expiresAt: EXPIRES_AT,
    files: [],
    stagedChanges: false,
    providerRollback: "supported",
    warnings: [],
  };
}

/** A media query that never matches. */
function matchMediaStub(query: string): MediaQueryList {
  return {
    matches: false,
    media: query,
    onchange: null,
    addListener: () => undefined,
    removeListener: () => undefined,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    dispatchEvent: () => true,
  };
}

/** A confirmation answer that records whether it has settled. */
function track(answer: Promise<boolean>): { settled: boolean; value: boolean | null } {
  const state: { settled: boolean; value: boolean | null } = { settled: false, value: null };
  void answer.then((value) => {
    state.settled = true;
    state.value = value;
  });
  return state;
}

/** Lets pending promises settle and the workspace rerender. */
async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await tick();
}

describe("ChatWorkspace", () => {
  const chat = getChat();
  const projects = getProjects();
  const originalActiveThreads = chat.activeThreads;
  const originalSelectedThreadId = chat.selectedThreadId;
  const originalSelectedProjectId = projects.selectedProjectId;
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.stubGlobal("matchMedia", matchMediaStub);
    vi.stubGlobal("ResizeObserver", class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    });
    vi.spyOn(chat, "ensureLoaded").mockResolvedValue(undefined);
    vi.spyOn(projects, "ensureLoaded").mockResolvedValue(undefined);
    vi.spyOn(chat, "syncProjectSelection").mockResolvedValue(undefined);
    restoreCalls.length = 0;
    chat.activeThreads = [thread("thread:first"), thread("thread:second")];
    chat.selectedThreadId = "thread:first";
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    chat.activeThreads = originalActiveThreads;
    chat.selectedThreadId = originalSelectedThreadId;
    projects.selectedProjectId = originalSelectedProjectId;
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  /** Mounts the workspace and returns its container. */
  async function mountWorkspace(): Promise<HTMLDivElement> {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatWorkspace, { target });
    await settle();
    return target;
  }

  it("drops a project's sync failure once another project is selected", async () => {
    vi.mocked(chat.syncProjectSelection).mockImplementation((projectId) => projectId === "project:failing"
      ? Promise.reject(new Error("Sync failed"))
      : Promise.resolve());
    projects.selectedProjectId = "project:failing";
    const root = await mountWorkspace();
    expect(root.querySelector("main [role='alert']")).not.toBeNull();

    projects.selectedProjectId = "project:working";
    await settle();

    expect(root.querySelector("main [role='alert']")).toBeNull();
  });
});
