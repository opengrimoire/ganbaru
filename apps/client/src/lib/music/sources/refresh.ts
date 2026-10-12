import {
  cancelMusicRefresh,
  getMusicRefreshProgress,
  startMusicLocalRefresh,
} from "$lib/api/music-library";
import type {
  MusicLocalRefreshRequest,
  MusicRefreshJobProgress,
} from "$lib/music/library/contracts";

export type MusicSourceRefreshTarget =
  | {
      collectionId: string;
      kind: "local-root";
      name: string;
      request: MusicLocalRefreshRequest;
    }
  | {
      collectionId: string;
      kind: "youtube-playlist";
      name: string;
    };

export type MusicSourceRefreshRunState =
  | "queued"
  | "awaiting-network-confirmation"
  | "running"
  | "completed"
  | "partial"
  | "failed"
  | "cancelled";

export interface MusicSourceRefreshStatus {
  collectionId: string;
  kind: MusicSourceRefreshTarget["kind"];
  name: string;
  state: MusicSourceRefreshRunState;
  progress: MusicRefreshJobProgress | null;
  error: string | null;
}

export interface MusicSourceRefreshPlan {
  id: number;
  targets: MusicSourceRefreshTarget[];
  localCount: number;
  onlineCount: number;
  requiresNetworkConfirmation: boolean;
}

export interface MusicSourceRefreshRunOptions {
  allowNetwork: boolean;
}

interface MusicSourceRefreshDependencies {
  startLocal(request: MusicLocalRefreshRequest): Promise<MusicRefreshJobProgress>;
  getProgress(jobId: string): Promise<MusicRefreshJobProgress>;
  cancelLocal(jobId: string, cancelledAt: number): Promise<MusicRefreshJobProgress>;
  refreshYouTube(target: Extract<MusicSourceRefreshTarget, { kind: "youtube-playlist" }>, signal: AbortSignal): Promise<void>;
  wait(ms: number, signal: AbortSignal): Promise<void>;
  now(): number;
  onStatus?(status: MusicSourceRefreshStatus): void;
}

export interface MusicSourceRefreshController {
  prepare(targets: MusicSourceRefreshTarget[]): MusicSourceRefreshPlan;
  run(plan: MusicSourceRefreshPlan, options: MusicSourceRefreshRunOptions): Promise<MusicSourceRefreshStatus[]>;
  cancel(collectionId?: string): Promise<void>;
  statuses(): MusicSourceRefreshStatus[];
}

const TERMINAL_STATES = new Set<MusicRefreshJobProgress["state"]>([
  "completed",
  "partial",
  "failed",
  "cancelled",
]);

const defaultDependencies: Omit<MusicSourceRefreshDependencies, "refreshYouTube"> = {
  startLocal: startMusicLocalRefresh,
  getProgress: getMusicRefreshProgress,
  cancelLocal: cancelMusicRefresh,
  wait: (ms, signal) => new Promise<void>((resolve, reject) => {
    const timeout = window.setTimeout(resolve, ms);
    signal.addEventListener("abort", () => {
      window.clearTimeout(timeout);
      reject(new DOMException("Refresh cancelled", "AbortError"));
    }, { once: true });
  }),
  now: Date.now,
};

export function createMusicSourceRefreshController(
  refreshYouTube: MusicSourceRefreshDependencies["refreshYouTube"],
  overrides: Partial<Omit<MusicSourceRefreshDependencies, "refreshYouTube">> = {},
  concurrency = 2,
  progressIntervalMs = 400,
): MusicSourceRefreshController {
  if (!Number.isSafeInteger(concurrency) || concurrency < 1 || concurrency > 4) {
    throw new Error("Music source refresh concurrency must be between 1 and 4.");
  }
  if (!Number.isSafeInteger(progressIntervalMs) || progressIntervalMs < 100) {
    throw new Error("Music source refresh progress interval must be at least 100 milliseconds.");
  }
  const dependencies: MusicSourceRefreshDependencies = {
    ...defaultDependencies,
    ...overrides,
    refreshYouTube,
  };
  let planSequence = 0;
  let activeGeneration = 0;
  const statusByCollection = new Map<string, MusicSourceRefreshStatus>();
  const abortByCollection = new Map<string, AbortController>();
  const localJobByCollection = new Map<string, string>();

  function publish(status: MusicSourceRefreshStatus): void {
    statusByCollection.set(status.collectionId, status);
    dependencies.onStatus?.({ ...status });
  }

  function statusFor(
    target: MusicSourceRefreshTarget,
    state: MusicSourceRefreshRunState,
    progress: MusicRefreshJobProgress | null = null,
    error: string | null = null,
  ): MusicSourceRefreshStatus {
    return {
      collectionId: target.collectionId,
      kind: target.kind,
      name: target.name,
      state,
      progress,
      error,
    };
  }

  function prepare(targets: MusicSourceRefreshTarget[]): MusicSourceRefreshPlan {
    const seen = new Set<string>();
    const uniqueTargets = targets.filter((target) => {
      if (!target.collectionId.trim()) throw new Error("Every music source requires a collection id.");
      if (seen.has(target.collectionId)) return false;
      seen.add(target.collectionId);
      return true;
    });
    const onlineCount = uniqueTargets.filter((target) => target.kind === "youtube-playlist").length;
    return {
      id: ++planSequence,
      targets: uniqueTargets,
      localCount: uniqueTargets.length - onlineCount,
      onlineCount,
      requiresNetworkConfirmation: onlineCount > 0,
    };
  }

  async function refreshLocal(
    target: Extract<MusicSourceRefreshTarget, { kind: "local-root" }>,
    controller: AbortController,
    generation: number,
  ): Promise<void> {
    let progress = await dependencies.startLocal(target.request);
    if (generation !== activeGeneration || controller.signal.aborted) return;
    localJobByCollection.set(target.collectionId, progress.jobId);
    publish(statusFor(target, progress.state, progress));
    while (!TERMINAL_STATES.has(progress.state)) {
      await dependencies.wait(progressIntervalMs, controller.signal);
      progress = await dependencies.getProgress(progress.jobId);
      if (generation !== activeGeneration || controller.signal.aborted) return;
      publish(statusFor(target, progress.state, progress));
    }
  }

  async function refreshTarget(
    target: MusicSourceRefreshTarget,
    generation: number,
  ): Promise<void> {
    const controller = new AbortController();
    abortByCollection.set(target.collectionId, controller);
    publish(statusFor(target, "running"));
    try {
      if (target.kind === "local-root") {
        await refreshLocal(target, controller, generation);
      } else {
        await dependencies.refreshYouTube(target, controller.signal);
        if (generation === activeGeneration && !controller.signal.aborted) {
          publish(statusFor(target, "completed"));
        }
      }
    } catch (error) {
      if (generation !== activeGeneration) return;
      if (controller.signal.aborted || (error instanceof DOMException && error.name === "AbortError")) {
        publish(statusFor(target, "cancelled"));
      } else {
        publish(statusFor(target, "failed", null, error instanceof Error ? error.message : String(error)));
      }
    } finally {
      abortByCollection.delete(target.collectionId);
      localJobByCollection.delete(target.collectionId);
    }
  }

  async function run(
    plan: MusicSourceRefreshPlan,
    options: MusicSourceRefreshRunOptions,
  ): Promise<MusicSourceRefreshStatus[]> {
    if (plan.id > planSequence || plan.id <= 0) throw new Error("The music source refresh plan is invalid.");
    const generation = ++activeGeneration;
    statusByCollection.clear();
    const runnable: MusicSourceRefreshTarget[] = [];
    for (const target of plan.targets) {
      if (target.kind === "youtube-playlist" && !options.allowNetwork) {
        publish(statusFor(target, "awaiting-network-confirmation"));
      } else {
        publish(statusFor(target, "queued"));
        runnable.push(target);
      }
    }
    let nextIndex = 0;
    async function worker(): Promise<void> {
      while (generation === activeGeneration) {
        const target = runnable[nextIndex++];
        if (!target) return;
        await refreshTarget(target, generation);
      }
    }
    await Promise.all(Array.from({ length: Math.min(concurrency, runnable.length) }, worker));
    return [...statusByCollection.values()];
  }

  async function cancel(collectionId?: string): Promise<void> {
    const ids = collectionId ? [collectionId] : [...abortByCollection.keys()];
    if (!collectionId) activeGeneration += 1;
    await Promise.all(ids.map(async (id) => {
      abortByCollection.get(id)?.abort();
      const jobId = localJobByCollection.get(id);
      if (jobId) {
        try {
          await dependencies.cancelLocal(jobId, dependencies.now());
        } catch (error) {
          console.error("Unable to cancel the persistent local music refresh.", error);
        }
      }
      const status = statusByCollection.get(id);
      if (status && !["completed", "partial", "failed", "cancelled"].includes(status.state)) {
        publish({ ...status, state: "cancelled" });
      }
    }));
  }

  return {
    prepare,
    run,
    cancel,
    statuses: () => [...statusByCollection.values()],
  };
}
