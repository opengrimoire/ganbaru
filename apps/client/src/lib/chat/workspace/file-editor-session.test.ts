// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectWorkingFolderFilePreview } from "$lib/chat/contracts";
import type { ConfirmationRequest } from "$lib/components/ui/confirmation.svelte";
import { ChatFileEditorSession, type ChatFileEditorSessionOptions } from "./file-editor-session.svelte";

const mocks = vi.hoisted(() => ({
  chat: {
    selectedWorkingFolderId: "folder-1" as string | null,
    selectedExecutionEnvironmentId: null as string | null,
  },
  confirm: vi.fn<(request: ConfirmationRequest) => Promise<boolean>>(),
  preview: vi.fn<(workingFolderId: string, path: string, environmentId: string | null) => Promise<unknown>>(),
  save: vi.fn<(request: unknown) => Promise<unknown>>(),
  recreate: vi.fn<(request: unknown) => Promise<unknown>>(),
}));

vi.mock("$lib/stores/chat.svelte", () => ({ useChat: () => mocks.chat }));
vi.mock("$lib/components/ui/confirmation.svelte", () => ({ requestConfirmation: mocks.confirm }));
vi.mock("$lib/api/chat", () => ({
  previewProjectWorkingFolderFile: mocks.preview,
  saveProjectWorkingFolderFile: mocks.save,
  recreateProjectWorkingFolderFile: mocks.recreate,
}));

function filePreview(relativePath: string, text: string, contentRevision = "rev-1"): ProjectWorkingFolderFilePreview {
  return {
    relativePath,
    displayName: relativePath,
    text,
    lineCount: 1,
    byteSize: text.length,
    binary: false,
    oversized: false,
    contentRevision,
  };
}

/** A confirmation the test answers later, so it can observe the session while the dialog is open. */
function pendingConfirmation(): (confirmed: boolean) => void {
  let answer: (confirmed: boolean) => void = () => undefined;
  mocks.confirm.mockImplementationOnce(() => new Promise<boolean>((resolve) => { answer = resolve; }));
  return (confirmed) => answer(confirmed);
}

function createSession(): { session: ChatFileEditorSession; selectedPaths: string[] } {
  const selectedPaths: string[] = [];
  const options: ChatFileEditorSessionOptions = {
    selectedPath: () => selectedPaths.at(-1) ?? null,
    setSelectedPath: (path) => { selectedPaths.push(path); },
    refreshParentDirectory: async () => undefined,
    onReviewCreated: () => undefined,
    discardUnsavedConfirmation: () => ({ message: "Discard?", confirmLabel: "Discard", danger: true }),
    overwriteConfirmation: () => ({ message: "Overwrite?", confirmLabel: "Overwrite", danger: true }),
    recreateConfirmation: () => ({ message: "Recreate?", confirmLabel: "Recreate" }),
    previewUnavailableMessage: () => "Preview unavailable",
  };
  return { session: new ChatFileEditorSession(options), selectedPaths };
}

async function openDirtyFile(session: ChatFileEditorSession, path = "src/a.ts"): Promise<void> {
  mocks.preview.mockResolvedValueOnce(filePreview(path, "saved"));
  await session.select(path);
  session.setDraftText("edited");
}

describe("chat file editor session confirmations", () => {
  beforeEach(() => {
    mocks.chat.selectedWorkingFolderId = "folder-1";
    mocks.chat.selectedExecutionEnvironmentId = null;
    mocks.confirm.mockReset();
    mocks.preview.mockReset();
    mocks.save.mockReset();
    mocks.recreate.mockReset();
  });

  it("keeps unsaved edits while the discard confirmation is open and when it is cancelled", async () => {
    const { session } = createSession();
    await openDirtyFile(session);
    const answer = pendingConfirmation();
    const opening = session.select("src/b.ts");
    await vi.waitFor(() => expect(mocks.confirm).toHaveBeenCalledOnce());
    expect(session.draftText).toBe("edited");
    expect(mocks.preview).toHaveBeenCalledTimes(1);

    answer(false);
    await expect(opening).resolves.toBe(false);
    expect(session.preview?.relativePath).toBe("src/a.ts");
    expect(session.draftText).toBe("edited");
  });

  it("opens the other file only after the discard is confirmed", async () => {
    const { session } = createSession();
    await openDirtyFile(session);
    mocks.confirm.mockResolvedValueOnce(true);
    mocks.preview.mockResolvedValueOnce(filePreview("src/b.ts", "other"));
    await expect(session.select("src/b.ts")).resolves.toBe(true);
    expect(session.preview?.relativePath).toBe("src/b.ts");
    expect(session.draftText).toBe("other");
  });

  it("ignores a confirmation answered after the working folder changed", async () => {
    const { session } = createSession();
    await openDirtyFile(session);
    const answer = pendingConfirmation();
    const opening = session.select("src/b.ts");
    await vi.waitFor(() => expect(mocks.confirm).toHaveBeenCalledOnce());
    mocks.chat.selectedWorkingFolderId = "folder-2";
    answer(true);
    await expect(opening).resolves.toBe(false);
    expect(mocks.preview).toHaveBeenCalledTimes(1);
  });

  it("asks only when the draft is dirty and reloads after the answer", async () => {
    const { session } = createSession();
    mocks.preview.mockResolvedValueOnce(filePreview("src/a.ts", "saved"));
    await session.select("src/a.ts");
    mocks.preview.mockResolvedValueOnce(filePreview("src/a.ts", "saved"));
    await session.reloadSelectedFile();
    expect(mocks.confirm).not.toHaveBeenCalled();

    session.setDraftText("edited");
    mocks.confirm.mockResolvedValueOnce(false);
    await session.reloadSelectedFile();
    expect(session.draftText).toBe("edited");
    expect(mocks.preview).toHaveBeenCalledTimes(2);

    mocks.confirm.mockResolvedValueOnce(true);
    mocks.preview.mockResolvedValueOnce(filePreview("src/a.ts", "from disk", "rev-2"));
    await session.reloadSelectedFile();
    expect(session.draftText).toBe("from disk");
  });

  it("overwrites the newer file only after the confirmation resolves true", async () => {
    const { session } = createSession();
    await openDirtyFile(session);
    mocks.confirm.mockResolvedValueOnce(false);
    await session.overwriteExternalFile();
    expect(mocks.save).not.toHaveBeenCalled();
    expect(session.saving).toBe(false);

    const answer = pendingConfirmation();
    const overwriting = session.overwriteExternalFile();
    await vi.waitFor(() => expect(mocks.confirm).toHaveBeenCalledTimes(2));
    expect(session.saving).toBe(false);
    mocks.preview.mockResolvedValueOnce(filePreview("src/a.ts", "newer", "rev-2"));
    mocks.save.mockResolvedValueOnce(filePreview("src/a.ts", "edited", "rev-3"));
    answer(true);
    await overwriting;
    expect(mocks.save).toHaveBeenCalledWith(expect.objectContaining({ contents: "edited", expectedRevision: "rev-2" }));
    expect(session.dirty).toBe(false);
  });

  it("recreates a deleted file only after the confirmation resolves true", async () => {
    const { session } = createSession();
    await openDirtyFile(session);
    mocks.preview.mockRejectedValueOnce({ code: "not_found" });
    await session.refreshFromDisk("src/a.ts", false);
    expect(session.fileDeleted).toBe(true);

    mocks.confirm.mockResolvedValueOnce(false);
    await session.save();
    expect(mocks.recreate).not.toHaveBeenCalled();

    mocks.confirm.mockResolvedValueOnce(true);
    mocks.recreate.mockResolvedValueOnce(filePreview("src/a.ts", "edited", "rev-2"));
    await session.save();
    expect(mocks.recreate).toHaveBeenCalledWith(expect.objectContaining({ contents: "edited", confirmed: true }));
    expect(session.fileDeleted).toBe(false);
  });
});
