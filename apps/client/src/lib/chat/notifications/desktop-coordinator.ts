import type { ChatReplyThreadId, ChatReviewQueueRead } from "$lib/chat/contracts";
import {
  attentionSignals,
  createChatNotificationGate,
  type ChatAttentionKind,
  type ChatNotificationDelivery,
} from "./gate";
import { getChatQuietStatus } from "./quiet-status.svelte";

/** What the main window supplies to the Chat notification coordinator. */
export interface ChatNotificationCoordinatorOptions {
  isThreadVisible(replyThreadId: ChatReplyThreadId): boolean;
  isKindEnabled(kind: ChatAttentionKind): boolean;
  deliver(delivery: ChatNotificationDelivery): void;
}

/** Feeds review queue reads to the notification gate and keeps the gate's quiet state current. */
export interface ChatNotificationCoordinator {
  /** Takes the latest review queue read of the current vault. */
  observe(queue: ChatReviewQueueRead): void;
  /** Reads the quiet state again, for example after a Focus phase change. */
  refreshQuiet(): void;
  /** Makes the next queue read a silent baseline, as after a vault switch. */
  reset(): void;
  dispose(): void;
}

/**
 * Creates the coordinator that owns the main window's Chat notification gate. Every observed queue read also
 * rereads the quiet state, so a key released after its grace period sees a recent value, and each completed
 * read lets the gate release overdue keys and deliver what was deferred.
 *
 * @param options Visibility, kind preferences, and delivery.
 */
export function createChatNotificationCoordinator(
  options: ChatNotificationCoordinatorOptions,
): ChatNotificationCoordinator {
  const quietStatus = getChatQuietStatus();
  const gate = createChatNotificationGate<ReturnType<typeof setTimeout>>({
    now: () => Date.now(),
    setTimer: (callback, timeoutMs) => setTimeout(callback, timeoutMs),
    clearTimer: (id) => clearTimeout(id),
    quietState: () => quietStatus.state,
    isThreadVisible: options.isThreadVisible,
    isKindEnabled: options.isKindEnabled,
    deliver: options.deliver,
  });
  const releaseQuiet = quietStatus.use(() => gate.reevaluate());

  return {
    observe(queue) {
      gate.observe(attentionSignals(queue));
      quietStatus.refresh();
    },
    refreshQuiet() {
      quietStatus.refresh();
    },
    reset() {
      gate.reset();
    },
    dispose() {
      releaseQuiet();
      gate.dispose();
    },
  };
}
