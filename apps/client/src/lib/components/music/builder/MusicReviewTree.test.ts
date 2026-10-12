// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MusicItemListEntry } from "$lib/music/library/contracts";
import MusicReviewTree from "./MusicReviewTree.svelte";

function item(id: string, title: string, relativePath: string): MusicItemListEntry {
  return {
    id, identityKey: id, sourceKind: "local-file", mediaKind: "audio", title, artist: "", album: "",
    localRootId: "root-1", relativePath, sourceCollectionIds: ["source-1"], originalArtworkIdentity: null, artworkOverride: null, durationMs: null, availability: "available", reviewState: "unreviewed",
    discoveredAtMs: 1, updatedAtMs: 1, version: 1, playlistCount: 0, activeSnoozeCount: 0,
    lastPlayedAtMs: null, playCount: 0, membershipId: null, membershipPosition: null,
    membershipWeight: null, membershipEnabled: null, membershipVersion: null,
  };
}

describe("MusicReviewTree", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
  });

  it("selects a complete folder as the active review selection", async () => {
    const onViewStateChange = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MusicReviewTree, {
      target,
      props: {
        items: [item("one", "One", "Album/one.flac"), item("two", "Two", "Album/two.flac")],
        totalCount: 2,
        activeItemId: "one",
        onActivate: vi.fn(),
        onViewStateChange,
      },
    });
    await tick();

    const album = target.querySelector<HTMLInputElement>('input[aria-label="Select all 2 tracks in Album"]');
    expect(album).not.toBeNull();
    album?.click();
    await tick();
    expect(onViewStateChange).toHaveBeenLastCalledWith(expect.objectContaining({
      selectedItemIds: ["one", "two"],
    }));
    expect(target.textContent).not.toContain("Add 2 selected to playlists");
  });

  it("toggles ignored tracks beside refresh and marks ignored rows with an eye", async () => {
    const onShowIgnoredChange = vi.fn();
    const ignored = item("ignored", "Ignored", "Album/ignored.flac");
    ignored.reviewState = "ignored";
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MusicReviewTree, {
      target,
      props: {
        items: [ignored],
        totalCount: 1,
        activeItemId: null,
        onActivate: vi.fn(),
        showIgnored: true,
        onShowIgnoredChange,
      },
    });
    await tick();

    const ignoredToggle = target.querySelector<HTMLButtonElement>('button[aria-label="Hide ignored tracks"]');
    const refresh = target.querySelector<HTMLButtonElement>('button[aria-label="Refresh local folders"]');
    expect(ignoredToggle?.nextElementSibling).toBe(refresh);
    expect(ignoredToggle?.getAttribute("aria-pressed")).toBe("true");
    expect(ignoredToggle?.querySelector(".lucide-eye-off")).not.toBeNull();
    expect(target.querySelector('[data-review-state="ignored"]')).not.toBeNull();
    ignoredToggle?.click();

    expect(onShowIgnoredChange).toHaveBeenCalledWith(false);
  });

  it("propagates folder selection downward without marking ancestors", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MusicReviewTree, {
      target,
      props: {
        items: [item("one", "One", "Album/one.flac"), item("two", "Two", "Album/two.flac")],
        totalCount: 2,
        activeItemId: null,
        onActivate: vi.fn(),
      },
    });
    await tick();

    const music = target.querySelector<HTMLInputElement>('input[aria-label="Select all 2 tracks in Music"]');
    const album = target.querySelector<HTMLInputElement>('input[aria-label="Select all 2 tracks in Album"]');
    const one = target.querySelector<HTMLInputElement>('input[aria-label="Select One"]');
    expect(music).not.toBeNull();
    expect(album).not.toBeNull();
    expect(one).not.toBeNull();

    one?.click();
    await tick();
    expect(one?.checked).toBe(true);
    expect(album?.checked).toBe(false);
    expect(album?.indeterminate).toBe(false);
    expect(music?.checked).toBe(false);
    expect(music?.indeterminate).toBe(false);

    album?.click();
    await tick();
    expect(album?.checked).toBe(true);
    expect(music?.checked).toBe(false);
    expect(music?.indeterminate).toBe(false);
  });

  it("opens every nested folder when Review first appears", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MusicReviewTree, {
      target,
      props: {
        items: [
          item("theme", "Theme", "Games/Nier/Disc 1/theme.flac"),
          item("ending", "Ending", "Games/Nier/ending.flac"),
        ],
        totalCount: 2,
        activeItemId: null,
        onActivate: vi.fn(),
      },
    });
    expect(target.textContent).toContain("Theme");
    expect(target.textContent).toContain("Ending");
    await tick();

    expect(
      [...target.querySelectorAll<HTMLButtonElement>("button[aria-expanded]")]
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual([
      "Collapse Music",
      "Collapse Games",
      "Collapse Nier",
      "Collapse Disc 1",
    ]);
  });

  it("shows every search match with folder context and clears with Escape", async () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MusicReviewTree, {
      target,
      props: {
        items: [
          item("morning", "Morning Theme", "Albums/Morning/theme.flac"),
          item("night", "Night Theme", "Albums/Night/theme.flac"),
          item("ending", "Ending", "Albums/Ending/ending.flac"),
        ],
        totalCount: 3,
        activeItemId: null,
        onActivate: vi.fn(),
      },
    });
    await tick();

    const search = target.querySelector<HTMLInputElement>('input[aria-label="Search folders and tracks"]');
    expect(search).not.toBeNull();
    if (!search) return;
    search.value = "theme";
    search.dispatchEvent(new InputEvent("input", { bubbles: true }));
    await tick();

    expect(target.textContent).toContain("Morning Theme");
    expect(target.textContent).toContain("Night Theme");
    expect(target.textContent).not.toContain("Ending");
    expect(target.querySelector('[aria-label="2 matches"]')).not.toBeNull();

    search.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(search.value).toBe("");
    expect(target.textContent).toContain("Ending");
  });

  it("requests a local-folder refresh from the search toolbar", async () => {
    const onRefresh = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MusicReviewTree, {
      target,
      props: {
        items: [item("one", "One", "Album/one.flac")],
        totalCount: 1,
        activeItemId: null,
        onActivate: vi.fn(),
        onRefresh,
      },
    });
    await tick();

    target.querySelector<HTMLButtonElement>('button[aria-label="Refresh local folders"]')?.click();
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it("opens attention work from one quiet entry and marks affected tracks", async () => {
    const onOpenIssues = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MusicReviewTree, {
      target,
      props: {
        items: [item("one", "One", "Album/one.flac"), item("two", "Two", "Album/two.flac")],
        totalCount: 2,
        activeItemId: "one",
        onActivate: vi.fn(),
        issueCount: 1,
        issueItemIds: new Set(["two"]),
        onOpenIssues,
      },
    });
    await tick();

    expect(target.querySelector('[aria-label="Two needs attention"]')).not.toBeNull();
    target.querySelector<HTMLButtonElement>('button[aria-label="1 item needs attention"]')?.click();
    expect(onOpenIssues).toHaveBeenCalledOnce();
  });
});
