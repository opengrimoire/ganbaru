export interface MobilePersistenceTarget {
  readonly visibilityState: DocumentVisibilityState;
  addEventListener(type: "visibilitychange", listener: () => void): void;
  removeEventListener(type: "visibilitychange", listener: () => void): void;
}

export interface MobilePageLifecycleTarget {
  addEventListener(type: "pagehide", listener: () => void): void;
  removeEventListener(type: "pagehide", listener: () => void): void;
}

export interface MobilePersistenceLifecycleOptions {
  readonly documentTarget: MobilePersistenceTarget;
  readonly windowTarget: MobilePageLifecycleTarget;
  readonly flushers: Readonly<Record<string, () => Promise<void>>>;
  readonly onError?: (label: string, error: unknown) => void;
}

/**
 * Flush short JavaScript persistence debounces before Android can freeze the
 * WebView. Repeated lifecycle signals are serialized and trigger one final
 * pass if another signal arrives while a flush is active.
 */
export class MobilePersistenceLifecycleController {
  readonly #options: MobilePersistenceLifecycleOptions;
  #activeFlush: Promise<void> | null = null;
  #rerunRequested = false;
  #attached = false;

  constructor(options: MobilePersistenceLifecycleOptions) {
    this.#options = options;
  }

  attach(): () => void {
    if (this.#attached) return () => this.detach();
    this.#attached = true;
    this.#options.documentTarget.addEventListener("visibilitychange", this.#onVisibilityChange);
    this.#options.windowTarget.addEventListener("pagehide", this.#onPageHide);
    return () => this.detach();
  }

  detach(): void {
    if (!this.#attached) return;
    this.#attached = false;
    this.#options.documentTarget.removeEventListener("visibilitychange", this.#onVisibilityChange);
    this.#options.windowTarget.removeEventListener("pagehide", this.#onPageHide);
  }

  flush(): Promise<void> {
    if (this.#activeFlush) {
      this.#rerunRequested = true;
      return this.#activeFlush;
    }

    this.#activeFlush = this.#runFlushPasses().finally(() => {
      this.#activeFlush = null;
    });
    return this.#activeFlush;
  }

  async #runFlushPasses(): Promise<void> {
    do {
      this.#rerunRequested = false;
      const entries = Object.entries(this.#options.flushers);
      const results = await Promise.allSettled(entries.map(([, flusher]) => flusher()));
      results.forEach((result, index) => {
        if (result.status === "rejected") {
          this.#options.onError?.(entries[index]?.[0] ?? "unknown", result.reason);
        }
      });
    } while (this.#rerunRequested);
  }

  readonly #onVisibilityChange = (): void => {
    if (this.#options.documentTarget.visibilityState === "hidden") void this.flush();
  };

  readonly #onPageHide = (): void => {
    void this.flush();
  };
}
