// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MusicItemListEntry } from "$lib/music/library/contracts";
import MusicVirtualItemList from "./MusicVirtualItemList.svelte";

const item: MusicItemListEntry = {
  id: "item-1",
  identityKey: "local:item-1",
  sourceKind: "local-file",
  mediaKind: "audio",
  title: "Quiet morning",
  artist: "Composer",
  album: "Soundtrack",
  localRootId: "root-1",
  relativePath: "Soundtrack/Quiet morning.flac",
  sourceCollectionIds: ["source-1"],
  originalArtworkIdentity: null,
  artworkOverride: null,
  durationMs: 120_000,
  availability: "available",
  reviewState: "reviewed",
  discoveredAtMs: 1,
  updatedAtMs: 1,
  version: 1,
  playlistCount: 1,
  activeSnoozeCount: 0,
  lastPlayedAtMs: null,
  playCount: 0,
  membershipId: "membership-1",
  membershipPosition: 0,
  membershipWeight: "normal",
  membershipEnabled: true,
  membershipVersion: 1,
};

describe("MusicVirtualItemList", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", class {
      observe(): void {}
      disconnect(): void {}
    });
    vi.stubGlobal("matchMedia", vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })));
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    vi.unstubAllGlobals();
    component = null;
    target = null;
  });

  it("plays from the row and keeps track actions in the overflow menu", async () => {
    const onTogglePlayback = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MusicVirtualItemList, {
      target,
      props: {
        items: [item],
        bindings: [{ rootId: "root-1", folderPath: "/Music", status: "available" }],
        playlistName: "Work (focus)",
        onTogglePlayback,
        onShowLocation: vi.fn(async () => undefined),
        onSnooze: vi.fn(async () => undefined),
        onRemoveSnooze: vi.fn(async () => undefined),
        onWeight: vi.fn(async () => undefined),
        onRemove: vi.fn(async () => undefined),
      },
    });
    await tick();

    expect(target.querySelector('[role="list"]')).not.toBeNull();
    const row = target.querySelector<HTMLElement>('[role="listitem"]');
    expect(row?.getAttribute("aria-posinset")).toBe("1");
    expect(row?.getAttribute("aria-setsize")).toBe("1");
    expect(target.querySelector('[role="option"]')).toBeNull();
    const playButton = target.querySelector<HTMLButtonElement>('button[aria-label="Play"]');
    expect(playButton).not.toBeNull();
    expect(playButton?.dataset.appTooltipDisabled).toBe("true");
    playButton?.click();
    expect(onTogglePlayback).toHaveBeenCalledWith(item);

    expect(target.querySelector('[draggable="true"]')).toBeNull();
    const actionButton = target.querySelector<HTMLButtonElement>('[aria-haspopup="dialog"]');
    expect(actionButton).not.toBeNull();
    actionButton?.click();
    await tick();
    expect(document.body.querySelectorAll('[role="dialog"]')).toHaveLength(1);
    expect(document.body.textContent).toContain("Remove from Work (focus)");

    const detailsButton = Array.from(document.body.querySelectorAll<HTMLButtonElement>("button"))
      .find((button) => button.textContent?.includes("Details"));
    detailsButton?.click();
    await tick();
    expect(document.body.querySelectorAll('[role="dialog"]')).toHaveLength(1);
    expect(document.body.textContent).toContain("Soundtrack");
    expect(target.textContent).not.toContain("Reviewed");
  });

  it("removes a Snooze beside the title without playing the row", async () => {
    const onTogglePlayback = vi.fn();
    const onRemoveSnooze = vi.fn(async () => undefined);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MusicVirtualItemList, { target, props: {
      items: [{ ...item, activeSnoozeCount: 1 }],
      bindings: [{ rootId: "root-1", folderPath: "/Music", status: "available" }],
      playlistName: "Work (focus)", onTogglePlayback,
      onShowLocation: vi.fn(async () => undefined), onSnooze: vi.fn(async () => undefined),
      onRemoveSnooze, onWeight: vi.fn(async () => undefined), onRemove: vi.fn(async () => undefined),
    } });
    await tick();

    const button = target.querySelector<HTMLButtonElement>('button[aria-label="Remove snooze"]');
    expect(button?.parentElement?.textContent).toContain("Quiet morning");
    button?.click();
    await vi.waitFor(() => expect(onRemoveSnooze).toHaveBeenCalledWith(expect.objectContaining({ id: "item-1" })));
    expect(onTogglePlayback).not.toHaveBeenCalled();
  });
});
