// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type * as ReviewDiffRuntimeModule from "$lib/chat/review/diff-runtime";
import ChatPierreDiff from "./ChatPierreDiff.svelte";

const runtimeLoad = vi.hoisted(() => {
  const state = {
    created: 0,
    cleanedUp: 0,
    resolve: (_module: unknown) => {},
    promise: null as Promise<unknown> | null,
  };
  return state;
});

vi.mock("$lib/chat/review/diff-loader", () => ({
  loadChatReviewDiffRuntime: () => {
    runtimeLoad.promise ??= new Promise((resolve) => {
      runtimeLoad.resolve = resolve;
    });
    return runtimeLoad.promise;
  },
}));

/** Stands in for the enhanced renderer and counts the instances still holding the host. */
class FakeReviewDiffRuntime {
  constructor() {
    runtimeLoad.created += 1;
  }

  setItems(): [] {
    return [];
  }

  setOptions(): void {}

  scrollToFile(): void {}

  scrollToLine(): void {}

  clearSelection(): void {}

  cleanUp(): void {
    runtimeLoad.cleanedUp += 1;
  }
}

describe("ChatPierreDiff", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    });
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    runtimeLoad.created = 0;
    runtimeLoad.cleanedUp = 0;
    runtimeLoad.promise = null;
    vi.unstubAllGlobals();
  });

  it("keeps one enhanced renderer when the panel is hidden and shown again while the renderer loads", async () => {
    const props = $state({
      active: true,
      items: [],
      selectedFileId: null,
      diffStyle: "unified" as const,
      wrap: false,
      labels: { attach: "Attach", resolve: "Resolve", reopen: "Reopen", outdated: "Outdated" },
      fallbackLabel: "The enhanced diff is unavailable",
      rawLoadMoreLabel: "Load more",
      onSelection: () => {},
      onActiveFile: () => {},
      onAttachComment: () => {},
      onResolveComment: () => {},
    });
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatPierreDiff, { target, props });
    await tick();

    props.active = false;
    flushSync();
    props.active = true;
    flushSync();
    runtimeLoad.resolve({ ReviewDiffRuntime: FakeReviewDiffRuntime } as unknown as typeof ReviewDiffRuntimeModule);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(runtimeLoad.created - runtimeLoad.cleanedUp).toBe(1);
  });
});
