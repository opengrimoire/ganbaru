// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as chatApi from "$lib/api/chat";
import ChatImageGallery from "./ChatImageGallery.svelte";

const images = [
  { id: "image:slow", displayName: "slow.png", byteSize: 10 },
  { id: "image:ready", displayName: "ready.png", byteSize: 20 },
];

async function settle(): Promise<void> {
  for (let index = 0; index < 4; index += 1) {
    await Promise.resolve();
    await tick();
  }
}

describe("ChatImageGallery", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    document.body.innerHTML = "";
    component = undefined;
    target = undefined;
    vi.restoreAllMocks();
  });

  it("does not open a slow image over a later choice or after that preview closes", async () => {
    let resolveSlow: ((url: string) => void) | undefined;
    let slowReads = 0;
    vi.spyOn(chatApi, "chatAttachmentDataUrl").mockImplementation((imageId: string) => {
      if (imageId === "image:ready") return Promise.resolve("data:image/png;base64,cmVhZHk=");
      slowReads += 1;
      if (slowReads === 1) return Promise.reject(new Error("not synced yet"));
      return new Promise<string>((resolve) => { resolveSlow = resolve; });
    });
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ChatImageGallery, { target, props: { images } });
    await settle();

    const tiles = target.querySelectorAll<HTMLButtonElement>(".attachment-preview");
    tiles[0]?.click();
    await settle();
    tiles[1]?.click();
    await settle();
    expect(document.querySelector(".chat-image-dialog")?.getAttribute("aria-label")).toContain("ready.png");

    document.querySelector<HTMLButtonElement>(".chat-image-backdrop")?.click();
    await settle();
    expect(document.querySelector(".chat-image-dialog")).toBeNull();

    resolveSlow?.("data:image/png;base64,c2xvdw==");
    await settle();
    expect(document.querySelector(".chat-image-dialog")).toBeNull();
  });
});
