// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { createMusicSourcesController } from "$lib/music/sources/controller.svelte";
import MusicDetectedFolderCard from "./MusicDetectedFolderCard.svelte";

describe("MusicDetectedFolderCard", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
  });

  it("shows the automatic preview and dismisses it without changing files", async () => {
    const controller = createMusicSourcesController();
    controller.detectedDefaultFolder = {
      folderPath: "/home/user/Music",
      tracks: [{ path: "/home/user/Music/focus.flac", title: "focus", artworkPath: null }],
      truncated: false,
    };
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MusicDetectedFolderCard, { target, props: { controller } });
    await tick();

    expect(target.textContent).toContain("Your Music folder is ready");
    expect(target.textContent).toContain("1 supported file");
    expect(target.textContent).toContain("/home/user/Music");
    const dismiss = [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.includes("Not now"));
    dismiss?.click();
    await tick();

    expect(controller.detectedDefaultFolder).toBeNull();
    expect(target.textContent).not.toContain("Your Music folder is ready");
  });
});
