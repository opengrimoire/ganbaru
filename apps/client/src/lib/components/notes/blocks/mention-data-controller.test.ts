import { afterEach, describe, expect, it, vi } from "vitest";
import { createNotesMentionDataController } from "./mention-data-controller.svelte";
import type { NotesDatabaseMentionData } from "./mention-targets";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

function row(id: string): NotesDatabaseMentionData["rowPages"][number] {
  return { id } as NotesDatabaseMentionData["rowPages"][number];
}

describe("Notes mention data controller", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shares database data only while mention menus need it", async () => {
    vi.useFakeTimers();
    const result = deferred<NotesDatabaseMentionData>();
    const load = vi.fn(() => result.promise);
    const controller = createNotesMentionDataController(load, 1_000);
    expect(load).not.toHaveBeenCalled();
    const closeFirst = controller.acquire();
    const closeSecond = controller.acquire();
    expect(load).toHaveBeenCalledOnce();
    result.resolve({ dataSources: [], rowPages: [row("row")] });
    await result.promise;
    expect(controller.rowPages).toHaveLength(1);
    closeFirst();
    closeFirst();
    closeSecond();
    vi.advanceTimersByTime(999);
    expect(controller.rowPages).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(controller.rowPages).toEqual([]);
    const closeThird = controller.acquire();
    expect(load).toHaveBeenCalledTimes(2);
    closeThird();
    vi.runAllTimers();
    await result.promise;
    expect(controller.rowPages).toEqual([]);
  });

  it("keeps showing loaded data while a briefly closed menu reopens and refreshes", async () => {
    vi.useFakeTimers();
    const first = deferred<NotesDatabaseMentionData>();
    const second = deferred<NotesDatabaseMentionData>();
    const load = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const controller = createNotesMentionDataController(load, 1_000);
    controller.acquire()();
    controller.acquire()();
    expect(load).toHaveBeenCalledOnce();
    const close = controller.acquire();
    first.resolve({ dataSources: [], rowPages: [row("first")] });
    await first.promise;
    expect(controller.rowPages.map((page) => page.id)).toEqual(["first"]);

    close();
    const reopen = controller.acquire();
    expect(load).toHaveBeenCalledTimes(2);
    expect(controller.rowPages.map((page) => page.id)).toEqual(["first"]);
    second.resolve({ dataSources: [], rowPages: [row("second")] });
    await second.promise;
    expect(controller.rowPages.map((page) => page.id)).toEqual(["second"]);
    vi.runAllTimers();
    expect(controller.rowPages.map((page) => page.id)).toEqual(["second"]);
    reopen();
  });

  it("rejects data from a previous vault while a menu is still open", async () => {
    const first = deferred<NotesDatabaseMentionData>();
    const second = deferred<NotesDatabaseMentionData>();
    const load = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const controller = createNotesMentionDataController(load);
    const close = controller.acquire();
    controller.switchVault();
    second.resolve({ dataSources: [], rowPages: [] });
    await second.promise;
    first.resolve({ dataSources: [], rowPages: [row("old-vault")] });
    await first.promise;
    expect(controller.rowPages).toEqual([]);
    close();
  });

  it("ignores an older aggregate result after a newer reload finishes", async () => {
    const first = deferred<NotesDatabaseMentionData>();
    const second = deferred<NotesDatabaseMentionData>();
    const load = vi.fn()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const controller = createNotesMentionDataController(load);

    const firstReload = controller.reload();
    const secondReload = controller.reload();
    second.resolve({ dataSources: [], rowPages: [] });
    await secondReload;
    first.resolve({
      dataSources: [],
      rowPages: [row("stale")],
    });
    await firstReload;

    expect(controller.rowPages).toEqual([]);
  });

  it("clears current data when the latest aggregate load fails", async () => {
    const successful: NotesDatabaseMentionData = { dataSources: [], rowPages: [] };
    const load = vi.fn()
      .mockResolvedValueOnce(successful)
      .mockRejectedValueOnce(new Error("failed"));
    const controller = createNotesMentionDataController(load);
    await controller.reload();
    await controller.reload();
    expect(controller.dataSources).toEqual([]);
    expect(controller.rowPages).toEqual([]);
  });
});
