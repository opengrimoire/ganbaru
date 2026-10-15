import { describe, expect, it, vi } from "vitest";
import type { ChatDraftRead } from "$lib/chat/contracts";
import { ChatComposerController, chatDraftId, type ChatDraftApi } from "./controller";
import type { ChatDraftWindowSync } from "./window-sync";

vi.mock("./window-sync", () => ({
  chatDraftWindowSync: { publish: vi.fn(), listen: vi.fn(async () => () => undefined) },
}));

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

function fakeSync(): { sync: ChatDraftWindowSync; changeElsewhere: (draftId: string) => void } {
  let onChange: ((draftIds: ReadonlySet<string>) => void) | null = null;
  return {
    sync: {
      publish: vi.fn(),
      listen: vi.fn(async (listener: (draftIds: ReadonlySet<string>) => void) => {
        onChange = listener;
        return () => { onChange = null; };
      }),
    },
    changeElsewhere: (draftId) => onChange?.(new Set([draftId])),
  };
}

const staleDraft = { code: "stale_revision", message: "Chat draft changed in another window", recoverable: true };

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
    await vi.waitFor(() => expect(api.save).toHaveBeenCalled());
    resolveSave?.(draft({ text: "Unsaved" }));
    await Promise.all([stale, current]);

    expect(controller.snapshot()).toMatchObject({ workingFolderId: "workspace-2", text: "Current" });
    expect(api.read).toHaveBeenLastCalledWith(chatDraftId("workspace-2", null));
  });

  it("bases each save on the version this window last read or stored", async () => {
    const { sync } = fakeSync();
    const api = fakeApi({
      save: vi.fn()
        .mockImplementationOnce(async (value) => draft({ ...value, updatedAt: "2026-07-21T12:00:01.000Z" }))
        .mockImplementationOnce(async (value) => draft({ ...value, updatedAt: "2026-07-21T12:00:02.000Z" })),
    });
    const controller = new ChatComposerController(api, 0, sync);
    await controller.bind("workspace-1", null);

    controller.setText("First");
    await controller.flush();
    controller.setText("Second");
    await controller.flush();

    expect(vi.mocked(api.save).mock.calls.map(([saved]) => saved.base)).toEqual([
      { kind: "none" },
      { kind: "saved", updatedAt: "2026-07-21T12:00:01.000Z" },
    ]);
    expect(sync.publish).toHaveBeenCalledWith(chatDraftId("workspace-1", null));
  });

  it("stores a send whatever another window saved", async () => {
    const api = fakeApi({ read: vi.fn(async () => draft()) });
    const controller = new ChatComposerController(api, 0, fakeSync().sync);
    await controller.bind("workspace-1", null);

    controller.markSent();
    await controller.flush();
    controller.setText("Next");
    await controller.flush();

    expect(vi.mocked(api.save).mock.calls.map(([saved]) => saved.base)).toEqual([
      { kind: "any" },
      { kind: "saved", updatedAt: timestamp },
    ]);
  });

  it("replaces local edits with the stored draft when another window saved first", async () => {
    const elsewhere = draft({ text: "From the other window", updatedAt: "2026-07-21T12:00:05.000Z" });
    const api = fakeApi({
      read: vi.fn().mockResolvedValueOnce(draft()).mockResolvedValueOnce(elsewhere),
      save: vi.fn()
        .mockRejectedValueOnce(staleDraft)
        .mockImplementationOnce(async (value) => draft({ ...value, updatedAt: "2026-07-21T12:00:06.000Z" })),
    });
    const controller = new ChatComposerController(api, 0, fakeSync().sync);
    await controller.bind("workspace-1", null);

    controller.setText("Local edit");
    await expect(controller.flush()).resolves.toBeUndefined();

    expect(controller.snapshot()).toMatchObject({
      text: "From the other window",
      dirty: false,
      saving: false,
      changedElsewhere: true,
      error: null,
    });
    controller.setText("Building on it");
    expect(controller.snapshot().changedElsewhere).toBe(false);
    await controller.flush();
    expect(vi.mocked(api.save).mock.calls.at(-1)?.[0].base).toEqual({
      kind: "saved",
      updatedAt: "2026-07-21T12:00:05.000Z",
    });
  });

  it("drops a save queued before a conflict replaced the draft", async () => {
    let rejectFirst: ((reason: unknown) => void) | undefined;
    const api = fakeApi({
      read: vi.fn().mockResolvedValueOnce(null).mockResolvedValueOnce(draft({ text: "From the other window" })),
      save: vi.fn(() => new Promise<ChatDraftRead>((_resolve, reject) => { rejectFirst = reject; })),
    });
    const controller = new ChatComposerController(api, 0, fakeSync().sync);
    await controller.bind("workspace-1", null);

    controller.setText("First");
    const first = controller.flush();
    controller.setText("Second");
    const second = controller.flush();
    await vi.waitFor(() => expect(api.save).toHaveBeenCalledTimes(1));
    rejectFirst?.(staleDraft);
    await Promise.all([first, second]);

    expect(api.save).toHaveBeenCalledTimes(1);
    expect(controller.snapshot()).toMatchObject({ text: "From the other window", dirty: false, saving: false });
  });

  it("shows another window's save while this composer has no unsaved edits", async () => {
    const { sync, changeElsewhere } = fakeSync();
    const api = fakeApi({
      read: vi.fn().mockResolvedValueOnce(draft()).mockResolvedValueOnce(draft({ text: "Typed elsewhere" })),
    });
    const controller = new ChatComposerController(api, 0, sync);
    await controller.bind("workspace-1", null);

    changeElsewhere(chatDraftId("workspace-1", "thread-9"));
    changeElsewhere(chatDraftId("workspace-1", null));

    await vi.waitFor(() => expect(controller.snapshot().text).toBe("Typed elsewhere"));
    expect(api.read).toHaveBeenCalledTimes(2);
    expect(controller.snapshot()).toMatchObject({ dirty: false, changedElsewhere: false });
  });

  it("keeps unsaved edits when another window saves the same draft", async () => {
    vi.useFakeTimers();
    const { sync, changeElsewhere } = fakeSync();
    const api = fakeApi({ read: vi.fn(async () => draft()) });
    const controller = new ChatComposerController(api, 100, sync);
    await controller.bind("workspace-1", null);

    controller.setText("Still typing");
    changeElsewhere(chatDraftId("workspace-1", null));
    await vi.advanceTimersByTimeAsync(0);

    expect(api.read).toHaveBeenCalledTimes(1);
    expect(controller.snapshot()).toMatchObject({ text: "Still typing", dirty: true });
    vi.useRealTimers();
  });
});
