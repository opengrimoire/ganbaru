import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatTimelineItemRead, ChatTimelinePageRead } from "$lib/chat/contracts";
import { ChatTimelineController } from "./timeline-controller.svelte";

const api = vi.hoisted(() => ({
  readPage: vi.fn<(threadId: string, cursor: string | null) => Promise<ChatTimelinePageRead>>(),
}));

vi.mock("$lib/api/chat", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/chat")>(),
  readChatTimelinePage: api.readPage,
}));

const PAGE_SIZE = 3;

/** A thread's projected rows, paged newest first like the native read. */
class FakeThread {
  items: ChatTimelineItemRead[] = [];

  constructor(readonly threadId: string, count: number) {
    for (let sequence = 1; sequence <= count; sequence += 1) this.append(sequence);
  }

  append(sequence: number): void {
    this.items.push(item(sequence));
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
    };
  }
}

describe("ChatTimelineController live refresh", () => {
  let selected: string | null;
  let thread: FakeThread;

  beforeEach(() => {
    api.readPage.mockReset();
    selected = "thread-1";
    thread = new FakeThread("thread-1", 10);
    api.readPage.mockImplementation((_threadId, cursor) => Promise.resolve(thread.page(cursor)));
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

  it("drops a newest-page row the fresh read no longer returns", async () => {
    const timeline = controller();
    await timeline.load("thread-1");
    await timeline.loadOlder();
    thread.items = thread.items.filter((entry) => entry.sequenceAnchor !== 9);

    await timeline.refreshLatest("thread-1");

    expect(sequences(timeline.items)).toEqual([5, 6, 7, 8, 10]);
  });

  it("replaces the pages when more rows arrived than one page holds", async () => {
    const timeline = controller();
    await timeline.load("thread-1");
    await timeline.loadOlder();
    for (let sequence = 11; sequence <= 15; sequence += 1) thread.append(sequence);

    await timeline.refreshLatest("thread-1");

    expect(timeline.pages).toHaveLength(1);
    expect(sequences(timeline.items)).toEqual([13, 14, 15]);
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
    const stale = deferred<ChatTimelinePageRead>();
    api.readPage.mockReturnValueOnce(stale.promise);

    const refresh = timeline.refreshLatest("thread-1");
    timeline.reset();
    thread.append(11);
    stale.resolve(thread.page(null));
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

function item(sequence: number): ChatTimelineItemRead {
  return {
    activityId: `item-${String(sequence).padStart(4, "0")}`,
    turnId: null,
    sequenceAnchor: sequence,
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
