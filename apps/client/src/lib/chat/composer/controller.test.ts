import { describe, expect, it, vi } from "vitest";
import type { ChatDraftRead } from "$lib/chat/contracts";
import { ChatComposerController, chatDraftId, type ChatDraftApi } from "./controller";

const timestamp = "2026-07-21T12:00:00Z";

function draft(overrides: Partial<ChatDraftRead> = {}): ChatDraftRead {
  return {
    id: chatDraftId("workspace-1", null),
    workingFolderId: "workspace-1",
    threadId: null,
    text: "Saved prompt",
    richContent: null,
    attachmentIds: [],
    mentions: { schemaVersion: 1, value: [] },
    providerInstanceId: "codex-personal",
    modelSelection: null,
    safetyMode: "ask_for_approval",
    interactionMode: "build",
    sentSnapshot: null,
    updatedAt: timestamp,
    ...overrides,
  };
}

function fakeApi(overrides: Partial<ChatDraftApi> = {}): ChatDraftApi {
  return {
    read: vi.fn(async () => null),
    save: vi.fn(async (value) => draft({ ...value, updatedAt: timestamp })),
    delete: vi.fn(async () => true),
    ...overrides,
  };
}

describe("ChatComposerController", () => {
  it("hydrates the same draft for the hero and docked composer", async () => {
    const api = fakeApi({ read: vi.fn(async () => draft()) });
    const controller = new ChatComposerController(api);

    await controller.bind("workspace-1", null);

    expect(controller.snapshot()).toMatchObject({
      text: "Saved prompt",
      providerInstanceId: "codex-personal",
      dirty: false,
    });
  });

  it("keeps channel text but clears folder references when its target folder changes", async () => {
    const api = fakeApi({
      read: vi.fn(async () => draft({
        id: "channel:planning",
        attachmentIds: ["attachment-1"],
        mentions: { schemaVersion: 1, value: [{ relativePath: "old.ts", kind: "file", ignored: false }] },
      })),
    });
    const controller = new ChatComposerController(api);

    await controller.bind("workspace-2", "thread-2", null, "channel:planning");

    expect(controller.snapshot()).toMatchObject({
      draftId: "channel:planning",
      workingFolderId: "workspace-2",
      threadId: "thread-2",
      text: "Saved prompt",
      attachmentIds: [],
      mentions: [],
      dirty: true,
    });
  });

  it("uses the current thread selection while an empty thread draft loads", async () => {
    let resolveRead: ((value: ChatDraftRead | null) => void) | undefined;
    const api = fakeApi({
      read: vi.fn(() => new Promise<ChatDraftRead | null>((resolve) => { resolveRead = resolve; })),
    });
    const controller = new ChatComposerController(api);
    const modelSelection = { schemaVersion: 1, value: { modelId: "gpt-5.3-codex-spark" } };

    const binding = controller.bind("workspace-1", "thread-1", {
      providerInstanceId: "codex-personal",
      modelSelection,
      safetyMode: "ask_for_approval",
      interactionMode: "build",
    });
    await vi.waitFor(() => expect(api.read).toHaveBeenCalledWith(chatDraftId("workspace-1", "thread-1")));

    expect(controller.snapshot()).toMatchObject({
      threadId: "thread-1",
      providerInstanceId: "codex-personal",
      modelSelection,
      loading: true,
    });
    resolveRead?.(null);
    await binding;
    expect(controller.snapshot()).toMatchObject({
      providerInstanceId: "codex-personal",
      modelSelection,
      loading: false,
    });
  });

  it("debounces edits and flushes before changing conversations", async () => {
    vi.useFakeTimers();
    const api = fakeApi();
    const controller = new ChatComposerController(api, 100);
    await controller.bind("workspace-1", null);
    controller.setText("First prompt");
    controller.setText("Latest prompt");

    await vi.advanceTimersByTimeAsync(99);
    expect(api.save).not.toHaveBeenCalled();
    const bind = controller.bind("workspace-1", "thread-2");
    await bind;

    expect(api.save).toHaveBeenCalledTimes(1);
    expect(api.save).toHaveBeenCalledWith(expect.objectContaining({ text: "Latest prompt" }));
    expect(controller.snapshot().threadId).toBe("thread-2");
    vi.useRealTimers();
  });

  it("keeps a recoverable sent snapshot when clearing submitted content", async () => {
    const api = fakeApi();
    const controller = new ChatComposerController(api);
    await controller.bind("workspace-1", null);
    const richContent = {
      schemaVersion: 1,
      value: { lines: [{ runs: [{ text: "Please", marks: ["bold"] }] }] },
    };
    controller.setRichContent("**Please** update the calendar", richContent);
    controller.setAttachments(["attachment-1"]);
    controller.setMentions([{ relativePath: "src/calendar.ts", kind: "file", ignored: false }]);

    controller.markSent();
    await controller.flush();

    const saved = vi.mocked(api.save).mock.calls.at(-1)?.[0];
    expect(saved).toMatchObject({ text: "", mentions: { value: [] } });
    expect(saved?.sentSnapshot?.value).toMatchObject({
      text: "**Please** update the calendar",
      richContent,
      attachmentIds: ["attachment-1"],
      mentions: [{ relativePath: "src/calendar.ts", kind: "file", ignored: false }],
    });
    expect(controller.restoreSentSnapshot()).toBe(true);
    expect(controller.snapshot()).toMatchObject({
      text: "**Please** update the calendar",
      richContent,
      attachmentIds: ["attachment-1"],
      mentions: [{ relativePath: "src/calendar.ts" }],
    });
  });

  it("ignores a stale load after another draft is selected", async () => {
    let resolveFirst: ((value: ChatDraftRead | null) => void) | undefined;
    const first = new Promise<ChatDraftRead | null>((resolve) => { resolveFirst = resolve; });
    const api = fakeApi({
      read: vi.fn()
        .mockReturnValueOnce(first)
        .mockResolvedValueOnce(draft({ id: chatDraftId("workspace-2", null), workingFolderId: "workspace-2", text: "New" })),
    });
    const controller = new ChatComposerController(api);

    const stale = controller.bind("workspace-1", null);
    await vi.waitFor(() => expect(api.read).toHaveBeenCalledWith(chatDraftId("workspace-1", null)));
    const current = controller.bind("workspace-2", null);
    await current;
    resolveFirst?.(draft({ text: "Stale" }));
    await stale;

    expect(controller.snapshot()).toMatchObject({ workingFolderId: "workspace-2", text: "New" });
  });

  it("ignores a stale load after the vault binding is reset", async () => {
    let resolveRead: ((value: ChatDraftRead | null) => void) | undefined;
    const api = fakeApi({
      read: vi.fn(() => new Promise<ChatDraftRead | null>((resolve) => { resolveRead = resolve; })),
    });
    const controller = new ChatComposerController(api);

    const stale = controller.bind("workspace-1", null);
    await vi.waitFor(() => expect(api.read).toHaveBeenCalled());
    controller.reset();
    resolveRead?.(draft());
    await stale;

    expect(controller.snapshot()).toMatchObject({
      draftId: null,
      workingFolderId: null,
      threadId: null,
      text: "",
    });
  });

  it("supersedes a bind that is still flushing the previous draft", async () => {
    let resolveSave: ((value: ChatDraftRead) => void) | undefined;
    const api = fakeApi({
      save: vi.fn(() => new Promise<ChatDraftRead>((resolve) => {
        resolveSave = resolve;
      })),
      read: vi.fn(async (id) => draft({
        id,
        workingFolderId: id.includes("workspace-2") ? "workspace-2" : "workspace-1",
        threadId: id.endsWith(":new") ? null : "thread-2",
        text: id.includes("workspace-2") ? "Current" : "Stale",
      })),
    });
    const controller = new ChatComposerController(api);
    await controller.bind("workspace-1", null);
    controller.setText("Unsaved");

    const stale = controller.bind("workspace-1", "thread-2");
    const current = controller.bind("workspace-2", null);
    await Promise.resolve();
    resolveSave?.(draft({ text: "Unsaved" }));
    await Promise.all([stale, current]);

    expect(controller.snapshot()).toMatchObject({ workingFolderId: "workspace-2", text: "Current" });
    expect(api.read).toHaveBeenLastCalledWith(chatDraftId("workspace-2", null));
  });
});
