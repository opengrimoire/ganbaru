import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatAttachmentRead,
  ChatDraftRead,
  ChatInteractionStateRead,
  ChatSettingsRead,
  ChatThreadShellRead,
  ResolveChatApprovalCommand,
  ResolveChatUserInputCommand,
  SaveChatDraftRequest,
  SendChatTurnCommand,
  SendChatTurnResult,
} from "$lib/chat/contracts";
import { CHAT_IMAGE_LIMIT } from "$lib/chat/composer/model";
import { ChatComposerRuntimeController } from "./composer-runtime-controller.svelte";

const api = vi.hoisted(() => ({
  readInteraction: vi.fn<(threadId: string) => Promise<ChatInteractionStateRead>>(),
  resolveApproval: vi.fn<(request: ResolveChatApprovalCommand) => Promise<void>>(),
  resolveUserInput: vi.fn<(request: ResolveChatUserInputCommand) => Promise<void>>(),
  readDraft: vi.fn<(draftId: string) => Promise<ChatDraftRead | null>>(),
  saveDraft: vi.fn<(draft: SaveChatDraftRequest) => Promise<ChatDraftRead>>(),
  sendTurn: vi.fn<(request: SendChatTurnCommand) => Promise<SendChatTurnResult>>(),
  rememberSelection: vi.fn<() => Promise<unknown>>(),
  readSettings: vi.fn<() => Promise<ChatSettingsRead>>(),
  importImage: vi.fn<() => Promise<ChatAttachmentRead>>(),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  readChatInteractionState: api.readInteraction,
  resolveChatApproval: api.resolveApproval,
  resolveChatUserInput: api.resolveUserInput,
  readChatDraft: api.readDraft,
  saveChatDraft: api.saveDraft,
  sendChatTurn: api.sendTurn,
  rememberChatComposerSelection: api.rememberSelection,
  readChatSettings: api.readSettings,
  importChatImage: api.importImage,
}));

describe("ChatComposerRuntimeController sending", () => {
  beforeEach(() => {
    api.readInteraction.mockReset().mockResolvedValue(interaction(null));
    api.readDraft.mockReset().mockResolvedValue(composerDraft());
    api.saveDraft.mockReset().mockImplementation(async (draft) => ({ ...draft, updatedAt: "2026-08-04T12:00:00.000Z" }));
    api.sendTurn.mockReset().mockResolvedValue({ thread: threadShell(), dispatch: null, launchError: null });
    api.rememberSelection.mockReset();
    api.readSettings.mockReset();
    api.importImage.mockReset();
  });

  it("resolves a sent turn when remembering the composer selection fails", async () => {
    api.rememberSelection.mockRejectedValue(new Error("Chat config is read-only"));
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const options = controllerOptions();
    const controller = new ChatComposerRuntimeController(options);
    await controller.bind("working-folder-1", "thread-1");

    await expect(controller.send()).resolves.toBeUndefined();

    expect(api.sendTurn).toHaveBeenCalledTimes(1);
    expect(options.upsertThread).toHaveBeenCalledWith(threadShell());
    expect(options.setSettings).not.toHaveBeenCalled();
    expect(controller.sendError).toBeNull();
    expect(api.readInteraction).toHaveBeenCalledWith("thread-1");
    expect(consoleError).toHaveBeenCalledTimes(1);
    consoleError.mockRestore();
  });

  it("rejects pasted or dropped images beyond the attachment limit before importing", async () => {
    const controller = new ChatComposerRuntimeController(controllerOptions());
    await controller.bind("working-folder-1", "thread-1");
    const files = Array.from(
      { length: CHAT_IMAGE_LIMIT + 1 },
      (_, index) => new File([new Uint8Array([index])], `image-${index}.png`, { type: "image/png" }),
    );

    await expect(controller.importImages(files)).rejects.toThrow();

    expect(api.importImage).not.toHaveBeenCalled();
  });
});

describe("ChatComposerRuntimeController interactions", () => {
  beforeEach(() => {
    api.readInteraction.mockReset();
    api.resolveApproval.mockReset().mockResolvedValue();
    api.resolveUserInput.mockReset().mockResolvedValue();
  });

  it("resolves pending requests without a streaming-sensitive thread revision", async () => {
    const thread = threadShell();
    const controller = new ChatComposerRuntimeController({
      selectedThread: () => thread,
      selectedThreadId: () => thread.id,
      selectedWorkingFolderId: () => thread.workingFolderId,
      selectedExecutionEnvironmentId: () => null,
      setSelectedThreadId: vi.fn(),
      upsertThread: vi.fn(),
      loadTimeline: vi.fn(async () => undefined),
      clearTimeline: vi.fn(),
      setSettings: vi.fn(),
      onComposerChanged: vi.fn(),
    });
    controller.interaction = interaction("approval");
    api.readInteraction.mockResolvedValue(interaction(null));

    await controller.resolveApproval({
      kind: "allow_once",
      providerOptionId: "accept",
      updatedToolInput: null,
    });

    expect(api.resolveApproval).toHaveBeenCalledWith(expect.objectContaining({
      command: expect.objectContaining({ expectedThreadRevision: null }),
      threadId: thread.id,
      requestId: "request-1",
      providerRequestId: "provider-request-1",
    }));

    controller.interaction = interaction("user_input");
    await controller.resolveUserInput([{
      questionId: "scope",
      selectedOptionIds: ["tests"],
      freeFormText: null,
    }]);

    expect(api.resolveUserInput).toHaveBeenCalledWith(expect.objectContaining({
      command: expect.objectContaining({ expectedThreadRevision: null }),
      threadId: thread.id,
      requestId: "request-1",
      providerRequestId: "provider-request-1",
    }));
  });
});

function interaction(requestKind: "approval" | "user_input" | null): ChatInteractionStateRead {
  return {
    sessionId: "session-1",
    sessionState: requestKind === "approval" ? "waiting_for_approval"
      : requestKind === "user_input" ? "waiting_for_user_input"
      : "active",
    activeTurnId: "turn-1",
    capabilities: { entries: [] },
    pendingRequest: requestKind ? {
      id: "request-1",
      turnId: "turn-1",
      providerRequestId: "provider-request-1",
      requestKind,
      safeDisplay: { schemaVersion: 1, value: {} },
      allowedDecisions: { schemaVersion: 1, value: [] },
      openedAt: "2026-08-04T12:00:00.000Z",
    } : null,
    queuedFollowup: null,
    usage: null,
    accountStatus: null,
    rateLimitStatus: null,
    automaticCompactionReported: false,
  };
}

function controllerOptions() {
  const thread = threadShell();
  return {
    selectedThread: () => thread,
    selectedThreadId: () => thread.id,
    selectedWorkingFolderId: () => thread.workingFolderId,
    selectedExecutionEnvironmentId: () => null,
    setSelectedThreadId: vi.fn(),
    upsertThread: vi.fn(),
    loadTimeline: vi.fn(async () => undefined),
    clearTimeline: vi.fn(),
    setSettings: vi.fn(),
    onComposerChanged: vi.fn(),
  };
}

function composerDraft(): ChatDraftRead {
  return {
    id: "draft-1",
    workingFolderId: "working-folder-1",
    threadId: "thread-1",
    text: "Run the tests",
    richContent: null,
    attachmentIds: [],
    mentions: { schemaVersion: 1, value: [] },
    providerInstanceId: "codex-1",
    modelSelection: { schemaVersion: 1, value: { modelId: "gpt-5.6-sol", providerManaged: false, options: [] } },
    safetyMode: "ask_for_approval",
    interactionMode: "build",
    sentSnapshot: null,
    updatedAt: "2026-08-04T12:00:00.000Z",
  };
}

function threadShell(): ChatThreadShellRead {
  return {
    id: "thread-1",
    workingFolderId: "working-folder-1",
    executionEnvironmentId: "current-folder:working-folder-1",
    scratchGenerationId: null,
    projectId: "project-1",
    title: "Thread",
    providerFamilyId: "codex",
    providerInstanceId: "codex-1",
    providerThreadId: "provider-thread-1",
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
    archivedAt: null,
  };
}
