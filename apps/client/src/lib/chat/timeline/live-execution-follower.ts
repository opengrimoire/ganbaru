import type { ChatLiveFollowRead, ChatWorkAssignmentId } from "$lib/chat/contracts";
import {
  applyLiveExecutionPage,
  liveExecutionNotServedDelay,
  liveExecutionRetryDelay,
  liveExecutionWaitDelay,
  type LiveExecutionState,
} from "./live-execution";

/** Reads one page of the live execution of an assignment, as the `chat_read_live_execution` command does. */
export type LiveExecutionReader = (
  assignmentId: ChatWorkAssignmentId,
  afterChange: number | null,
  wait: boolean,
) => Promise<ChatLiveFollowRead>;

/** Runs `callback` after `delayMs` and returns a function that cancels it. */
export type LiveExecutionScheduler = (callback: () => void, delayMs: number) => () => void;

/**
 * Where following stands: reading the first page, following the run, trying again after a failed read, done because
 * the run settled, or not served because no other device runs the work yet.
 */
export type LiveExecutionStatus = "connecting" | "live" | "retrying" | "settled" | "notServed";

/** What the follower shows: the execution held so far and where following stands. */
export interface LiveExecutionView {
  state: LiveExecutionState | null;
  status: LiveExecutionStatus;
}

export interface LiveExecutionFollowerOptions {
  read: LiveExecutionReader;
  /** Receives every change to the view. */
  onUpdate: (view: LiveExecutionView) => void;
  schedule?: LiveExecutionScheduler;
  now?: () => number;
}

function timerScheduler(callback: () => void, delayMs: number): () => void {
  const timer = globalThis.setTimeout(callback, delayMs);
  return () => globalThis.clearTimeout(timer);
}

/**
 * Follows the live execution of one work assignment a linked desktop runs. Reads run one at a time: the first reads
 * the whole run, later ones wait on the desktop for the next change, a page with more changes is followed at once, and
 * a failed read is tried again after a growing delay. Work no device serves yet, as while the desktop has not claimed
 * it or its claim has not synced here, is asked about again after a growing delay. Following ends when the run
 * settles, and `pause` stops reading while keeping the rows held, so `follow` resumes from the same cursor.
 */
export class LiveExecutionFollower {
  readonly #read: LiveExecutionReader;
  readonly #onUpdate: (view: LiveExecutionView) => void;
  readonly #schedule: LiveExecutionScheduler;
  readonly #now: () => number;
  #assignmentId: ChatWorkAssignmentId | null = null;
  #state: LiveExecutionState | null = null;
  #status: LiveExecutionStatus = "connecting";
  #failures = 0;
  #notServedAnswers = 0;
  /** Changes whenever following stops or restarts, so a read that returns afterwards is dropped. */
  #generation = 0;
  #cancelScheduled: (() => void) | null = null;

  constructor(options: LiveExecutionFollowerOptions) {
    this.#read = options.read;
    this.#onUpdate = options.onUpdate;
    this.#schedule = options.schedule ?? timerScheduler;
    this.#now = options.now ?? (() => Date.now());
  }

  /** Starts or resumes following `assignmentId`; following another assignment drops the rows held. */
  follow(assignmentId: ChatWorkAssignmentId): void {
    this.pause();
    if (this.#assignmentId !== assignmentId) {
      this.#assignmentId = assignmentId;
      this.#state = null;
      this.#status = "connecting";
      this.#failures = 0;
      this.#notServedAnswers = 0;
      this.#publish();
    }
    if (this.#status === "settled") return;
    this.#run(this.#generation, false);
  }

  /** Stops reading and keeps the rows held. */
  pause(): void {
    this.#generation += 1;
    this.#cancelScheduled?.();
    this.#cancelScheduled = null;
  }

  #run(generation: number, wait: boolean): void {
    const assignmentId = this.#assignmentId;
    if (assignmentId === null) return;
    const startedAt = this.#now();
    const afterChange = this.#state?.changeCursor ?? null;
    void this.#read(assignmentId, afterChange, wait && afterChange !== null).then(
      (answer) => {
        if (generation !== this.#generation) return;
        this.#receive(generation, answer, startedAt);
      },
      (error: unknown) => {
        if (generation !== this.#generation) return;
        console.warn("chat: read the live execution", error);
        this.#retry(generation);
      },
    );
  }

  #receive(generation: number, answer: ChatLiveFollowRead, startedAt: number): void {
    if (answer.kind === "notServed") {
      this.#notServedAnswers += 1;
      if (this.#state !== null || this.#status !== "notServed") {
        this.#state = null;
        this.#status = "notServed";
        this.#publish();
      }
      this.#later(generation, liveExecutionNotServedDelay(this.#notServedAnswers), false);
      return;
    }
    if (answer.kind === "unreachable") {
      this.#retry(generation);
      return;
    }
    this.#failures = 0;
    this.#notServedAnswers = 0;
    const step = applyLiveExecutionPage(this.#state, answer.read);
    this.#state = step.state;
    this.#status = step.next === "stop" ? "settled" : step.state === null ? "connecting" : "live";
    this.#publish();
    if (step.next === "stop") return;
    if (step.next === "wait") {
      this.#later(generation, liveExecutionWaitDelay(this.#now() - startedAt), true);
      return;
    }
    this.#run(generation, false);
  }

  #retry(generation: number): void {
    this.#failures += 1;
    if (this.#status !== "retrying") {
      this.#status = "retrying";
      this.#publish();
    }
    this.#later(generation, liveExecutionRetryDelay(this.#failures), false);
  }

  #later(generation: number, delayMs: number, wait: boolean): void {
    if (delayMs === 0) {
      this.#run(generation, wait);
      return;
    }
    this.#cancelScheduled = this.#schedule(() => {
      this.#cancelScheduled = null;
      if (generation === this.#generation) this.#run(generation, wait);
    }, delayMs);
  }

  #publish(): void {
    this.#onUpdate({ state: this.#state, status: this.#status });
  }
}
