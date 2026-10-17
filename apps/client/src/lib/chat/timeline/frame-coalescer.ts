export type FrameScheduler = (callback: () => void) => () => void;

function browserFrameScheduler(callback: () => void): () => void {
  if (typeof requestAnimationFrame === "function") {
    const frame = requestAnimationFrame(() => callback());
    return () => cancelAnimationFrame(frame);
  }
  const timer = globalThis.setTimeout(callback, 16);
  return () => globalThis.clearTimeout(timer);
}

/**
 * Keeps only the newest value submitted within a paint frame and serializes
 * async consumers so a slow refresh cannot overlap the next refresh.
 */
export class AsyncFrameCoalescer<T> {
  readonly #consume: (value: T) => Promise<void>;
  readonly #schedule: FrameScheduler;
  #pending: T | undefined;
  #waiters: Array<{ resolve: () => void; reject: (error: unknown) => void }> = [];
  #cancelFrame: (() => void) | null = null;
  #running = false;

  constructor(
    consume: (value: T) => Promise<void>,
    schedule: FrameScheduler = browserFrameScheduler,
  ) {
    this.#consume = consume;
    this.#schedule = schedule;
  }

  push(value: T): Promise<void> {
    this.#pending = value;
    const completion = new Promise<void>((resolve, reject) => {
      this.#waiters.push({ resolve, reject });
    });
    this.#ensureScheduled();
    return completion;
  }

  #ensureScheduled(): void {
    if (this.#running || this.#cancelFrame || this.#pending === undefined) return;
    this.#cancelFrame = this.#schedule(() => {
      this.#cancelFrame = null;
      void this.#flush();
    });
  }

  async #flush(): Promise<void> {
    const value = this.#pending;
    if (value === undefined) return;
    const waiters = this.#waiters;
    this.#pending = undefined;
    this.#waiters = [];
    this.#running = true;
    try {
      await this.#consume(value);
      for (const waiter of waiters) waiter.resolve();
    } catch (error: unknown) {
      for (const waiter of waiters) waiter.reject(error);
    } finally {
      this.#running = false;
      this.#ensureScheduled();
    }
  }
}
