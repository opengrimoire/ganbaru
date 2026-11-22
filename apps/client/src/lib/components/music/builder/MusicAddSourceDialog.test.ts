// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MusicSourcesController } from "$lib/music/sources/controller.svelte";
import type { MusicYouTubeSourcePreview } from "$lib/music/sources/youtube-source-resolver";
import MusicAddSourceDialog from "./MusicAddSourceDialog.svelte";

/** Build a single video preview with the given title. */
function videoPreview(videoId: string, title: string): MusicYouTubeSourcePreview {
  return {
    kind: "youtube-video",
    videoId,
    playlistId: null,
    title,
    channel: "Channel",
    durationMs: 1_000,
    videoIds: [videoId],
    videos: [],
    metadataTruncated: false,
    duplicateCount: 0,
  };
}

describe("MusicAddSourceDialog", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  it("ignores a link preview that resolves after the link was edited", async () => {
    target = document.createElement("div");
    document.body.append(target);
    let finishResolution: (preview: MusicYouTubeSourcePreview | null) => void = () => undefined;
    const controller = {
      busy: false,
      resolving: false,
      resolutionError: null,
      cancelResolution: vi.fn(),
      parseYouTubeInput: vi.fn(() => ({ source: { kind: "youtube-video", videoId: "old" }, error: null })),
      resolveYouTube: vi.fn(() => new Promise<MusicYouTubeSourcePreview | null>((resolve) => { finishResolution = resolve; })),
    } as unknown as MusicSourcesController;
    component = mount(MusicAddSourceDialog, {
      target,
      props: { controller, kind: "youtube", onClose: vi.fn(), onSaved: vi.fn() },
    });
    await tick();

    const dialog = document.body.querySelector<HTMLElement>("[role='dialog']");
    const input = dialog?.querySelector<HTMLInputElement>(".source-input");
    if (!input) throw new Error("missing link input");
    input.value = "https://www.youtube.com/watch?v=old";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    const previewButton = [...(dialog?.querySelectorAll<HTMLButtonElement>("button") ?? [])]
      .find((button) => button.textContent?.includes("Preview"));
    previewButton?.click();
    await tick();

    input.value = "https://www.youtube.com/watch?v=new";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(controller.cancelResolution).toHaveBeenCalled();

    finishResolution(videoPreview("old", "Old video"));
    await tick();
    await tick();

    expect(dialog?.textContent).not.toContain("Old video");
    expect(dialog?.textContent).not.toContain("Add to library");
  });
});
