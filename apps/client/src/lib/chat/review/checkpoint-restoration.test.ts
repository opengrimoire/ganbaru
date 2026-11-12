import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatRestorePreviewRead, ChatRestoreResultRead } from "$lib/chat/contracts";
import { restoreChatCheckpoint } from "./checkpoint-restoration";

const api = vi.hoisted(() => ({
  preview: vi.fn<(threadId: string, checkpointId: string) => Promise<ChatRestorePreviewRead>>(),
  execute: vi.fn<(request: unknown) => Promise<ChatRestoreResultRead>>(),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  previewChatCheckpointRestore: api.preview,
  executeChatCheckpointRestore: api.execute,
}));

const preview: ChatRestorePreviewRead = {
  previewId: "preview-1",
  checkpointId: "checkpoint-1",
  expiresAt: "2026-11-12T10:15:00Z",
  files: [],
  stagedChanges: true,
  providerRollback: "unsupported",
  warnings: ["staging_restored", "provider_fork_required"],
};

const result: ChatRestoreResultRead = {
  checkpointId: "checkpoint-1",
  revertedTurnIds: [],
  providerHistoryAction: "fork_required",
  recoveryState: "complete",
  threadRevision: 8,
};

describe("checkpoint restoration", () => {
  beforeEach(() => {
    api.preview.mockReset().mockResolvedValue(preview);
    api.execute.mockReset().mockResolvedValue(result);
  });

  it("executes only after the confirmation resolves true", async () => {
    let answer: (confirmed: boolean) => void = () => undefined;
    const confirm = vi.fn(() => new Promise<boolean>((resolve) => { answer = resolve; }));
    const onRestored = vi.fn(async () => undefined);
    const restoring = restoreChatCheckpoint({
      thread: { id: "thread-1", revision: 7 },
      checkpointId: "checkpoint-1",
      confirm,
      onRestored,
    });
    await vi.waitFor(() => expect(confirm).toHaveBeenCalledWith(preview));
    await Promise.resolve();
    expect(api.execute).not.toHaveBeenCalled();

    answer(true);
    await restoring;
    expect(api.execute).toHaveBeenCalledWith(expect.objectContaining({
      threadId: "thread-1",
      previewId: "preview-1",
      confirmed: true,
      command: expect.objectContaining({ expectedThreadRevision: 7 }),
    }));
    expect(onRestored).toHaveBeenCalledWith("thread-1");
  });

  it("does not execute when the confirmation resolves false", async () => {
    const onRestored = vi.fn(async () => undefined);
    await restoreChatCheckpoint({
      thread: { id: "thread-1", revision: 7 },
      checkpointId: "checkpoint-1",
      confirm: async () => false,
      onRestored,
    });
    expect(api.preview).toHaveBeenCalledWith("thread-1", "checkpoint-1");
    expect(api.execute).not.toHaveBeenCalled();
    expect(onRestored).not.toHaveBeenCalled();
  });
});
