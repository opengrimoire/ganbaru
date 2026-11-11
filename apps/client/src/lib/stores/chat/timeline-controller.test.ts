import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatTimelineDeltaRead,
  ChatTimelineItemRead,
  ChatTimelinePageRead,
  ChatTimelineTurnRead,
} from "$lib/chat/contracts";
import { ChatTimelineController, mergeTimelineDelta } from "./timeline-controller.svelte";

const api = vi.hoisted(() => ({
  readPage: vi.fn<(threadId: string, cursor: string | null) => Promise<ChatTimelinePageRead>>(),
  readSince: vi.fn<(threadId: string, afterChange: number) => Promise<ChatTimelineDeltaRead>>(),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  readChatTimelinePage: api.readPage,
  readChatTimelineSince: api.readSince,
}));

const PAGE_SIZE = 3;

/** A thread's projected rows and change clock, paged newest first like the native read. */
class FakeThread {
  items: ChatTimelineItemRead[] = [];
  removals: { itemId: string; change: number }[] = [];
  clock = 0;
  prunedThrough = 0;

  constructor(readonly threadId: string, count: number) {
    for (let sequence = 1; sequence <= count; sequence += 1) this.append(sequence);
  }

  append(sequence: number): void {
    this.clock += 1;
    this.items.push(item(sequence, this.clock));
  }

  edit(sequence: number, markdown: string): void {
    this.clock += 1;
    const change = this.clock;
    this.items = this.items.map((entry) => entry.sequenceAnchor === sequence
      ? { ...entry, changeSequence: change, data: { schemaVersion: 1, value: { markdown } } }
      : entry);
  }

  remove(sequence: number): void {
    this.clock += 1;
    const removed = this.items.find((entry) => entry.sequenceAnchor === sequence);
    if (removed) this.removals.push({ itemId: removed.activityId, change: this.clock });
    this.items = this.items.filter((entry) => entry.sequenceAnchor !== sequence);
  }

  page(cursor: string | null): ChatTimelinePageRead {
    const end = cursor === null ? this.items.length : Number(cursor);
    const start = Math.max(0, end - PAGE_SIZE);
    return {
      threadId: this.threadId,
      items: this.items.slice(start, end),
      turns: [],
      previousCursor: start > 0 ? String(start) : null,
      nextCursor: null,
      threadRevision: this.items.length,
      changeCursor: this.clock,
    };
  }

  since(afterChange: number): ChatTimelineDeltaRead {
    const changed = this.items
      .filter((entry) => entry.changeSequence > afterChange)
      .sort((left, right) => left.changeSequence - right.changeSequence);
    const items = changed.slice(0, PAGE_SIZE);
    const hasMore = changed.length > PAGE_SIZE;
    const changeCursor = hasMore ? items.at(-1)?.changeSequence ?? afterChange : this.clock;
    return {
      threadId: this.threadId,
      items,
      turns: [],
      removedItemIds: this.removals
        .filter((removal) => removal.change > afterChange && removal.change <= changeCursor)
        .map((removal) => removal.itemId),
      changeCursor,
      hasMore,
      resetRequired: afterChange < this.prunedThrough,
      threadRevision: this.items.length,
    };
  }
}

describe("ChatTimelineController live refresh", () => {
  let selected: string | null;
  let thread: FakeThread;

  beforeEach(() => {
    api.readPage.mockReset();
    api.readSince.mockReset();
    selected = "thread-1";
    thread = new FakeThread("thread-1", 10);
    api.readPage.mockImplementation((_threadId, cursor) => Promise.resolve(thread.page(cursor)));
    api.readSince.mockImplementation((_threadId, afterChange) => Promise.resolve(thread.since(afterChange)));
  });

  function controller(): ChatTimelineController {
    return new ChatTimelineController({ selectedThreadId: () => selected });
  }

  it("keeps older pages and the row that slid out of the newest page", async () => {
    const timeline = controller();
    await timeline.load("thread-1");
    await timeline.loadOlder();
    await timeline.loadOlder();
    expect(timeline.pages).toHaveLength(3);
    expect(sequences(timeline.items)).toEqual([2, 3, 4, 5, 6, 7, 8, 9, 10]);

    thread.append(11);
    await timeline.refreshLatest("thread-1");

    expect(timeline.pages).toHaveLength(3);
    expect(sequences(timeline.items)).toEqual([2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
    expect(timeline.pages[0]?.previousCursor).toBe("1");
  });

  it("drops a row the since-read reports as removed", async () => {
    const timeline = controller();
    await timeline.load("thread-1");
    await timeline.loadOlder();
    thread.remove(9);

    await timeline.refreshLatest("thread-1");

    expect(sequences(timeline.items)).toEqual([5, 6, 7, 8, 10]);
    expect(api.readPage).toHaveBeenCalledTimes(2);
  });

  it("replaces a changed row in an older page without rereading pages", async () => {
    const timeline = controller();
    await timeline.load("thread-1");
    await timeline.loadOlder();
    thread.edit(5, "edited");

    await timeline.refreshLatest("thread-1");

    expect(sequences(timeline.items)).toEqual([5, 6, 7, 8, 9, 10]);
    expect(timeline.items[0]?.data.value).toEqual({ markdown: "edited" });
    expect(api.readPage).toHaveBeenCalledTimes(2);
  });

  it("pages the since-read until no change is pending, without rereading pages", async () => {
    const timeline = controller();
    await timeline.load("thread-1");
    await timeline.loadOlder();
    for (let sequence = 11; sequence <= 15; sequence += 1) thread.append(sequence);

    await timeline.refreshLatest("thread-1");

    expect(api.readPage).toHaveBeenCalledTimes(2);
    expect(api.readSince).toHaveBeenCalledTimes(2);
    expect(sequences(timeline.items)).toEqual([5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
    thread.append(16);
    await timeline.refreshLatest("thread-1");
    expect(api.readSince).toHaveBeenLastCalledWith("thread-1", 15);
    expect(sequences(timeline.items).at(-1)).toBe(16);
  });

  it("keeps an older-page change that lands after a since-read reports more pending", async () => {
    const timeline = controller();
    await timeline.load("thread-1");
    await timeline.loadOlder();
    for (let sequence = 11; sequence <= 15; sequence += 1) thread.append(sequence);
    thread.edit(5, "edited");

    await timeline.refreshLatest("thread-1");

    expect(sequences(timeline.items)).toEqual([5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
    expect(timeline.items[0]?.data.value).toEqual({ markdown: "edited" });
    thread.append(16);
    await timeline.refreshLatest("thread-1");
    expect(api.readSince).toHaveBeenLastCalledWith("thread-1", thread.clock - 1);
  });

  it("falls back to a full load when changes stay pending after the pass bound", async () => {
    const timeline = controller();
    await timeline.load("thread-1");
    await timeline.loadOlder();
    for (let sequence = 11; sequence <= 40; sequence += 1) thread.append(sequence);

    await timeline.refreshLatest("thread-1");

    expect(api.readSince).toHaveBeenCalledTimes(8);
    expect(timeline.pages).toHaveLength(1);
    expect(sequences(timeline.items)).toEqual([38, 39, 40]);
    thread.append(41);
    await timeline.refreshLatest("thread-1");
    expect(api.readSince).toHaveBeenLastCalledWith("thread-1", 40);
  });

  it("reloads the newest page when the removals after the cursor were pruned", async () => {
    const timeline = controller();
    await timeline.load("thread-1");
    await timeline.loadOlder();
    thread.append(11);
    thread.prunedThrough = thread.clock;

    await timeline.refreshLatest("thread-1");

    expect(timeline.pages).toHaveLength(1);
    expect(sequences(timeline.items)).toEqual([9, 10, 11]);
  });

  it("ignores a refresh for a thread that is not selected", async () => {
    const timeline = controller();
    await timeline.load("thread-1");
    await timeline.loadOlder();

    await timeline.refreshLatest("thread-2");

    expect(api.readPage).toHaveBeenCalledTimes(2);
    expect(timeline.pages).toHaveLength(2);
  });

  it("discards a refresh that resolves after the thread was reset", async () => {
    const timeline = controller();
    await timeline.load("thread-1");
    const stale = deferred<ChatTimelineDeltaRead>();
    api.readSince.mockReturnValueOnce(stale.promise);

    const refresh = timeline.refreshLatest("thread-1");
    timeline.reset();
    thread.append(11);
    stale.resolve(thread.since(10));
    await refresh;

    expect(timeline.pages).toEqual([]);
    expect(timeline.items).toEqual([]);
  });

  it("does not block or discard an older-page load that is in flight", async () => {
    const timeline = controller();
    await timeline.load("thread-1");
    const older = deferred<ChatTimelinePageRead>();
    api.readPage.mockReturnValueOnce(older.promise);

    const loadingOlder = timeline.loadOlder();
    thread.append(11);
    await timeline.refreshLatest("thread-1");
    older.resolve(thread.page("7"));
    await loadingOlder;

    expect(timeline.loading).toBe(false);
    expect(sequences(timeline.items)).toEqual([5, 6, 7, 8, 9, 10, 11]);
  });

  it("loads the newest page when nothing is loaded yet", async () => {
    const timeline = controller();

    await timeline.refreshLatest("thread-1");

    expect(sequences(timeline.items)).toEqual([8, 9, 10]);
  });

  it("keeps the page that holds the selected sequence when pages are evicted", async () => {
    thread = new FakeThread("thread-1", 30);
    const timeline = controller();
    await timeline.load("thread-1");
    for (let index = 0; index < 7; index += 1) await timeline.loadOlder();
    expect(timeline.pages).toHaveLength(8);
    await timeline.loadOlder(8);
    expect(timeline.pages).toHaveLength(8);
    expect(sequences(timeline.items)).not.toContain(30);
    const before = sequences(timeline.items);

    thread.append(31);
    await timeline.refreshLatest("thread-1", 8);

    expect(timeline.pages).toHaveLength(8);
    expect(sequences(timeline.items)).toEqual(before);
    expect(sequences(timeline.items)).toContain(8);
  });
});

describe("mergeTimelineDelta", () => {
  it("drops a changed row older than the loaded history and inserts one inside it in order", () => {
    const pages = [page([4, 5, 6], "3"), page([7, 8, 9], "6")];

    const merged = mergeTimelineDelta(pages, delta([item(2, 20), item(5, 21)]), true);

    expect(merged.map((entry) => sequences(entry.items))).toEqual([[4, 5, 6], [7, 8, 9]]);
    expect(merged[0]?.items[1]?.changeSequence).toBe(21);
  });

  it("keeps a changed row older than the oldest row when the whole history is loaded", () => {
    const merged = mergeTimelineDelta([page([4, 5], null)], delta([item(2, 20)]), true);

    expect(sequences(merged[0]?.items ?? [])).toEqual([2, 4, 5]);
  });

  it("drops a row newer than the loaded pages unless they hold the newest row", () => {
    const pages = [page([4, 5, 6], null)];

    expect(sequences(mergeTimelineDelta(pages, delta([item(7, 20)]), false)[0]?.items ?? [])).toEqual([4, 5, 6]);
    expect(sequences(mergeTimelineDelta(pages, delta([item(7, 20)]), true)[0]?.items ?? [])).toEqual([4, 5, 6, 7]);
  });

  it("drops a page a removal empties and keeps the outer paging cursor", () => {
    const pages = [page([4], "3"), page([7, 8], "6")];

    const merged = mergeTimelineDelta(pages, delta([], [item(4, 1).activityId]), true);

    expect(merged).toHaveLength(1);
    expect(merged[0]?.previousCursor).toBe("3");
    expect(sequences(merged[0]?.items ?? [])).toEqual([7, 8]);
  });

  it("updates a loaded turn and attaches the turn of a new row once", () => {
    const pages = [{ ...page([4, 5], null), turns: [turn("turn-1", "active")] }];
    const fresh = { ...item(6, 20), turnId: "turn-2" };

    const merged = mergeTimelineDelta(
      pages,
      { ...delta([fresh]), turns: [turn("turn-1", "completed"), turn("turn-2", "active")] },
      true,
    );

    expect(merged[0]?.turns.map((entry) => [entry.turnId, entry.state])).toEqual([
      ["turn-1", "completed"],
      ["turn-2", "active"],
    ]);
  });
});

function page(rows: number[], previousCursor: string | null): ChatTimelinePageRead {
  return {
    threadId: "thread-1",
    items: rows.map((sequence) => item(sequence, sequence)),
    turns: [],
    previousCursor,
    nextCursor: null,
    threadRevision: 1,
    changeCursor: 10,
  };
}

function delta(items: ChatTimelineItemRead[], removedItemIds: string[] = []): ChatTimelineDeltaRead {
  return {
    threadId: "thread-1",
    items,
    turns: [],
    removedItemIds,
    changeCursor: 30,
    hasMore: false,
    resetRequired: false,
    threadRevision: 2,
  };
}

function turn(turnId: string, state: ChatTimelineTurnRead["state"]): ChatTimelineTurnRead {
  return {
    turnId,
    state,
    startedAt: null,
    completedAt: null,
    stopReason: null,
    modelId: null,
    modelOptions: [],
    modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
    usage: null,
    changedFiles: [],
  };
}

function item(sequence: number, change: number): ChatTimelineItemRead {
  return {
    activityId: `item-${String(sequence).padStart(4, "0")}`,
    turnId: null,
    sequenceAnchor: sequence,
    changeSequence: change,
    kind: "message",
    data: { schemaVersion: 1, value: { markdown: `row ${sequence}` } },
  };
}

function sequences(items: readonly ChatTimelineItemRead[]): number[] {
  return items.map((entry) => entry.sequenceAnchor);
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
