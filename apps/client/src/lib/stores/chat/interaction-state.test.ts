import { describe, expect, it } from "vitest";
import type { ChatInteractionStateRead } from "$lib/chat/contracts";
import {
  NO_INTERACTION,
  shownInteraction,
  transitionInteractionLoad,
  type InteractionLoad,
  type InteractionLoadEvent,
} from "./interaction-state";

function read(sessionId: string): ChatInteractionStateRead {
  return {
    sessionId,
    sessionState: "active",
    activeTurnId: null,
    capabilities: { entries: [] },
    pendingRequests: [],
    queuedFollowup: null,
    usage: null,
    accountStatus: null,
    rateLimitStatus: null,
    automaticCompactionReported: false,
  };
}

const first = read("session-a");
const second = read("session-b");
const shownA: InteractionLoad = { phase: "shown", threadId: "thread-a", interaction: first };
const loadingA: InteractionLoad = { phase: "loading", threadId: "thread-a", request: 2, shown: first };

describe("transitionInteractionLoad", () => {
  it("keeps the same thread's state on screen while it is reread", () => {
    expect(transitionInteractionLoad(shownA, { type: "started", threadId: "thread-a", request: 2 })).toEqual(loadingA);
  });

  it("never shows another thread's state while a new thread loads", () => {
    expect(transitionInteractionLoad(shownA, { type: "started", threadId: "thread-b", request: 2 }))
      .toEqual({ phase: "loading", threadId: "thread-b", request: 2, shown: null });
  });

  it("supersedes a read in flight with a newer one", () => {
    expect(transitionInteractionLoad(loadingA, { type: "started", threadId: "thread-a", request: 3 }))
      .toEqual({ ...loadingA, request: 3 });
  });

  it("shows the result of the read in flight", () => {
    expect(transitionInteractionLoad(loadingA, { type: "loaded", request: 2, interaction: second }))
      .toEqual({ phase: "shown", threadId: "thread-a", interaction: second });
  });

  it("restores the thread's previous state when its reread fails", () => {
    expect(transitionInteractionLoad(loadingA, { type: "failed", request: 2 })).toEqual(shownA);
  });

  it("shows nothing when a first read fails", () => {
    const loading: InteractionLoad = { phase: "loading", threadId: "thread-b", request: 2, shown: null };
    expect(transitionInteractionLoad(loading, { type: "failed", request: 2 })).toBe(NO_INTERACTION);
  });

  it("drops a read whose thread left the selection", () => {
    expect(transitionInteractionLoad(loadingA, { type: "abandoned", request: 2 })).toBe(NO_INTERACTION);
  });

  it.each<InteractionLoadEvent>([
    { type: "loaded", request: 1, interaction: second },
    { type: "failed", request: 1 },
    { type: "abandoned", request: 1 },
  ])("ignores a stale $type result", (event) => {
    expect(transitionInteractionLoad(loadingA, event)).toBe(loadingA);
  });

  it.each<InteractionLoadEvent>([
    { type: "loaded", request: 2, interaction: second },
    { type: "failed", request: 2 },
    { type: "abandoned", request: 2 },
  ])("ignores a $type result when nothing is loading", (event) => {
    expect(transitionInteractionLoad(shownA, event)).toBe(shownA);
    expect(transitionInteractionLoad(NO_INTERACTION, event)).toBe(NO_INTERACTION);
  });

  it("replaces the shown state and lets a read in flight still land", () => {
    const replaced = transitionInteractionLoad(loadingA, { type: "replaced", interaction: null });
    expect(replaced).toEqual({ ...loadingA, shown: null });
    expect(transitionInteractionLoad(replaced, { type: "loaded", request: 2, interaction: second }))
      .toEqual({ phase: "shown", threadId: "thread-a", interaction: second });
  });

  it("replaces a settled state directly", () => {
    expect(transitionInteractionLoad(shownA, { type: "replaced", interaction: second }))
      .toEqual({ phase: "shown", threadId: "thread-a", interaction: second });
    expect(transitionInteractionLoad(NO_INTERACTION, { type: "replaced", interaction: second }))
      .toEqual({ phase: "shown", threadId: null, interaction: second });
    expect(transitionInteractionLoad(shownA, { type: "replaced", interaction: null })).toBe(NO_INTERACTION);
  });

  it("clears both a settled state and a read in flight", () => {
    expect(transitionInteractionLoad(loadingA, { type: "cleared" })).toBe(NO_INTERACTION);
    expect(transitionInteractionLoad(shownA, { type: "cleared" })).toBe(NO_INTERACTION);
    expect(transitionInteractionLoad(NO_INTERACTION, { type: "cleared" })).toBe(NO_INTERACTION);
  });
});

describe("shownInteraction", () => {
  it("reads the state on screen in every phase", () => {
    expect(shownInteraction(NO_INTERACTION)).toBeNull();
    expect(shownInteraction(loadingA)).toBe(first);
    expect(shownInteraction(shownA)).toBe(first);
  });
});
