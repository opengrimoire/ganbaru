import type { ChatThreadId } from "$lib/chat/contracts";
import type { TimelineActivityRow } from "./types";

/** The UTF-8 length a native timeline read keeps of an activity detail, mirroring the Rust read. */
export const ACTIVITY_DETAIL_PREVIEW_BYTES = 4096;

/** How many whole activity details stay loaded after their disclosures open. */
export const MAX_CACHED_ACTIVITY_DETAILS = 16;

/** Activity kinds whose preview keeps the end of the detail, where the newest output is. */
const TAIL_PREVIEW_ACTIVITY_KINDS: ReadonlySet<TimelineActivityRow["activityKind"]> = new Set([
  "command_execution",
  "command_output",
  "reasoning_summary",
]);

/** Returns whether a shortened preview of this activity shows the end of its detail. */
export function activityDetailPreviewKeepsTail(activity: Pick<TimelineActivityRow, "activityKind">): boolean {
  return TAIL_PREVIEW_ACTIVITY_KINDS.has(activity.activityKind);
}

/** Reads the whole detail of one activity. */
export type ActivityDetailReader = (threadId: ChatThreadId, activityId: string) => Promise<string | null>;

/**
 * Holds whole activity details loaded on demand, keyed by activity version so a changed activity
 * reloads, and bounded so long conversations do not keep every opened output in memory.
 */
export class ActivityDetailCache {
  readonly #entries = new Map<string, Promise<string | null>>();

  /**
   * @param read Reads one whole activity detail.
   * @param limit The most details kept at once; the least recently used one leaves first.
   */
  constructor(
    private readonly read: ActivityDetailReader,
    private readonly limit = MAX_CACHED_ACTIVITY_DETAILS,
  ) {}

  /** Loads the whole detail of a shortened activity, sharing a read already in flight. */
  load(threadId: ChatThreadId, activity: Pick<TimelineActivityRow, "id" | "updatedAt">): Promise<string | null> {
    const key = `${threadId}\u0000${activity.id}\u0000${activity.updatedAt ?? ""}`;
    const cached = this.#entries.get(key);
    if (cached) {
      this.#entries.delete(key);
      this.#entries.set(key, cached);
      return cached;
    }
    const pending = this.read(threadId, activity.id);
    this.#entries.set(key, pending);
    pending.catch(() => {
      if (this.#entries.get(key) === pending) this.#entries.delete(key);
    });
    while (this.#entries.size > this.limit) {
      const oldest = this.#entries.keys().next();
      if (oldest.done) break;
      this.#entries.delete(oldest.value);
    }
    return pending;
  }

  /** The number of details held, for tests. */
  get size(): number {
    return this.#entries.size;
  }
}
