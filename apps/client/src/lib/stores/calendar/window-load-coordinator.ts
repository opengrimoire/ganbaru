export type WindowLoadOutcome = "applied" | "superseded";

export type WindowLoadEvent<TRequest> =
  | { type: "start"; key: string; request: TRequest }
  | { type: "queue"; key: string; replacedKey?: string; request: TRequest }
  | { type: "drop"; key: string; reason: "superseded" }
  | { type: "finish"; key: string; outcome: WindowLoadOutcome }
  | { type: "error"; key: string; error: unknown };

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (error: unknown) => void;
};

type QueueEntry<TRequest> = {
  key: string;
  request: TRequest;
  deferred: Deferred<WindowLoadOutcome>;
  superseded: boolean;
};

export type WindowLoadRunner<TRequest> = (
  request: TRequest,
  isSuperseded: () => boolean,
) => Promise<WindowLoadOutcome>;

export type WindowLoadEventHandler<TRequest> = (event: WindowLoadEvent<TRequest>) => void;

function createDeferred<T>(): Deferred<T> {
  let resolve: (value: T) => void = () => undefined;
  let reject: (error: unknown) => void = () => undefined;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

export class LatestWindowLoadCoordinator<TRequest> {
  #active: QueueEntry<TRequest> | null = null;
  #queued: QueueEntry<TRequest> | null = null;
  readonly #run: WindowLoadRunner<TRequest>;
  readonly #onEvent?: WindowLoadEventHandler<TRequest>;
  #idleWaiters: Array<() => void> = [];

  constructor(
    run: WindowLoadRunner<TRequest>,
    onEvent?: WindowLoadEventHandler<TRequest>,
  ) {
    this.#run = run;
    this.#onEvent = onEvent;
  }

  get activeKey(): string | null {
    return this.#active?.key ?? null;
  }

  get queuedKey(): string | null {
    return this.#queued?.key ?? null;
  }

  get busy(): boolean {
    return this.#active !== null || this.#queued !== null;
  }

  enqueue(key: string, request: TRequest): Promise<WindowLoadOutcome> {
    if (this.#active?.key === key) return this.#active.deferred.promise;
    if (this.#queued?.key === key) return this.#queued.deferred.promise;

    const entry: QueueEntry<TRequest> = {
      key,
      request,
      deferred: createDeferred<WindowLoadOutcome>(),
      superseded: false,
    };

    if (!this.#active) {
      this.#start(entry);
      return entry.deferred.promise;
    }

    if (this.#queued) {
      this.#onEvent?.({ type: "drop", key: this.#queued.key, reason: "superseded" });
      this.#queued.deferred.resolve("superseded");
    }

    const replacedKey = this.#queued?.key;
    this.#queued = entry;
    this.#onEvent?.({ type: "queue", key, replacedKey, request });
    return entry.deferred.promise;
  }

  isSuperseded(key: string): boolean {
    return (this.#active?.key === key && this.#active.superseded)
      || (this.#queued !== null && this.#queued.key !== key);
  }

  supersedeActive(): void {
    if (!this.#active || this.#active.superseded) return;
    this.#active.superseded = true;
    this.#onEvent?.({ type: "drop", key: this.#active.key, reason: "superseded" });
  }

  supersedePending(): void {
    this.supersedeActive();
    if (!this.#queued) return;
    this.#onEvent?.({ type: "drop", key: this.#queued.key, reason: "superseded" });
    this.#queued.deferred.resolve("superseded");
    this.#queued = null;
    this.#resolveIdle();
  }

  whenIdle(): Promise<void> {
    if (!this.busy) return Promise.resolve();
    return new Promise((resolve) => {
      this.#idleWaiters.push(resolve);
    });
  }

  #start(entry: QueueEntry<TRequest>): void {
    this.#active = entry;
    this.#onEvent?.({ type: "start", key: entry.key, request: entry.request });

    void (async () => {
      try {
        const outcome = await this.#run(entry.request, () => this.isSuperseded(entry.key));
        entry.deferred.resolve(outcome);
        this.#onEvent?.({ type: "finish", key: entry.key, outcome });
      } catch (error) {
        entry.deferred.reject(error);
        this.#onEvent?.({ type: "error", key: entry.key, error });
      } finally {
        if (this.#active === entry) this.#active = null;
        const next = this.#queued;
        this.#queued = null;
        if (next) {
          this.#start(next);
        } else {
          this.#resolveIdle();
        }
      }
    })();
  }

  #resolveIdle(): void {
    if (this.busy) return;
    const waiters = this.#idleWaiters;
    this.#idleWaiters = [];
    for (const resolve of waiters) resolve();
  }
}

export class BoundedWindowCache<TValue> {
  readonly #limit: number;
  readonly #entries = new Map<string, TValue>();

  constructor(limit: number) {
    if (!Number.isInteger(limit) || limit < 1) {
      throw new Error("BoundedWindowCache limit must be a positive integer");
    }
    this.#limit = limit;
  }

  get size(): number {
    return this.#entries.size;
  }

  has(key: string): boolean {
    return this.#entries.has(key);
  }

  peek(key: string): TValue | undefined {
    return this.#entries.get(key);
  }

  get(key: string): TValue | undefined {
    const value = this.#entries.get(key);
    if (value === undefined) return undefined;
    this.#entries.delete(key);
    this.#entries.set(key, value);
    return value;
  }

  find(predicate: (value: TValue, key: string) => boolean): TValue | undefined {
    for (const [key, value] of this.#entries) {
      if (predicate(value, key)) return value;
    }
    return undefined;
  }

  set(key: string, value: TValue): void {
    if (this.#entries.has(key)) this.#entries.delete(key);
    this.#entries.set(key, value);
    while (this.#entries.size > this.#limit) {
      const first = this.#entries.keys().next().value;
      if (typeof first !== "string") break;
      this.#entries.delete(first);
    }
  }

  clear(): void {
    this.#entries.clear();
  }
}
