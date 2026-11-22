/** One tab kept out of the strip while it moves to another window. */
interface TabDeparture<Origin> {
  /** Names this hold, so a late failure of an earlier hold of the same tab cannot release a newer one. */
  readonly token: symbol;
  readonly origin: Origin;
  readonly timer: ReturnType<typeof setTimeout>;
}

export interface TabDeparturesOptions<Origin> {
  /** How long a tab may take to reach its new window before it comes back. */
  readonly timeoutMs: number;
  /**
   * Brings back a tab whose move failed or did not finish in time. It must call `remove` once, where the tab
   * should stop being held out of the strip.
   */
  readonly release: (id: string, origin: Origin, remove: () => void) => void;
}

/**
 * The tabs this window dropped elsewhere, each kept out of the strip with its own timeout and original position
 * until it reaches its new window or comes back.
 */
export class TabDepartures<Origin> {
  #entries = $state.raw<ReadonlyMap<string, TabDeparture<Origin>>>(new Map());
  readonly #options: TabDeparturesOptions<Origin>;

  /**
   * @param options The timeout and how a tab comes back.
   */
  constructor(options: TabDeparturesOptions<Origin>) {
    this.#options = options;
  }

  /**
   * Whether the tab is kept out of the strip on its way to another window.
   *
   * @param id The tab.
   */
  has(id: string): boolean {
    return this.#entries.has(id);
  }

  /** The tabs kept out of the strip. */
  get ids(): string[] {
    return [...this.#entries.keys()];
  }

  /**
   * Keeps the tab out of the strip until it is gone. When its `move` fails, or it has not gone in time, it is
   * released with `origin`. Holding a tab again replaces its earlier hold.
   *
   * @param id The tab.
   * @param move The move to its new window, or null when another window already took it.
   * @param origin Where the tab started.
   */
  hold(id: string, move: Promise<void> | null, origin: Origin): void {
    const previous = this.#entries.get(id);
    if (previous) clearTimeout(previous.timer);
    const token = Symbol("tab departure");
    const timer = setTimeout(() => this.#release(id, token), this.#options.timeoutMs);
    this.#entries = new Map(this.#entries).set(id, { token, origin, timer });
    void move?.catch((error: unknown) => {
      console.error("Failed to move tab to a new window:", error);
      this.#release(id, token);
    });
  }

  /**
   * Stops holding the tabs that have reached their new window.
   *
   * @param present Whether a tab is still in this window's tab list.
   */
  forgetGone(present: (id: string) => boolean): void {
    for (const [id, entry] of this.#entries) {
      if (!present(id)) this.#remove(id, entry.token);
    }
  }

  /** Stops every timeout without releasing anything. */
  dispose(): void {
    for (const entry of this.#entries.values()) clearTimeout(entry.timer);
  }

  #release(id: string, token: symbol): void {
    const entry = this.#entries.get(id);
    if (entry?.token !== token) return;
    clearTimeout(entry.timer);
    this.#options.release(id, entry.origin, () => this.#remove(id, token));
  }

  #remove(id: string, token: symbol): void {
    const entry = this.#entries.get(id);
    if (entry?.token !== token) return;
    clearTimeout(entry.timer);
    const next = new Map(this.#entries);
    next.delete(id);
    this.#entries = next;
  }
}
