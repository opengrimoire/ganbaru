import * as chatApi from "$lib/api/chat";
import type { ChatChannelId, ChatReviewQueueRead, ChatWorkAssignmentId } from "$lib/chat/contracts";
import { chatErrorCode, chatErrorMessage } from "$lib/chat/error-presentation";
import {
  batchAcceptCandidates,
  summarizeBatchAccept,
  type ChatBatchAcceptOutcome,
  type ChatBatchAcceptSummary,
} from "$lib/chat/review-queue/model";

export interface ChatReviewQueueControllerOptions {
  /** Called after a batch accept recorded at least one outcome, with the channels it touched. */
  onReviewed: (channelIds: ChatChannelId[]) => void;
}

/**
 * Owns the vault-wide review queue: the assignments that wait for the person across every
 * project. It always reads every project, because the rail and the notification gate both need
 * the whole queue.
 */
export class ChatReviewQueueController {
  /** The last queue read, or `null` before the first read succeeds. */
  queue = $state<ChatReviewQueueRead | null>(null);
  /** Why the last refresh failed, while the previous queue stays shown. */
  error = $state<string | null>(null);

  private generation = 0;
  private inFlight: Promise<void> | null = null;
  private queued: Promise<void> | null = null;

  constructor(private readonly options: ChatReviewQueueControllerOptions) {}

  /** Forgets the queue of the previous vault and drops pending reads. */
  reset(): void {
    this.generation += 1;
    this.queue = null;
    this.error = null;
    this.inFlight = null;
    this.queued = null;
  }

  /**
   * Rereads the queue. One read runs at a time and at most one more waits behind it, so a burst
   * of changes costs two reads. A failed read keeps the previous queue and sets `error`.
   */
  refresh(): Promise<void> {
    if (this.queued) return this.queued;
    const running = this.inFlight;
    if (!running) return this.startRefresh();
    const generation = this.generation;
    const queued = running.then(() => {
      if (generation !== this.generation) return;
      this.queued = null;
      return this.startRefresh();
    });
    this.queued = queued;
    return queued;
  }

  /**
   * Accepts the selected reviews one at a time, each against the revision the queue showed, then
   * rereads the queue once. Every accept is its own transaction, so a partial batch keeps what
   * was recorded.
   */
  async acceptReviewItems(ids: readonly ChatWorkAssignmentId[]): Promise<ChatBatchAcceptSummary> {
    const candidates = batchAcceptCandidates(this.queue?.items ?? [], new Set(ids));
    const results: ChatBatchAcceptOutcome[] = [];
    const reviewedChannelIds = new Set<ChatChannelId>();
    for (const item of candidates) {
      try {
        await chatApi.reviewChatAssignment(item.assignmentId, item.revision, { kind: "accept" });
        results.push("accepted");
        reviewedChannelIds.add(item.channelId);
      } catch (error: unknown) {
        if (chatErrorCode(error) === "stale_revision") {
          results.push("stale");
        } else {
          console.error("Chat review could not be accepted", error);
          results.push("failed");
        }
      }
    }
    if (reviewedChannelIds.size > 0) this.options.onReviewed([...reviewedChannelIds]);
    await this.refresh();
    return summarizeBatchAccept(results);
  }

  private startRefresh(): Promise<void> {
    const generation = this.generation;
    const read = this.read(generation).finally(() => {
      if (this.inFlight === read) this.inFlight = null;
    });
    this.inFlight = read;
    return read;
  }

  private async read(generation: number): Promise<void> {
    try {
      const queue = await chatApi.listChatReviewQueue(null);
      if (generation !== this.generation) return;
      this.queue = queue;
      this.error = null;
    } catch (error: unknown) {
      if (generation !== this.generation) return;
      console.error("Chat review queue could not be read", error);
      this.error = chatErrorMessage(error);
    }
  }
}
