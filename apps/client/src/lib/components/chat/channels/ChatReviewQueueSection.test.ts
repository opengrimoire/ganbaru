// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatReviewQueueItemRead, ChatReviewQueueRead } from "$lib/chat/contracts";
import ChatReviewQueueSection from "./ChatReviewQueueSection.svelte";

const timestamp = "2026-11-12T12:00:00.000Z";

function reviewItem(id: string, agentDisplayName: string): ChatReviewQueueItemRead {
  return {
    assignmentId: `assignment:${id}`,
    revision: 1,
    kind: "review",
    projectId: "project:alpha",
    projectName: "Alpha",
    channelId: `channel:${id}`,
    channelName: id,
    replyThreadId: `thread:${id}`,
    agentParticipantId: `participant:${id}`,
    agentDisplayName,
    stateReason: null,
    changedFileCount: 2,
    executedHere: true,
    updatedAt: timestamp,
  } as ChatReviewQueueItemRead;
}

const queue: ChatReviewQueueRead = {
  items: [reviewItem("a", "Atlas"), reviewItem("b", "Borealis")],
  total: 2,
  failureCount: 0,
  truncated: false,
};

const chat = vi.hoisted(() => ({
  reviewQueue: null as ChatReviewQueueRead | null,
  selectedChannel: null,
  openReviewItem: vi.fn<(item: ChatReviewQueueItemRead) => Promise<void>>(),
  assignmentReviewDetail: vi.fn<(assignmentId: string) => Promise<{ assignmentId: string }>>(),
  acceptReviewItems: vi.fn(),
}));
vi.mock("$lib/stores/chat.svelte", () => ({ useChat: () => chat }));
const paneEvents = vi.hoisted(() => ({ target: new EventTarget() }));
vi.mock("$lib/chat/pane-events", () => ({ useChatPaneEvents: () => paneEvents.target }));
vi.mock("$lib/platform", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/platform")>(),
  platformHasCapability: () => true,
}));

async function settle(): Promise<void> {
  for (let index = 0; index < 4; index += 1) {
    await Promise.resolve();
    await tick();
  }
}

describe("ChatReviewQueueSection", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.resetAllMocks();
    chat.reviewQueue = queue;
    chat.openReviewItem.mockResolvedValue();
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  it("opens only the changes of the item chosen last when an earlier one finishes later", async () => {
    let finishFirst: (detail: { assignmentId: string }) => void = () => undefined;
    chat.assignmentReviewDetail
      .mockReturnValueOnce(new Promise((resolve) => { finishFirst = resolve; }))
      .mockResolvedValueOnce({ assignmentId: "assignment:b" });
    const opened: unknown[] = [];
    const listener = (event: Event): void => { opened.push((event as CustomEvent<unknown>).detail); };
    paneEvents.target.addEventListener("ganbaru:chat-open-review", listener);
    const onOpened = vi.fn<() => void>();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReviewQueueSection, { target, props: { onOpened } });
    flushSync();

    const reviewButtons = target.querySelectorAll<HTMLButtonElement>(".explorer-icon");
    expect(reviewButtons).toHaveLength(2);
    reviewButtons[0]?.click();
    await settle();
    reviewButtons[1]?.click();
    await settle();
    expect(opened).toEqual([{ assignmentId: "assignment:b" }]);

    finishFirst({ assignmentId: "assignment:a" });
    await settle();
    expect(opened).toEqual([{ assignmentId: "assignment:b" }]);
    expect(onOpened).toHaveBeenCalledOnce();
    paneEvents.target.removeEventListener("ganbaru:chat-open-review", listener);
  });

  it("drops the failure of an earlier opening once the person opens another item", async () => {
    let failFirst: (cause: unknown) => void = () => undefined;
    chat.openReviewItem
      .mockReturnValueOnce(new Promise((_resolve, reject) => { failFirst = reject; }))
      .mockResolvedValueOnce();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatReviewQueueSection, { target, props: {} });
    flushSync();

    const rows = target.querySelectorAll<HTMLButtonElement>(".queue-open");
    rows[0]?.click();
    rows[1]?.click();
    await settle();
    failFirst({ code: "storage", message: "busy" });
    await settle();
    expect(target.querySelector('[role="alert"], .notice')).toBeNull();
  });
});
