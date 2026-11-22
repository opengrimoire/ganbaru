// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  listNotesProjectHistoryVersions,
  loadNotesProjectHistoryTree,
  previewNotesProjectHistoryRestore,
  restoreNotesProjectHistoryVersion,
} from "$lib/api/notes/project-history";
import type { NotesProjectHistoryVersion } from "$lib/notes/types";
import NotesProjectVersionHistoryModal from "./NotesProjectVersionHistoryModal.svelte";

vi.mock("$lib/api/notes/project-history", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/notes/project-history")>(),
  listNotesProjectHistoryVersions: vi.fn(),
  loadNotesProjectHistoryPage: vi.fn(),
  loadNotesProjectHistoryTree: vi.fn(),
  previewNotesProjectHistoryRestore: vi.fn(),
  restoreNotesProjectHistoryVersion: vi.fn(),
}));

const version: NotesProjectHistoryVersion = {
  id: "version-1",
  projectId: "project-1",
  manifestHash: "hash",
  reason: "edit",
  createdBy: "user-1",
  displayName: { type: "user", resolved_name: "You" },
  changedNoteSummary: "",
  pageCount: 1,
  activePageCount: 1,
  archivedPageCount: 0,
  deletedPageCount: 0,
  createdTime: "2026-11-20T10:00:00.000Z",
};

describe("NotesProjectVersionHistoryModal", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
    vi.resetAllMocks();
  });

  function enabledButtonsLabeled(label: string): HTMLButtonElement[] {
    return [...document.querySelectorAll<HTMLButtonElement>("button")]
      .filter((entry) => entry.textContent?.trim() === label && !entry.disabled);
  }

  /** The confirmation dialog names its action with the Enter key hint. */
  function confirmButton(): HTMLButtonElement | null {
    return [...document.querySelectorAll<HTMLButtonElement>("button")]
      .find((entry) => entry.textContent?.trim().startsWith("Restore (")) ?? null;
  }

  it("leaves a reopened history open when a restore finishes after the earlier one closed", async () => {
    vi.mocked(listNotesProjectHistoryVersions).mockResolvedValue({ versions: [version], nextCursorTime: null, nextCursorId: null });
    vi.mocked(loadNotesProjectHistoryTree).mockResolvedValue({ pages: [] } as unknown as Awaited<ReturnType<typeof loadNotesProjectHistoryTree>>);
    vi.mocked(previewNotesProjectHistoryRestore).mockResolvedValue({
      versionId: "version-1",
      removeCount: 0,
      recreateCount: 0,
      changeCount: 1,
      copyCount: 0,
      safetyVersionWillBeCreated: true,
    });
    let finishRestore: (safety: NotesProjectHistoryVersion) => void = () => undefined;
    vi.mocked(restoreNotesProjectHistoryVersion).mockImplementation(() => new Promise<NotesProjectHistoryVersion>((resolve) => { finishRestore = resolve; }));
    const onClose = vi.fn();
    const onRestored = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(NotesProjectVersionHistoryModal, { target, props: { projectId: "project-1", onClose, onRestored } });

    await vi.waitFor(() => expect(enabledButtonsLabeled("Restore")).toHaveLength(1));
    enabledButtonsLabeled("Restore")[0]?.click();
    await vi.waitFor(() => expect(confirmButton()).not.toBeNull());
    confirmButton()?.click();
    await tick();
    expect(restoreNotesProjectHistoryVersion).toHaveBeenCalledOnce();

    await unmount(component);
    component = null;
    finishRestore({ ...version, id: "version-safety" });
    await tick();
    await tick();

    expect(onRestored).toHaveBeenCalledOnce();
    expect(onClose).not.toHaveBeenCalled();
  });
});
