/**
 * Observes the size of live timeline refresh reads. The read path checks
 * {@link hasTimelineReadObservers} first, so the size is measured only while
 * something, such as the stream flood benchmark, is listening.
 */

/** One live timeline refresh read as it arrived from the backend. */
export interface TimelineSinceReadSample {
  /** UTF-8 size of the read's JSON payload. */
  bytes: number;
  /** Rows the read carried. */
  itemCount: number;
}

export type TimelineSinceReadObserver = (sample: TimelineSinceReadSample) => void;

const observers = new Set<TimelineSinceReadObserver>();
const encoder = new TextEncoder();

/**
 * Starts observing live timeline refresh reads.
 *
 * @returns A function that stops this observer.
 */
export function observeTimelineSinceReads(observer: TimelineSinceReadObserver): () => void {
  observers.add(observer);
  return () => {
    observers.delete(observer);
  };
}

/** Whether any observer is listening, so callers skip measuring otherwise. */
export function hasTimelineReadObservers(): boolean {
  return observers.size > 0;
}

/**
 * Measures one raw refresh read and reports it to every observer.
 *
 * @param payload The unparsed backend response.
 * @param itemCount Rows the parsed read carried.
 */
export function reportTimelineSinceRead(payload: unknown, itemCount: number): void {
  if (observers.size === 0) return;
  const bytes = encoder.encode(JSON.stringify(payload) ?? "").length;
  for (const observer of observers) observer({ bytes, itemCount });
}
