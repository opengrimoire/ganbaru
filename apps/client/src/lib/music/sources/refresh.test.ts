import { describe, expect, it, vi } from "vitest";
import type { MusicRefreshJobProgress } from "$lib/music/library/contracts";
import {
  createMusicSourceRefreshController,
  type MusicSourceRefreshTarget,
} from "./refresh";

function progress(state: MusicRefreshJobProgress["state"], jobId: string): MusicRefreshJobProgress {
  return {
    jobId,
    collectionId: jobId,
    rootId: jobId,
    kind: "local-root",
    state,
    generation: 1,
    discoveredCount: 1,
    processedCount: state === "completed" ? 1 : 0,
    skippedCount: 0,
    issueCount: 0,
    truncatedCount: 0,
    absenceDetermined: state === "completed",
    statusMessage: state,
    requestedAtMs: 1,
    startedAtMs: 1,
    finishedAtMs: state === "completed" ? 2 : null,
    updatedAtMs: 2,
  };
}

function local(collectionId: string): MusicSourceRefreshTarget {
  return {
    collectionId,
    kind: "local-root",
    name: collectionId,
    request: {
      jobId: collectionId,
      rootId: collectionId,
      collectionId,
      folderPath: "/music",
      availableRoots: [{ rootId: collectionId, folderPath: "/music" }],
      requestedAtMs: 1,
    },
  };
}

describe("music source refresh orchestration", () => {
  it("does not start online work before explicit network confirmation", async () => {
    const refreshYouTube = vi.fn(async () => undefined);
    const controller = createMusicSourceRefreshController(refreshYouTube, {
      startLocal: vi.fn(async (request) => progress("completed", request.jobId)),
      wait: vi.fn(async () => undefined),
    });
    const youtube: MusicSourceRefreshTarget = {
      collectionId: "youtube-1",
      kind: "youtube-playlist",
      name: "Online",
    };
    const plan = controller.prepare([youtube]);
    expect(plan.requiresNetworkConfirmation).toBe(true);
    const statuses = await controller.run(plan, { allowNetwork: false });
    expect(refreshYouTube).not.toHaveBeenCalled();
    expect(statuses[0]?.state).toBe("awaiting-network-confirmation");
  });

  it("bounds concurrency and isolates unrelated failures", async () => {
    let active = 0;
    let maximum = 0;
    const release: Array<() => void> = [];
    const refreshYouTube = vi.fn(async (target: Extract<MusicSourceRefreshTarget, { kind: "youtube-playlist" }>) => {
      active += 1;
      maximum = Math.max(maximum, active);
      await new Promise<void>((resolve) => release.push(resolve));
      active -= 1;
      if (target.collectionId === "youtube-fail") throw new Error("Private playlist");
    });
    const controller = createMusicSourceRefreshController(refreshYouTube, {}, 2);
    const targets = ["youtube-a", "youtube-fail", "youtube-c"].map((collectionId) => ({
      collectionId,
      kind: "youtube-playlist" as const,
      name: collectionId,
    }));
    const running = controller.run(controller.prepare(targets), { allowNetwork: true });
    await vi.waitFor(() => expect(active).toBe(2));
    release.shift()?.();
    release.shift()?.();
    await vi.waitFor(() => expect(release.length).toBe(1));
    release.shift()?.();
    const statuses = await running;
    expect(maximum).toBe(2);
    expect(statuses.find((status) => status.collectionId === "youtube-fail")?.state).toBe("failed");
    expect(statuses.filter((status) => status.state === "completed")).toHaveLength(2);
  });

  it("polls only an explicitly started local job and can cancel it", async () => {
    const getProgress = vi.fn(async () => progress("running", "local-1"));
    const cancelLocal = vi.fn(async () => progress("cancelled", "local-1"));
    let releaseWait: (() => void) | null = null;
    const controller = createMusicSourceRefreshController(vi.fn(async () => undefined), {
      startLocal: vi.fn(async () => progress("running", "local-1")),
      getProgress,
      cancelLocal,
      wait: vi.fn((_ms, signal) => new Promise<void>((resolve, reject) => {
        releaseWait = resolve;
        signal.addEventListener("abort", () => reject(new DOMException("cancelled", "AbortError")), { once: true });
      })),
      now: () => 50,
    });
    expect(getProgress).not.toHaveBeenCalled();
    const running = controller.run(controller.prepare([local("local-1")]), { allowNetwork: false });
    await vi.waitFor(() => expect(releaseWait).not.toBeNull());
    await controller.cancel("local-1");
    await running;
    expect(cancelLocal).toHaveBeenCalledWith("local-1", 50);
    expect(controller.statuses()[0]?.state).toBe("cancelled");
  });
});
