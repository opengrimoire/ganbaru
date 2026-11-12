import type { ChatBehaviorPreferences, ChatReplyThreadId, ChatReviewQueueRead } from "$lib/chat/contracts";
import {
  attentionSignals,
  createChatNotificationGate,
  type ChatAttentionKind,
  type ChatNotificationDelivery,
} from "./gate";
import { getChatQuietStatus, type ChatQuietStatus } from "./quiet-status.svelte";

/** What the main window supplies to the Chat notification coordinator. */
export interface ChatNotificationCoordinatorOptions {
  isThreadVisible(replyThreadId: ChatReplyThreadId): boolean;
  /** The Chat behavior preferences of the current vault, or `null` while the settings are not read. */
  behavior(): ChatBehaviorPreferences | null;
  /** Reads the review queue of the current vault; the result arrives through {@link ChatNotificationCoordinator.observe}. */
  loadReviewQueue(): Promise<void>;
  /** Reads the Chat settings of the current vault, so {@link behavior} becomes known. */
  loadSettings(): Promise<void>;
  deliver(delivery: ChatNotificationDelivery): void;
  /** The quiet status to read; the window's shared one by default. */
  quietStatus?: ChatQuietStatus;
}

/** Feeds review queue reads to the notification gate and keeps the gate's inputs current. */
export interface ChatNotificationCoordinator {
  /**
   * Reads the review queue and, when unknown, the Chat settings of the current vault without waiting for a Chat
   * pane, so the silent baseline is the state before any change arrives. Call once a vault is active.
   */
  loadInputs(): void;
  /** Takes the latest review queue read of the current vault. */
  observe(queue: ChatReviewQueueRead): void;
  /** Rereads the review queue after a native change that may have moved work into or out of it. */
  handleChange(changedProjectionKeys: readonly string[]): void;
  /** Delivers what waited for the settings or quiet state, for example after the settings load elsewhere. */
  reevaluate(): void;
  /** Reads the quiet state again, for example after a Focus phase change. */
  refreshQuiet(): void;
  /** Makes the next queue read a silent baseline, as after a vault switch. */
  reset(): void;
  dispose(): void;
}

/** Whether a kind's notifications are on, or `null` while the Chat settings are unknown. */
export function chatNotificationKindEnabled(
  behavior: ChatBehaviorPreferences | null,
  kind: ChatAttentionKind,
): boolean | null {
  if (!behavior) return null;
  if (kind === "review") return behavior.notifyReviewReady;
  if (kind === "failure") return behavior.notifyFailures;
  return behavior.notifyWaitingForYou;
}

/**
 * Creates the coordinator that owns the main window's Chat notification gate. Every observed queue read also
 * rereads the quiet state, so a key released after its grace period sees a recent value, and each completed
 * read lets the gate release overdue keys and deliver what was deferred. While the settings are unknown, every
 * queue change retries reading them, so a failed read does not hold notifications until Chat opens.
 *
 * @param options Visibility, preferences, input reads, and delivery.
 */
export function createChatNotificationCoordinator(
  options: ChatNotificationCoordinatorOptions,
): ChatNotificationCoordinator {
  const quietStatus = options.quietStatus ?? getChatQuietStatus();
  let disposed = false;
  let generation = 0;
  let settingsRequest: Promise<void> | null = null;
  const gate = createChatNotificationGate<ReturnType<typeof setTimeout>>({
    now: () => Date.now(),
    setTimer: (callback, timeoutMs) => setTimeout(callback, timeoutMs),
    clearTimer: (id) => clearTimeout(id),
    quietState: () => quietStatus.state,
    isThreadVisible: options.isThreadVisible,
    isKindEnabled: (kind) => chatNotificationKindEnabled(options.behavior(), kind),
    deliver: options.deliver,
  });
  const releaseQuiet = quietStatus.use(() => gate.reevaluate());

  function loadSettingsWhenUnknown(): void {
    if (disposed || settingsRequest || options.behavior()) return;
    const requestGeneration = generation;
    const request = options.loadSettings()
      .then(() => {
        if (!disposed && requestGeneration === generation) gate.reevaluate();
      })
      .catch((error: unknown) => {
        console.error("[chat notifications] settings read failed:", error);
      })
      .finally(() => {
        if (settingsRequest === request) settingsRequest = null;
      });
    settingsRequest = request;
  }

  return {
    loadInputs() {
      if (disposed) return;
      void options.loadReviewQueue().catch((error: unknown) => {
        console.error("[chat notifications] review queue read failed:", error);
      });
      loadSettingsWhenUnknown();
    },
    observe(queue) {
      if (disposed) return;
      gate.observe(attentionSignals(queue));
      quietStatus.refresh();
      loadSettingsWhenUnknown();
    },
    handleChange(changedProjectionKeys) {
      if (disposed) return;
      if (!changedProjectionKeys.includes("organizational") && !changedProjectionKeys.includes("unknown")) return;
      void options.loadReviewQueue().catch((error: unknown) => {
        console.error("[chat notifications] review queue read failed:", error);
      });
    },
    reevaluate() {
      if (!disposed) gate.reevaluate();
    },
    refreshQuiet() {
      quietStatus.refresh();
    },
    reset() {
      generation += 1;
      settingsRequest = null;
      gate.reset();
    },
    dispose() {
      disposed = true;
      releaseQuiet();
      gate.dispose();
    },
  };
}
