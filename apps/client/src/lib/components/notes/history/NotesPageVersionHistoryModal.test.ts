// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesPageHistorySnapshot } from "$lib/notes/types";
import NotesPageVersionHistoryModal from "./NotesPageVersionHistoryModal.svelte";

const editor = vi.hoisted(() => ({
  store: {
    pageHistorySnapshots: [] as NotesPageHistorySnapshot[],
    pageHistorySnapshotsLoading: false,
    pageHistorySnapshotsError: null as string | null,
    pageHistoryVersion: null,
    pageHistoryVersionLoading: false,
    pageHistoryVersionError: null as string | null,
    pageHistoryActionError: null as string | null,
    localUser: null,
    localUserLoading: false,
    ensureOptionalSubsystem: () => Promise.resolve(),
    reloadPageHistory: (_pageId: string) => Promise.resolve(),
    loadPageHistoryVersion: (_snapshotId: string) => Promise.resolve(),
    copyPageHistoryBlocks: (_snapshotId: string) => Promise.resolve(),
    restorePageHistoryVersion: (_snapshotId: string) => Promise.resolve(),
  },
}));

vi.mock("$lib/components/notes/editor-context", () => ({
  getNotesEditor: () => editor.store,
}));

function snapshot(): NotesPageHistorySnapshot {
  return {
    object: "page_history_snapshot",
    id: "snapshot-1",
    page_id: "page-1",
    title: "Plan",
    icon: null,
    cover: null,
    block_count: 2,
    reason: "edit",
    created_by: { object: "user", id: "user-1" },
    created_time: "2026-11-20T10:00:00.000Z",
    page_last_edited_time: "2026-11-20T10:00:00.000Z",
  };
}

describe("NotesPageVersionHistoryModal", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
    vi.restoreAllMocks();
  });

  function buttonLabeled(label: string): HTMLButtonElement {
    const button = [...document.querySelectorAll<HTMLButtonElement>("button")]
      .find((entry) => entry.textContent?.trim() === label);
    if (!button) throw new Error(`No button labeled ${label}`);
    return button;
  }

  it("refuses a restore while copied blocks are still being added, so the modal never closes without restoring", async () => {
    editor.store.pageHistorySnapshots = [snapshot()];
    let finishCopy = () => {};
    const copy = vi.spyOn(editor.store, "copyPageHistoryBlocks")
      .mockReturnValue(new Promise<void>((resolve) => { finishCopy = resolve; }));
    const restore = vi.spyOn(editor.store, "restorePageHistoryVersion");
    const onClose = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(NotesPageVersionHistoryModal, { target, props: { pageId: "page-1", onClose } });
    await vi.waitFor(() => expect(buttonLabeled("Restore").disabled).toBe(false));

    buttonLabeled("Copy blocks").click();
    flushSync();
    expect(copy).toHaveBeenCalledOnce();

    expect(buttonLabeled("Restore").disabled).toBe(true);
    buttonLabeled("Restore").click();
    flushSync();
    expect(restore).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();

    finishCopy();
    await vi.waitFor(() => expect(buttonLabeled("Restore").disabled).toBe(false));
    await tick();
  });
});
