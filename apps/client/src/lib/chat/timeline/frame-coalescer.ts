export type FrameScheduler = (callback: () => void) => () => void;

function browserFrameScheduler(callback: () => void): () => void {
  if (typeof requestAnimationFrame === "function") {
    const frame = requestAnimationFrame(() => callback());
    return () => cancelAnimationFrame(frame);
  }
  const timer = globalThis.setTimeout(callback, 16);
  return () => globalThis.clearTimeout(timer);
}

interface PendingKey<Value> {
  value: Value;
  waiters: Array<{ resolve: () => void; reject: (error: unknown) => void }>;
}

/**
 * Collects keys submitted within a paint frame and runs the async consumer once per distinct key, in the order the
 * keys first arrived. Values pushed for a key that is already pending are combined with `merge`. Consumers run one at
 * a time, so a slow refresh cannot overlap the next one; keys pushed while a frame runs wait for the next frame.
 */
export class AsyncFrameKeyedCoalescer<Key, Value> {
  readonly #consume: (key: Key, value: Value) => Promise<void>;
  readonly #merge: (current: Value, next: Value) => Value;
  readonly #schedule: FrameScheduler;
  #pending = new Map<Key, PendingKey<Value>>();
  #cancelFrame: (() => void) | null = null;
  #running = false;

  constructor(
    consume: (key: Key, value: Value) => Promise<void>,
    merge: (current: Value, next: Value) => Value,
    schedule: FrameScheduler = browserFrameScheduler,
  ) {
    this.#consume = consume;
    this.#merge = merge;
    this.#schedule = schedule;
  }

  /** Queues `key` for the next frame; resolves after its consumer ran, or rejects with the consumer's error. */
  push(key: Key, value: Value): Promise<void> {
    const pending = this.#pending.get(key);
    const completion = new Promise<void>((resolve, reject) => {
      if (pending) {
        pending.value = this.#merge(pending.value, value);
        pending.waiters.push({ resolve, reject });
      } else {
        this.#pending.set(key, { value, waiters: [{ resolve, reject }] });
      }
    });
    this.#ensureScheduled();
    return completion;
  }

  #ensureScheduled(): void {
    if (this.#running || this.#cancelFrame || this.#pending.size === 0) return;
    this.#cancelFrame = this.#schedule(() => {
      this.#cancelFrame = null;
      void this.#flush();
    });
  }

  async #flush(): Promise<void> {
    const batch = this.#pending;
    if (batch.size === 0) return;
    this.#pending = new Map();
    this.#running = true;
    try {
      for (const [key, { value, waiters }] of batch) {
        try {
          await this.#consume(key, value);
          for (const waiter of waiters) waiter.resolve();
        } catch (error: unknown) {
          for (const waiter of waiters) waiter.reject(error);
        }
      }
    } finally {
      this.#running = false;
      this.#ensureScheduled();
    }
  }
}
