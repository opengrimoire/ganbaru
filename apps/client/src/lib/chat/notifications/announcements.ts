import type { ChatReplyThreadId } from "$lib/chat/contracts";
import { attentionKey, type ChatAttentionSignal } from "./gate";

/**
 * Picks the attention change a polite live region speaks: the newest signal that is new since the previous
 * snapshot and belongs to a reply thread other than the selected one. The review queue carries only work that
 * waits for the person, so working and queued work never reach this selector.
 *
 * @param previous Signals already announced or present at the baseline.
 * @param next Signals of the latest snapshot.
 * @param selectedReplyThreadId The reply thread the person has open, whose changes its own live region speaks.
 * @returns The signal to announce, or `null` when nothing qualifies.
 */
export function nextBackgroundAnnouncement(
  previous: readonly ChatAttentionSignal[],
  next: readonly ChatAttentionSignal[],
  selectedReplyThreadId: ChatReplyThreadId | null,
): ChatAttentionSignal | null {
  const known = new Set(previous.map(attentionKey));
  let newest: ChatAttentionSignal | null = null;
  let newestMs = Number.NEGATIVE_INFINITY;
  for (const signal of next) {
    if (known.has(attentionKey(signal)) || signal.replyThreadId === selectedReplyThreadId) continue;
    const updatedMs = Date.parse(signal.updatedAt);
    if (newest === null || updatedMs > newestMs) {
      newest = signal;
      newestMs = updatedMs;
    }
  }
  return newest;
}

/** What decides whether a background change is spoken now. */
export interface BackgroundAnnouncementContext {
  /** Whether Chat is quiet on this device, read after the latest queue change. */
  quiet: boolean;
  /** Whether the person keeps background announcements on. */
  enabled: boolean;
  selectedReplyThreadId: ChatReplyThreadId | null;
}

/** Remembers which attention changes a live region already covered. */
export interface BackgroundAnnouncementTracker {
  /**
   * Takes the latest queue signals, or `null` when no queue is loaded, as after a vault switch. Returns whether a
   * fresh quiet read should now call {@link BackgroundAnnouncementTracker.decide}; the first snapshot after a reset
   * becomes the silent baseline instead.
   */
  observe(signals: readonly ChatAttentionSignal[] | null): boolean;
  /**
   * Returns the change to speak, if any. While quiet the baseline stays frozen, so the newest change is spoken
   * once quiet ends; with announcements off the baseline still advances.
   */
  decide(context: BackgroundAnnouncementContext): ChatAttentionSignal | null;
}

/** Creates the tracker one Chat pane uses for background announcements. */
export function createBackgroundAnnouncementTracker(): BackgroundAnnouncementTracker {
  let baseline: readonly ChatAttentionSignal[] | null = null;
  let latest: readonly ChatAttentionSignal[] | null = null;
  return {
    observe(signals) {
      latest = signals;
      if (signals === null || baseline === null) {
        baseline = signals;
        return false;
      }
      return true;
    },
    decide({ quiet, enabled, selectedReplyThreadId }) {
      if (latest === null || baseline === null) {
        baseline = latest;
        return null;
      }
      if (quiet) return null;
      const signal = nextBackgroundAnnouncement(baseline, latest, selectedReplyThreadId);
      baseline = latest;
      return enabled ? signal : null;
    },
  };
}
