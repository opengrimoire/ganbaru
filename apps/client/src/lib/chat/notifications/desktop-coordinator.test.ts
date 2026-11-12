import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatBehaviorPreferences,
  ChatQuietStateRead,
  ChatReviewQueueItemRead,
  ChatReviewQueueKind,
  ChatReviewQueueRead,
} from "$lib/chat/contracts";
import { defaultChatConfiguration } from "$lib/chat/validation";
import {
  chatNotificationKindEnabled,
  createChatNotificationCoordinator,
  type ChatNotificationCoordinator,
} from "./desktop-coordinator";
import { CHAT_NOTIFICATION_GRACE_MS, type ChatNotificationDelivery } from "./gate";
import { ChatQuietStatus } from "./quiet-status.svelte";

function item(id: string, kind: ChatReviewQueueKind): ChatReviewQueueItemRead {
  return {
    assignmentId: id,
    revision: 1,
    kind,
    projectId: "project:one",
    projectName: "One",
    channelId: "channel:one",
    channelName: "general",
    replyThreadId: `reply-thread:${id}`,
    teammateParticipantId: "participant:teammate",
    teammateDisplayName: "Builder",
    stateReason: null,
    changedFileCount: 1,
    executedHere: true,
    updatedAt: "2026-11-12T10:00:00.000Z",
  };
}

function queueOf(items: ChatReviewQueueItemRead[]): ChatReviewQueueRead {
  return { items, total: items.length, truncated: false };
}

/**
 * The main window as the coordinator sees it with no Chat pane mounted: the native queue and settings, and an
 * App effect that hands every completed queue read to the coordinator.
 */
interface MainWindow {
  coordinator: ChatNotificationCoordinator;
  delivered: ChatNotificationDelivery[];
  nativeQueue: ChatReviewQueueItemRead[];
  nativeBehavior: ChatBehaviorPreferences;
  behavior: ChatBehaviorPreferences | null;
  loadSettings: ReturnType<typeof vi.fn<() => Promise<void>>>;
  settingsReadFails: boolean;
}

function mainWindow(quietRead: () => Promise<ChatQuietStateRead>): MainWindow {
  const window: Omit<MainWindow, "coordinator" | "loadSettings"> = {
    delivered: [],
    nativeQueue: [],
    nativeBehavior: defaultChatConfiguration().behavior,
    behavior: null,
    settingsReadFails: false,
  };
  let coordinator: ChatNotificationCoordinator | null = null;
  const loadSettings = vi.fn(async () => {
    await Promise.resolve();
    if (window.settingsReadFails) throw new Error("settings unavailable");
    window.behavior = window.nativeBehavior;
  });
  coordinator = createChatNotificationCoordinator({
    isThreadVisible: () => false,
    behavior: () => window.behavior,
    loadReviewQueue: async () => {
      await Promise.resolve();
      coordinator?.observe(queueOf([...window.nativeQueue]));
    },
    loadSettings,
    deliver: (delivery) => window.delivered.push(delivery),
    quietStatus: new ChatQuietStatus({ read: quietRead }),
  });
  return Object.assign(window, { coordinator, loadSettings });
}

const notQuiet = (): Promise<ChatQuietStateRead> => Promise.resolve({ quiet: false, reason: null, endsAt: null });

function deliveredIds(deliveries: readonly ChatNotificationDelivery[]): string[] {
  return deliveries.flatMap((delivery) => (delivery.kind === "single" ? [delivery.signal] : delivery.signals))
    .map((signal) => signal.assignmentId);
}

describe("Chat notification coordinator", () => {
  let windows: MainWindow[] = [];

  function start(quietRead: () => Promise<ChatQuietStateRead> = notQuiet): MainWindow {
    const created = mainWindow(quietRead);
    windows.push(created);
    return created;
  }

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-12T10:00:00Z"));
    vi.stubGlobal("window", new EventTarget());
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    for (const window of windows) window.coordinator.dispose();
    windows = [];
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("delivers the first change after startup without a mounted Chat pane", async () => {
    const test = start();
    test.coordinator.loadInputs();
    await vi.advanceTimersByTimeAsync(0);

    test.nativeQueue = [item("a", "review")];
    test.coordinator.handleChange(["organizational"]);
    await vi.advanceTimersByTimeAsync(CHAT_NOTIFICATION_GRACE_MS);

    expect(deliveredIds(test.delivered)).toEqual(["a"]);
  });

  it("does not deliver a kind the person turned off before Chat opens", async () => {
    const test = start();
    test.nativeBehavior = { ...test.nativeBehavior, notifyFailures: false };
    test.coordinator.loadInputs();
    await vi.advanceTimersByTimeAsync(0);

    test.nativeQueue = [item("a", "failure"), item("b", "review")];
    test.coordinator.handleChange(["organizational"]);
    await vi.advanceTimersByTimeAsync(CHAT_NOTIFICATION_GRACE_MS);

    expect(deliveredIds(test.delivered)).toEqual(["b"]);
  });

  it("ignores changes that cannot move work into the review queue", async () => {
    const test = start();
    test.coordinator.loadInputs();
    await vi.advanceTimersByTimeAsync(0);

    test.nativeQueue = [item("a", "review")];
    test.coordinator.handleChange(["timeline"]);
    await vi.advanceTimersByTimeAsync(CHAT_NOTIFICATION_GRACE_MS);

    expect(test.delivered).toEqual([]);
  });

  it("retries a failed settings read on the next queue change and then delivers what waited", async () => {
    const test = start();
    test.settingsReadFails = true;
    test.coordinator.loadInputs();
    await vi.advanceTimersByTimeAsync(0);
    expect(test.behavior).toBeNull();

    test.settingsReadFails = false;
    test.nativeQueue = [item("a", "answer")];
    test.coordinator.handleChange(["organizational"]);
    await vi.advanceTimersByTimeAsync(CHAT_NOTIFICATION_GRACE_MS);

    expect(test.loadSettings).toHaveBeenCalledTimes(2);
    expect(deliveredIds(test.delivered)).toEqual(["a"]);
  });

  it("holds delivery while the quiet state cannot be read", async () => {
    const test = start(() => Promise.reject(new Error("quiet state unavailable")));
    test.coordinator.loadInputs();
    await vi.advanceTimersByTimeAsync(0);

    test.nativeQueue = [item("a", "review")];
    test.coordinator.handleChange(["organizational"]);
    await vi.advanceTimersByTimeAsync(CHAT_NOTIFICATION_GRACE_MS * 2);

    expect(test.delivered).toEqual([]);
  });

  it("takes a fresh silent baseline for the next vault after a reset", async () => {
    const test = start();
    test.coordinator.loadInputs();
    await vi.advanceTimersByTimeAsync(0);

    test.coordinator.reset();
    test.nativeQueue = [item("old", "review")];
    test.coordinator.loadInputs();
    await vi.advanceTimersByTimeAsync(0);
    test.nativeQueue = [item("old", "review"), item("new", "review")];
    test.coordinator.handleChange(["unknown"]);
    await vi.advanceTimersByTimeAsync(CHAT_NOTIFICATION_GRACE_MS);

    expect(deliveredIds(test.delivered)).toEqual(["new"]);
  });
});

describe("chatNotificationKindEnabled", () => {
  it("maps each kind to its switch and reports unknown settings as null", () => {
    const behavior: ChatBehaviorPreferences = {
      ...defaultChatConfiguration().behavior,
      notifyWaitingForYou: false,
      notifyReviewReady: true,
      notifyFailures: false,
    };

    expect(chatNotificationKindEnabled(behavior, "review")).toBe(true);
    expect(chatNotificationKindEnabled(behavior, "failure")).toBe(false);
    expect(chatNotificationKindEnabled(behavior, "answer")).toBe(false);
    expect(chatNotificationKindEnabled(behavior, "approval")).toBe(false);
    expect(chatNotificationKindEnabled(null, "review")).toBeNull();
  });
});
