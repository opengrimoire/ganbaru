import * as chatApi from "$lib/api/chat";
import type {
  ChatReviewSnapshotRead,
  OpenChatReviewRequest,
} from "$lib/chat/contracts";

const REVIEW_PREFETCH_LIFETIME_MS = 30_000;

interface CachedReviewRequest {
  key: string;
  promise: Promise<ChatReviewSnapshotRead>;
  settledAt: number | null;
}

let cachedRequest: CachedReviewRequest | null = null;

/** Shares one bounded immutable review request between intent prefetch and panel opening. */
export function openChatReviewSingleFlight(
  request: OpenChatReviewRequest,
): Promise<ChatReviewSnapshotRead> {
  const key = JSON.stringify(request);
  const now = Date.now();
  if (cachedRequest?.key === key
    && (cachedRequest.settledAt === null
      || now - cachedRequest.settledAt <= REVIEW_PREFETCH_LIFETIME_MS)) {
    return cachedRequest.promise;
  }

  const cacheSettledResult = request.source.kind === "provider_turn";
  let entry: CachedReviewRequest;
  const promise = chatApi.openChatReview(request)
    .then((snapshot) => {
      if (cachedRequest === entry) {
        if (cacheSettledResult) entry.settledAt = Date.now();
        else cachedRequest = null;
      }
      return snapshot;
    })
    .catch((error: unknown) => {
      if (cachedRequest === entry) cachedRequest = null;
      throw error;
    });
  entry = { key, settledAt: null, promise };
  cachedRequest = entry;
  return entry.promise;
}

/** Starts speculative review work only when no review request is in flight. */
export function prefetchChatReview(request: OpenChatReviewRequest): void {
  if (cachedRequest?.settledAt === null) return;
  void openChatReviewSingleFlight(request).catch(() => undefined);
}
