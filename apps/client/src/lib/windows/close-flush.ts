const flushers = new Set<() => Promise<void>>();

/**
 * Register a save that must finish before this window closes.
 *
 * @param flush Writes the owner's pending edits.
 * @returns A function that removes the registration.
 */
export function registerWindowCloseFlusher(flush: () => Promise<void>): () => void {
  flushers.add(flush);
  return () => flushers.delete(flush);
}

/**
 * Run every registered save, letting each finish even when another fails.
 *
 * @throws The first failure, so the window stays open with its edits.
 */
export async function flushBeforeWindowClose(): Promise<void> {
  const results = await Promise.allSettled([...flushers].map((flush) => flush()));
  const failure = results.find((result) => result.status === "rejected");
  if (failure) throw failure.reason;
}
