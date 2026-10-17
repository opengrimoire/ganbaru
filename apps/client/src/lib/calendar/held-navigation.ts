export type HeldNavigationKey = "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown";
export type HeldNavigationDirection = "forward" | "back";

export const NAV_HOLD_DELAY_MS = 280;
export const NAV_REPEAT_MS = 120;

export type HeldNavigationEvent =
  | { type: "hold-start"; key: HeldNavigationKey; direction: HeldNavigationDirection }
  | { type: "hold-stop"; key: HeldNavigationKey; repeats: number }
  | { type: "repeat-skip"; key: HeldNavigationKey; repeats: number; reason: "not-ready" }
  | { type: "repeat"; key: HeldNavigationKey; direction: HeldNavigationDirection; repeats: number }
  | { type: "repeat-cancelled"; key: HeldNavigationKey; stage: "delay" | "tick" };

type TimerId = ReturnType<typeof setTimeout>;

function defaultSetTimer(callback: () => void, delayMs: number): TimerId {
  return globalThis.setTimeout(callback, delayMs);
}

function defaultClearTimer(id: TimerId): void {
  globalThis.clearTimeout(id);
}

export interface HeldNavigationControllerOptions {
  holdDelayMs: number;
  repeatMs: number;
  navigate: (direction: HeldNavigationDirection, source: "key" | "hold-repeat") => void;
  canRepeat: (direction: HeldNavigationDirection) => boolean;
  mark?: (event: HeldNavigationEvent) => void;
  setTimer?: (callback: () => void, delayMs: number) => TimerId;
  clearTimer?: (id: TimerId) => void;
  now?: () => number;
}

export class HeldNavigationController {
  readonly #holdDelayMs: number;
  readonly #repeatMs: number;
  readonly #navigate: HeldNavigationControllerOptions["navigate"];
  readonly #canRepeat: HeldNavigationControllerOptions["canRepeat"];
  readonly #mark?: (event: HeldNavigationEvent) => void;
  readonly #setTimer: (callback: () => void, delayMs: number) => TimerId;
  readonly #clearTimer: (id: TimerId) => void;
  readonly #now: () => number;

  #key: HeldNavigationKey | null = null;
  #direction: HeldNavigationDirection | null = null;
  #repeatTimer: TimerId | null = null;
  #generation = 0;
  #repeats = 0;
  #startedAt = 0;
  #repeatArmed = false;

  constructor(opts: HeldNavigationControllerOptions) {
    this.#holdDelayMs = opts.holdDelayMs;
    this.#repeatMs = opts.repeatMs;
    this.#navigate = opts.navigate;
    this.#canRepeat = opts.canRepeat;
    this.#mark = opts.mark;
    this.#setTimer = opts.setTimer ?? defaultSetTimer;
    this.#clearTimer = opts.clearTimer ?? defaultClearTimer;
    this.#now = opts.now ?? (() => globalThis.performance?.now() ?? Date.now());
  }

  get activeKey(): HeldNavigationKey | null {
    return this.#key;
  }

  start(key: HeldNavigationKey, direction: HeldNavigationDirection): void {
    if (this.#key === key) return;
    if (this.#key !== null) this.stop();

    this.#generation++;
    this.#key = key;
    this.#direction = direction;
    this.#repeats = 0;
    this.#startedAt = this.#now();
    this.#repeatArmed = false;
    this.#mark?.({ type: "hold-start", key, direction });
    this.#navigate(direction, "key");
  }

  armRepeatsFromKeydown(key: HeldNavigationKey): void {
    if (this.#key !== key) return;
    if (this.#repeatArmed) return;
    this.#repeatArmed = true;

    const generation = this.#generation;
    const elapsedMs = this.#now() - this.#startedAt;
    const delayMs = Math.max(0, this.#holdDelayMs - elapsedMs);
    this.#repeatTimer = this.#setTimer(() => {
      this.#repeatTimer = null;
      if (!this.#isActive(generation)) {
        this.#mark?.({ type: "repeat-cancelled", key, stage: "delay" });
        return;
      }
      this.#runRepeatTick(generation, key);
    }, delayMs);
  }

  stop(key?: HeldNavigationKey): HeldNavigationKey | null {
    if (key && this.#key !== key) return null;
    const stoppedKey = this.#key;
    if (stoppedKey === null) return null;

    this.#generation++;
    this.#key = null;
    this.#direction = null;
    this.#repeatArmed = false;
    this.#clearTimers();
    this.#mark?.({ type: "hold-stop", key: stoppedKey, repeats: this.#repeats });
    return stoppedKey;
  }

  #clearTimers(): void {
    if (this.#repeatTimer !== null) {
      this.#clearTimer(this.#repeatTimer);
      this.#repeatTimer = null;
    }
  }

  #isActive(generation: number): boolean {
    return this.#generation === generation && this.#key !== null && this.#direction !== null;
  }

  #runRepeatTick(generation: number, key: HeldNavigationKey): void {
    if (!this.#isActive(generation)) {
      this.#mark?.({ type: "repeat-cancelled", key, stage: "tick" });
      return;
    }

    const direction = this.#direction;
    if (direction === null) return;
    this.#tryRepeat(generation, key);

    if (!this.#isActive(generation)) return;
    this.#repeatTimer = this.#setTimer(() => {
      this.#repeatTimer = null;
      this.#runRepeatTick(generation, key);
    }, this.#repeatMs);
  }

  #tryRepeat(generation: number, key: HeldNavigationKey): void {
    if (!this.#isActive(generation)) return;
    const now = this.#now();
    if (now - this.#startedAt < this.#holdDelayMs) return;

    const direction = this.#direction;
    if (direction === null) return;
    if (!this.#canRepeat(direction)) {
      this.#mark?.({ type: "repeat-skip", key, repeats: this.#repeats, reason: "not-ready" });
      return;
    }

    this.#repeats++;
    this.#mark?.({ type: "repeat", key, direction, repeats: this.#repeats });
    this.#navigate(direction, "hold-repeat");
  }
}
