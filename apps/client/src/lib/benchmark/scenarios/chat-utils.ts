import { invoke } from "@tauri-apps/api/core";
import { mount, tick, unmount } from "svelte";
import * as chatApi from "$lib/api/chat";
import ChatTimeline from "$lib/components/chat/timeline/ChatTimeline.svelte";
import { getChat } from "$lib/stores/chat.svelte";
import {
  benchmarkDatasetId,
  type BenchmarkDatasetProfile,
  type BenchmarkSeedHandle,
  type ChatTimelineDatasetProfile,
} from "../types";
import { waitForFrames, waitForMs } from "./calendar-utils";

/**
 * Size of the benchmark timeline host, fixed so every run lays out the same rows whatever the
 * window size.
 */
export const CHAT_BENCHMARK_HOST_WIDTH_PX = 720;
export const CHAT_BENCHMARK_HOST_HEIGHT_PX = 900;
/** Longest wait for the first timeline page of a seeded thread. */
const TIMELINE_READY_TIMEOUT_MS = 15_000;
const TIMELINE_READY_POLL_MS = 50;

interface ChatTimelineSeedSummary {
  threadId: string;
  rowCount: number;
}

/** A Chat timeline mounted for a benchmark, with its scroll element. */
export interface BenchmarkTimelineHost {
  scroller: HTMLElement;
  /** Unmounts the timeline, removes the host, and clears the selected thread. */
  dispose(): Promise<void>;
}

/** Narrows a dataset to a Chat timeline profile. */
export function requireChatTimelineDataset(dataset: BenchmarkDatasetProfile): ChatTimelineDatasetProfile {
  if (dataset.kind !== "chat-timeline") {
    throw new Error(`Dataset ${benchmarkDatasetId(dataset)} is not a Chat timeline dataset`);
  }
  return dataset;
}

/** Thread ID a Chat dataset seeds; the backend accepts only this `benchmark-chat-` prefix. */
export function chatBenchmarkThreadId(dataset: ChatTimelineDatasetProfile): string {
  return `benchmark-${benchmarkDatasetId(dataset)}`;
}

/** Seeds one Chat timeline dataset into the benchmark database. */
export async function seedChatTimelineDataset(dataset: BenchmarkDatasetProfile): Promise<BenchmarkSeedHandle> {
  const chatDataset = requireChatTimelineDataset(dataset);
  const summary = await invoke<ChatTimelineSeedSummary>("benchmark_seed_chat_timeline", {
    request: { threadId: chatBenchmarkThreadId(chatDataset), shape: chatDataset.shape },
  });
  return {
    calendarId: null,
    chatThreadId: summary.threadId,
    eventCount: summary.rowCount,
    datasetId: benchmarkDatasetId(chatDataset),
    dataset: chatDataset,
  };
}

/**
 * Mounts the direct-thread timeline over the app in a fixed-size host and opens `threadId` in
 * the primary Chat store, then waits for its first page.
 *
 * The benchmark mounts the timeline alone because the Chat workspace reaches a direct thread only
 * through provider sessions the harness cannot start; the timeline reads the same store either way.
 */
export async function mountBenchmarkTimeline(threadId: string, signal?: AbortSignal): Promise<BenchmarkTimelineHost> {
  const chat = getChat();
  const host = document.createElement("div");
  host.dataset.benchmarkChatTimeline = "true";
  Object.assign(host.style, {
    position: "fixed",
    top: "0",
    left: "0",
    width: `${CHAT_BENCHMARK_HOST_WIDTH_PX}px`,
    height: `${CHAT_BENCHMARK_HOST_HEIGHT_PX}px`,
    zIndex: "2147483000",
    overflow: "hidden",
    background: "var(--background)",
  });
  document.body.append(host);
  const component = mount(ChatTimeline, { target: host });
  const dispose = async () => {
    await unmount(component);
    host.remove();
    chat.selectThread(null);
  };
  try {
    chat.selectThreadShell(await chatApi.readChatThreadShell(threadId));
    await waitForTimeline(threadId, signal);
    await tick();
    await waitForFrames(2);
    const scroller = host.querySelector<HTMLElement>(".chat-timeline-scroller");
    if (!scroller) throw new Error("Chat timeline scroller did not mount");
    return { scroller, dispose };
  } catch (error: unknown) {
    await dispose();
    throw error;
  }
}

async function waitForTimeline(threadId: string, signal?: AbortSignal): Promise<void> {
  const chat = getChat();
  const deadline = performance.now() + TIMELINE_READY_TIMEOUT_MS;
  const controller = signal ?? new AbortController().signal;
  while (chat.selectedThreadId !== threadId || chat.timelineLoading || chat.timelinePages.length === 0) {
    if (chat.timelineError) throw new Error(`Chat timeline failed to load: ${chat.timelineError}`);
    if (performance.now() > deadline) throw new Error("Chat timeline did not load in time");
    await waitForMs(TIMELINE_READY_POLL_MS, controller);
  }
}

/** Whether the selected thread's oldest page is loaded. */
export function timelineReachedStart(): boolean {
  const pages = getChat().timelinePages;
  return pages.length > 0 && pages[0].previousCursor === null;
}
