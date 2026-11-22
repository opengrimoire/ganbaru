// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatThreadShellRead } from "$lib/chat/contracts";
import { getChat } from "$lib/stores/chat.svelte";
import ChatFirstUse from "./ChatFirstUse.svelte";

vi.mock("$lib/stores/projects.svelte", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/stores/projects.svelte")>(),
  useProjects: () => ({ selectedProjectId: "project:test", selectedProject: null }),
}));

const ARCHIVED_AT = "2026-08-12T20:00:00.000Z";

/** An archived direct thread with the given id. */
function archivedThread(id: string): ChatThreadShellRead {
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
    lastActivityAt: ARCHIVED_AT,
    unreadAt: null,
    archivedAt: ARCHIVED_AT,
  };
}

describe("ChatFirstUse", () => {
  const chat = getChat();
  const originalArchivedThreads = chat.archivedThreads;
  const originalSelectedThreadId = chat.selectedThreadId;
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    chat.archivedThreads = [archivedThread("thread:first"), archivedThread("thread:second")];
    chat.selectedThreadId = "thread:first";
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    chat.archivedThreads = originalArchivedThreads;
    chat.selectedThreadId = originalSelectedThreadId;
    vi.restoreAllMocks();
  });

  /** Mounts the first-use surface and returns its container. */
  async function mountFirstUse(): Promise<HTMLDivElement> {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatFirstUse, { target });
    await tick();
    return target;
  }

  /** Lets a rejected action settle and the surface rerender. */
  async function settle(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 0));
    await tick();
  }

  it("shows a failed restore on the thread it was for", async () => {
    vi.spyOn(chat, "restoreThread").mockRejectedValue(new Error("Restore failed"));
    const root = await mountFirstUse();

    root.querySelector<HTMLButtonElement>(".chat-primary-button")?.click();
    await settle();

    expect(root.querySelector("[role='alert']")).not.toBeNull();
  });

  it("keeps a restore that fails after the selection moved off the newly shown thread", async () => {
    let rejectRestore: (reason: unknown) => void = () => {};
    vi.spyOn(chat, "restoreThread").mockImplementation(() => new Promise<void>((_, reject) => {
      rejectRestore = reject;
    }));
    const root = await mountFirstUse();

    root.querySelector<HTMLButtonElement>(".chat-primary-button")?.click();
    chat.selectedThreadId = "thread:second";
    await tick();
    rejectRestore(new Error("Restore failed"));
    await settle();

    expect(root.querySelector("[role='alert']")).toBeNull();
  });

  it("leaves a failed restore behind when another thread is selected", async () => {
    vi.spyOn(chat, "restoreThread").mockRejectedValue(new Error("Restore failed"));
    const root = await mountFirstUse();

    root.querySelector<HTMLButtonElement>(".chat-primary-button")?.click();
    await settle();
    chat.selectedThreadId = "thread:second";
    await tick();

    expect(root.querySelector("[role='alert']")).toBeNull();
  });
});
