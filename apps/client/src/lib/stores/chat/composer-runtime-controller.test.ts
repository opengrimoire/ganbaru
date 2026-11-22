import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatAttachmentRead,
  ChatDraftRead,
  ChatError,
  ChatInteractionStateRead,
  ChatPendingRequestRead,
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

  it("leaves a thread picked while a send was in flight selected and free of its launch error", async () => {
    let selected: string | null = "thread-1";
    const options = { ...controllerOptions(), selectedThreadId: () => selected };
    const controller = new ChatComposerRuntimeController(options);
    await controller.bind("working-folder-1", "thread-1");
    const sent = deferred<SendChatTurnResult>();
    api.sendTurn.mockReturnValueOnce(sent.promise);

    const sending = controller.send();
    await vi.waitFor(() => expect(api.sendTurn).toHaveBeenCalledTimes(1));
    selected = "thread-2";
    api.readDraft.mockResolvedValueOnce({ ...composerDraft(), id: "draft-2", threadId: "thread-2" });
    await controller.bind("working-folder-1", "thread-2");
    sent.resolve({ thread: threadShell(), dispatch: null, launchError: launchError() });
    await sending;

    expect(options.upsertThread).toHaveBeenCalledWith(threadShell());
    expect(options.setSelectedThreadId).not.toHaveBeenCalled();
    expect(controller.sendError).toBeNull();
  });

  it("selects the thread a send started and reports its launch error while the user stays", async () => {
    const options = controllerOptions();
    const controller = new ChatComposerRuntimeController(options);
    await controller.bind("working-folder-1", "thread-1");
    api.sendTurn.mockResolvedValueOnce({ thread: threadShell(), dispatch: null, launchError: launchError() });

    await controller.send();

    expect(options.setSelectedThreadId).toHaveBeenCalledWith("thread-1");
    expect(controller.sendError).not.toBeNull();
  });

  it("switches to the thread a draft send created while the draft stays shown", async () => {
    const options = { ...controllerOptions(), selectedThread: () => null, selectedThreadId: () => null };
    const controller = new ChatComposerRuntimeController(options);
    api.readDraft.mockResolvedValueOnce({ ...composerDraft(), id: "draft-new", threadId: null });
    await controller.bind("working-folder-1", null);
    api.sendTurn.mockImplementationOnce(async (request) => ({
      thread: { ...threadShell(), id: request.newThreadId ?? "missing" },
      dispatch: null,
      launchError: null,
    }));

    await controller.send();

    const createdId = api.sendTurn.mock.calls[0]?.[0].newThreadId;
    expect(createdId).toEqual(expect.any(String));
    expect(options.setSelectedThreadId).toHaveBeenCalledWith(createdId);
    expect(controller.draftThreadId).toBeNull();
  });

  it("stays on a thread picked during a draft send but never reuses the consumed draft thread ID", async () => {
    let selected: string | null = null;
    const options = { ...controllerOptions(), selectedThread: () => null, selectedThreadId: () => selected };
    const controller = new ChatComposerRuntimeController(options);
    api.readDraft.mockResolvedValueOnce({ ...composerDraft(), id: "draft-new", threadId: null });
    await controller.bind("working-folder-1", null);
    const sent = deferred<SendChatTurnResult>();
    api.sendTurn.mockReturnValueOnce(sent.promise);

    const sending = controller.send();
    await vi.waitFor(() => expect(api.sendTurn).toHaveBeenCalledTimes(1));
    selected = "thread-2";
    api.readDraft.mockResolvedValueOnce({ ...composerDraft(), id: "draft-2", threadId: "thread-2" });
    await controller.bind("working-folder-1", "thread-2");
    sent.resolve({ thread: threadShell(), dispatch: null, launchError: launchError() });
    await sending;

    expect(options.setSelectedThreadId).not.toHaveBeenCalled();
    expect(controller.sendError).toBeNull();
    expect(controller.draftThreadId).toBeNull();
  });

  it("keeps the text of a failed send out of a draft the user switched to meanwhile", async () => {
    const options = controllerOptions();
    const controller = new ChatComposerRuntimeController(options);
    await controller.bind("working-folder-1", "thread-1");
    const sent = deferred<SendChatTurnResult>();
    api.sendTurn.mockReturnValueOnce(sent.promise);

    const sending = controller.send();
    await vi.waitFor(() => expect(api.sendTurn).toHaveBeenCalledTimes(1));
    api.readDraft.mockResolvedValueOnce({
      ...composerDraft(),
      id: "draft-2",
      threadId: "thread-2",
      text: "Other draft",
      sentSnapshot: { schemaVersion: 1, value: { text: "Earlier prompt", richContent: null, attachmentIds: [], mentions: [] } },
    });
    await controller.bind("working-folder-1", "thread-2");
    sent.reject(new Error("Provider unavailable"));
    await expect(sending).rejects.toThrow("Provider unavailable");

    expect(controller.composer.threadId).toBe("thread-2");
    expect(controller.composer.text).toBe("Other draft");
    const savedSent = api.saveDraft.mock.calls.map(([draft]) => draft).filter((draft) => draft.id === "draft-1");
    expect(savedSent.at(-1)?.sentSnapshot?.value).toMatchObject({ text: "Run the tests" });
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

  it("keeps an image imported for a draft that was replaced meanwhile out of the shown draft", async () => {
    const controller = new ChatComposerRuntimeController(controllerOptions());
    await controller.bind("working-folder-1", "thread-1");
    const imported = deferred<ChatAttachmentRead>();
    api.importImage.mockReturnValueOnce(imported.promise);

    const importing = controller.importImages([new File([new Uint8Array([1])], "image.png", { type: "image/png" })]);
    await vi.waitFor(() => expect(api.importImage).toHaveBeenCalledTimes(1));
    api.readDraft.mockResolvedValueOnce({ ...composerDraft(), id: "draft-2", threadId: "thread-2" });
    await controller.bind("working-folder-1", "thread-2");
    imported.resolve(attachmentRead("attachment-1"));
    await importing;

    expect(controller.composer.threadId).toBe("thread-2");
    expect(controller.composer.attachmentIds).toEqual([]);
    expect(controller.attachments).toEqual([]);
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
      refreshTimeline: vi.fn(async () => undefined),
      clearTimeline: vi.fn(),
      setSettings: vi.fn(),
      onComposerChanged: vi.fn(),
    });
    controller.interaction = interaction("approval");
    api.readInteraction.mockResolvedValue(interaction(null));
    const [pending] = controller.interaction.pendingRequests;

    await controller.resolveApproval(pending, {
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
    await controller.resolveUserInput(controller.interaction.pendingRequests[0], [{
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

  it("resolves each of several open approvals by its own request id", async () => {
    const controller = new ChatComposerRuntimeController(controllerOptions());
    const first = pendingRequest("approval", "1");
    const second = pendingRequest("approval", "2");
    controller.interaction = { ...interaction(null), pendingRequests: [first, second] };
    api.readInteraction.mockResolvedValueOnce({ ...interaction(null), pendingRequests: [second] })
      .mockResolvedValueOnce(interaction(null));
    const decision = { kind: "allow_once" as const, providerOptionId: "accept", updatedToolInput: null };

    await controller.resolveApproval(first, decision);
    expect(controller.interaction?.pendingRequests).toEqual([second]);
    await controller.resolveApproval(second, decision);

    expect(api.resolveApproval.mock.calls.map(([command]) => [command.requestId, command.providerRequestId])).toEqual([
      ["request-1", "provider-request-1"],
      ["request-2", "provider-request-2"],
    ]);
    expect(controller.interaction?.pendingRequests).toEqual([]);
  });
});

describe("ChatComposerRuntimeController interaction refresh", () => {
  beforeEach(() => {
    api.readInteraction.mockReset();
  });

  function selectable(initial: string | null) {
    let selected = initial;
    const options = { ...controllerOptions(), selectedThreadId: () => selected };
    return { options, select: (threadId: string | null) => { selected = threadId; } };
  }

  it("stops loading when the selection clears while a refresh is in flight", async () => {
    const { options, select } = selectable("thread-1");
    const controller = new ChatComposerRuntimeController(options);
    const read = deferred<ChatInteractionStateRead>();
    api.readInteraction.mockReturnValueOnce(read.promise);

    const pending = controller.refreshInteraction();
    expect(controller.interactionLoading).toBe(true);
    select(null);
    await controller.refreshInteraction();
    read.resolve(interaction("approval"));
    await pending;

    expect(controller.interactionLoading).toBe(false);
    expect(controller.interaction).toBeNull();
  });

  it("keeps the selected thread's refresh when a thread out of view is refreshed meanwhile", async () => {
    const { options } = selectable("thread-1");
    const controller = new ChatComposerRuntimeController(options);
    const read = deferred<ChatInteractionStateRead>();
    api.readInteraction.mockReturnValueOnce(read.promise).mockResolvedValueOnce(interaction("user_input"));

    const selected = controller.refreshInteraction("thread-1");
    await controller.refreshInteraction("thread-2");
    read.resolve(interaction("approval"));
    await selected;

    expect(controller.interaction).toEqual(interaction("approval"));
    expect(controller.interactionLoading).toBe(false);
  });

  it("drops the previous thread's interaction when the next thread's read fails", async () => {
    const { options, select } = selectable("thread-1");
    const controller = new ChatComposerRuntimeController(options);
    api.readInteraction.mockResolvedValueOnce(interaction("approval")).mockRejectedValueOnce(new Error("offline"));
    await controller.refreshInteraction();

    select("thread-2");
    await expect(controller.refreshInteraction()).rejects.toThrow("offline");

    expect(controller.interaction).toBeNull();
    expect(controller.interactionLoading).toBe(false);
  });

  it("keeps a thread's interaction while its own reread fails", async () => {
    const { options } = selectable("thread-1");
    const controller = new ChatComposerRuntimeController(options);
    api.readInteraction.mockResolvedValueOnce(interaction("approval")).mockRejectedValueOnce(new Error("offline"));
    await controller.refreshInteraction();

    await expect(controller.refreshInteraction()).rejects.toThrow("offline");

    expect(controller.interaction).toEqual(interaction("approval"));
  });
});

/** Creates a request the test settles by hand. */
function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  let reject: (error: unknown) => void = () => undefined;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

/** Builds the provider launch failure a send can report after the turn is stored. */
function launchError(): ChatError {
  return { code: "validation", message: "Provider failed to start", field: null, recoverable: true, details: null };
}

function pendingRequest(requestKind: "approval" | "user_input", suffix = "1"): ChatPendingRequestRead {
  return {
    id: `request-${suffix}`,
    turnId: "turn-1",
    providerRequestId: `provider-request-${suffix}`,
    requestKind,
    safeDisplay: { schemaVersion: 1, value: {} },
    allowedDecisions: { schemaVersion: 1, value: [] },
    openedAt: "2026-08-04T12:00:00.000Z",
    rememberable: null,
  };
}

function interaction(requestKind: "approval" | "user_input" | null): ChatInteractionStateRead {
  return {
    sessionId: "session-1",
    sessionState: requestKind === "approval" ? "waiting_for_approval"
      : requestKind === "user_input" ? "waiting_for_user_input"
      : "active",
    activeTurnId: "turn-1",
    capabilities: { entries: [] },
    pendingRequests: requestKind ? [pendingRequest(requestKind)] : [],
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
    refreshTimeline: vi.fn(async () => undefined),
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

function attachmentRead(id: string): ChatAttachmentRead {
  return {
    id,
    workingFolderId: "working-folder-1",
    kind: "image",
    originalDisplayName: "image.png",
    mimeType: "image/png",
    byteSize: 1,
    sha256: "0".repeat(64),
    managedRelativePath: `chat/attachments/${id}.png`,
    signatureKind: "png",
    createdAt: "2026-08-04T12:00:00.000Z",
  };
}
