// @vitest-environment jsdom

import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ChatChangedFilesSummary from "./ChatChangedFilesSummary.svelte";

describe("ChatChangedFilesSummary", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.restoreAllMocks();
  });

  it("routes a diff request to the provider session that produced the files", () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatChangedFilesSummary, {
      target,
      props: {
        turnId: "turn-older",
        sourceThreadId: "thread-older",
        sourceWorkingFolderId: "folder-older",
        files: [{
          relativePath: "hello.py",
          previousRelativePath: null,
          status: "added",
          additions: 1,
          deletions: 0,
          binary: false,
          providerReported: true,
          gitObserved: true,
        }],
      },
    });
    const listener = vi.fn();
    window.addEventListener("ganbaru-ai:chat-open-review", listener, { once: true });

    target.querySelector<HTMLButtonElement>(".changed-files-header")?.click();

    const event = listener.mock.calls[0]?.[0];
    expect(event).toBeInstanceOf(CustomEvent);
    expect((event as CustomEvent).detail).toEqual({
      source: { kind: "provider_turn", turnId: "turn-older" },
      relativePath: "hello.py",
      sourceThreadId: "thread-older",
      sourceWorkingFolderId: "folder-older",
    });
  });
});
