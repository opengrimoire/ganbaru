// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  ChatScratchDirectoryPageRead,
  ChatScratchGenerationRead,
  ChatScratchPromotionResultRead,
  ChatScratchScopeRead,
} from "$lib/chat/contracts";
import ChatScratchManager from "./ChatScratchManager.svelte";

const chatApi = vi.hoisted(() => ({
  listChatScratchScopes: vi.fn(),
  readChatChannelAgentSettings: vi.fn(),
  browseChatScratchGeneration: vi.fn(),
  promoteChatScratchFile: vi.fn(),
}));

const chat = vi.hoisted(() => ({ workingFolders: [] }));

vi.mock("$lib/api/chat", () => chatApi);
vi.mock("$lib/stores/chat.svelte", () => ({ getChat: () => chat }));

const generation = {
  id: "generation",
  executionEnvironmentId: "environment",
  generation: 1,
  lifecycleState: "active",
  byteSize: 10,
  entryCount: 2,
  sizeTruncated: false,
  deviceAvailability: "available",
  retainedSources: [],
  createdAt: "2026-11-01T00:00:00Z",
  updatedAt: "2026-11-01T00:00:00Z",
} as ChatScratchGenerationRead;

const scope = {
  id: "scope",
  replyThreadId: "reply-thread",
  agentId: "agent",
  agentName: "Agent",
  channelId: "channel",
  channelName: "general",
  projectId: "project",
  projectName: "Project",
  groupId: null,
  groupName: null,
  lifecycleState: "active",
  revision: 1,
  generations: [generation],
  createdAt: "2026-11-01T00:00:00Z",
  updatedAt: "2026-11-01T00:00:00Z",
} as ChatScratchScopeRead;

const file = (name: string) => ({
  relativePath: name,
  displayName: name,
  kind: "file" as const,
  byteSize: 5,
  contentRevision: `revision-${name}`,
  promotable: true,
});

const page = {
  scratchGenerationId: "generation",
  relativePath: "",
  entries: [file("first.txt"), file("second.txt")],
  nextCursor: null,
} as ChatScratchDirectoryPageRead;

/** Waits for pending promise callbacks and the renders they cause. */
async function settle(): Promise<void> {
  for (let pass = 0; pass < 5; pass += 1) await Promise.resolve();
  flushSync();
}

describe("ChatScratchManager", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    target = undefined;
    component = undefined;
    vi.clearAllMocks();
  });

  it("keeps a promotion form opened for another file while an earlier promotion finishes", async () => {
    chatApi.listChatScratchScopes.mockResolvedValue([scope]);
    chatApi.readChatChannelAgentSettings.mockRejectedValue(new Error("no access"));
    chatApi.browseChatScratchGeneration.mockResolvedValue(page);
    let finishPromotion!: (result: ChatScratchPromotionResultRead) => void;
    chatApi.promoteChatScratchFile.mockImplementationOnce(() => new Promise((resolve) => { finishPromotion = resolve; }));
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatScratchManager, { target, props: { onClose: vi.fn() } });
    await settle();

    target.querySelector<HTMLButtonElement>(".generation-select")?.click();
    await settle();
    const promoteButtons = () => [...(target?.querySelectorAll<HTMLButtonElement>(".promote-button") ?? [])];
    promoteButtons()[0]?.click();
    flushSync();
    target.querySelector<HTMLFormElement>("form.promotion")?.requestSubmit();
    await settle();
    expect(chatApi.promoteChatScratchFile).toHaveBeenCalledOnce();

    promoteButtons()[1]?.click();
    flushSync();
    finishPromotion({} as ChatScratchPromotionResultRead);
    await settle();

    expect(target.querySelector("form.promotion header p")?.textContent).toBe("second.txt");
  });

  it("reports a failed promotion after its form was closed", async () => {
    chatApi.listChatScratchScopes.mockResolvedValue([scope]);
    chatApi.readChatChannelAgentSettings.mockRejectedValue(new Error("no access"));
    chatApi.browseChatScratchGeneration.mockResolvedValue(page);
    let failPromotion!: (error: Error) => void;
    chatApi.promoteChatScratchFile.mockImplementationOnce(() => new Promise((_, reject) => { failPromotion = reject; }));
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatScratchManager, { target, props: { onClose: vi.fn() } });
    await settle();

    target.querySelector<HTMLButtonElement>(".generation-select")?.click();
    await settle();
    target.querySelector<HTMLButtonElement>(".promote-button")?.click();
    flushSync();
    target.querySelector<HTMLFormElement>("form.promotion")?.requestSubmit();
    await settle();
    target.querySelector<HTMLButtonElement>("form.promotion footer button[type='button']")?.click();
    flushSync();
    expect(target.querySelector("form.promotion")).toBeNull();
    failPromotion(new Error("disk full"));
    await settle();

    expect(target.querySelector(".browser .scratch-error")).not.toBeNull();
  });
});
