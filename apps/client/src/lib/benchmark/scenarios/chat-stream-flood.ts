/**
 * Chat stream flood benchmark. Opens the seeded empty thread, then has the backend replay a
 * streaming turn into it through the production ingestor: a 2 MiB command output in 4 KiB deltas
 * plus 2000 assistant text deltas. The webview forwards each change notification to the Chat store
 * the same way the Chat workspace does, so the timeline refreshes as it would in a live run.
 */
import { invoke } from "@tauri-apps/api/core";
import { parseChatChangeNotification } from "$lib/chat/validation";
import { observeTimelineSinceReads } from "$lib/chat/timeline/read-observer";
import { listenForChatChanges } from "$lib/chat/workspace/local-execution-ui";
import { getChat } from "$lib/stores/chat.svelte";
import { requireScenarioMetadata } from "../registry";
import type {
  BenchmarkDatasetProfile,
  BenchmarkMetric,
  BenchmarkScenario,
  BenchmarkScenarioContext,
  BenchmarkSeedHandle,
} from "../types";
import { waitForMs } from "./calendar-utils";
import { frameBudgetMetrics, startFrameSampler, streamFloodMetrics } from "./chat-metrics";
import {
  chatBenchmarkThreadId,
  mountBenchmarkTimeline,
  requireChatTimelineDataset,
  seedChatTimelineDataset,
  type BenchmarkTimelineHost,
} from "./chat-utils";
import { throwIfAborted } from "./operation-utils";

/** Quiet time without refresh reads after which the timeline counts as caught up. */
const SETTLE_QUIET_MS = 300;
/** Longest wait for the timeline to catch up after the replay returns. */
const SETTLE_TIMEOUT_MS = 5_000;
const SETTLE_POLL_MS = 50;

interface ChatStreamReplaySummary {
  emittedNotifications: number;
  ingestedEvents: number;
  durationMs: number;
}

const metadata = requireScenarioMetadata("chat-stream-flood");
let mounted: { host: BenchmarkTimelineHost; threadId: string } | null = null;

async function waitForQuietReads(lastReadAt: () => number, signal: AbortSignal): Promise<void> {
  const deadline = performance.now() + SETTLE_TIMEOUT_MS;
  while (performance.now() - lastReadAt() < SETTLE_QUIET_MS && performance.now() < deadline) {
    await waitForMs(SETTLE_POLL_MS, signal);
  }
}

export const chatStreamFloodScenario: BenchmarkScenario = {
  ...metadata,

  async setup(_context: BenchmarkScenarioContext): Promise<void> {
    const threadId = chatBenchmarkThreadId(requireChatTimelineDataset(metadata.defaultDataset));
    mounted = { host: await mountBenchmarkTimeline(threadId), threadId };
  },

  async runWorkload(signal: AbortSignal): Promise<BenchmarkMetric[]> {
    const target = mounted;
    if (!target) throw new Error("Chat timeline is not mounted; cannot run the stream flood benchmark");
    mounted = null;
    const chat = getChat();
    const readBytes: number[] = [];
    let lastReadAt = performance.now();
    let notifications = 0;
    const stopObserving = observeTimelineSinceReads((sample) => {
      readBytes.push(sample.bytes);
      lastReadAt = performance.now();
    });
    const stopListening = await listenForChatChanges((payload) => {
      try {
        const change = parseChatChangeNotification(payload);
        if (change.threadId !== target.threadId) return;
        notifications += 1;
        void chat.handleNativeChange(change.threadId, change).catch(() => undefined);
      } catch (error: unknown) {
        console.error("Invalid Chat change notification", error);
      }
    });
    const sampler = startFrameSampler();
    let durations: number[] = [];
    try {
      throwIfAborted(signal);
      const summary = await invoke<ChatStreamReplaySummary>("benchmark_replay_chat_stream", {
        threadId: target.threadId,
      });
      await waitForQuietReads(() => lastReadAt, signal);
      durations = sampler.stop();
      return [
        ...streamFloodMetrics({
          notifications,
          emittedNotifications: summary.emittedNotifications,
          replayMs: summary.durationMs,
          refreshReadBytes: readBytes,
        }),
        ...frameBudgetMetrics("stream flood", durations),
        {
          label: "stream flood events ingested",
          unit: "count",
          value: summary.ingestedEvents,
          details: { timelineRows: chat.timelineItems.length },
        },
      ];
    } finally {
      sampler.stop();
      stopListening();
      stopObserving();
      await target.host.dispose();
    }
  },

  async seed(dataset: BenchmarkDatasetProfile): Promise<BenchmarkSeedHandle> {
    return seedChatTimelineDataset(dataset);
  },

  async cleanup(_seedHandle: BenchmarkSeedHandle): Promise<void> {
    // The isolated benchmark DB is deleted after the run.
  },
};
