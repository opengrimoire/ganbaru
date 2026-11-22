/**
 * Chat timeline scroll benchmark. Opens the seeded 5000-row thread at its newest page and scrolls
 * the timeline toward its start a fixed distance every frame, so the measured frames include
 * row virtualization, row measurement, and the older pages the timeline loads on the way up.
 */
import { getChat } from "$lib/stores/chat.svelte";
import { requireScenarioMetadata } from "../registry";
import type {
  BenchmarkDatasetProfile,
  BenchmarkMetric,
  BenchmarkScenario,
  BenchmarkScenarioContext,
  BenchmarkSeedHandle,
} from "../types";
import { frameBudgetMetrics, startFrameSampler } from "./chat-metrics";
import {
  chatBenchmarkThreadId,
  mountBenchmarkTimeline,
  requireChatTimelineDataset,
  seedChatTimelineDataset,
  timelineReachedStart,
  type BenchmarkTimelineHost,
} from "./chat-utils";
import { throwIfAborted } from "./operation-utils";

/** Distance scrolled toward the start each frame, about a fast flick on a trackpad. */
const SCROLL_STEP_PX = 240;

const metadata = requireScenarioMetadata("chat-timeline-scroll");
let mounted: BenchmarkTimelineHost | null = null;

function nextFrame(): Promise<number> {
  return new Promise((resolve) => requestAnimationFrame(resolve));
}

export const chatTimelineScrollScenario: BenchmarkScenario = {
  ...metadata,

  async setup(_context: BenchmarkScenarioContext): Promise<void> {
    const dataset = requireChatTimelineDataset(metadata.defaultDataset);
    mounted = await mountBenchmarkTimeline(chatBenchmarkThreadId(dataset));
  },

  async runWorkload(signal: AbortSignal): Promise<BenchmarkMetric[]> {
    const host = mounted;
    if (!host) throw new Error("Chat timeline is not mounted; cannot run the scroll benchmark");
    const startedAt = performance.now();
    const sampler = startFrameSampler();
    let durations: number[] = [];
    try {
      while (performance.now() - startedAt < metadata.workload.durationMs) {
        throwIfAborted(signal);
        if (host.scroller.scrollTop <= 0 && timelineReachedStart()) break;
        host.scroller.scrollTop = Math.max(0, host.scroller.scrollTop - SCROLL_STEP_PX);
        await nextFrame();
      }
    } finally {
      durations = sampler.stop();
    }
    const elapsedMs = performance.now() - startedAt;
    const rows = getChat().timelineItems.length;
    try {
      return [
        ...frameBudgetMetrics("timeline scroll", durations),
        {
          label: "timeline rows loaded",
          unit: "count",
          value: rows,
          details: { reachedStart: timelineReachedStart() ? 1 : 0, elapsedMs: Math.round(elapsedMs) },
        },
      ];
    } finally {
      mounted = null;
      await host.dispose();
    }
  },

  async seed(
    dataset: BenchmarkDatasetProfile,
    _context: BenchmarkScenarioContext,
    signal: AbortSignal,
  ): Promise<BenchmarkSeedHandle> {
    return seedChatTimelineDataset(dataset, signal);
  },

  async cleanup(_seedHandle: BenchmarkSeedHandle): Promise<void> {
    // The isolated benchmark DB is deleted after the run.
  },
};
