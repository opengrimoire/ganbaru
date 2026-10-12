// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MusicIssue, MusicItemListEntry } from "$lib/music/library/contracts";
import MusicReviewIssuesPanel from "./MusicReviewIssuesPanel.svelte";

function item(): MusicItemListEntry {
  return {
    id: "track-1", identityKey: "track-1", sourceKind: "local-file", mediaKind: "audio",
    title: "Missing song", artist: "", album: "", localRootId: "root-1", relativePath: "Album/song.flac", sourceCollectionIds: ["source-1"],
    originalArtworkIdentity: null, artworkOverride: null, durationMs: null, availability: "missing",
    reviewState: "unreviewed", discoveredAtMs: 1, updatedAtMs: 1, version: 1, playlistCount: 0,
    activeSnoozeCount: 0, lastPlayedAtMs: null, playCount: 0, membershipId: null,
    membershipPosition: null, membershipWeight: null, membershipEnabled: null, membershipVersion: null,
  };
}

function issue(): MusicIssue {
  return {
    id: "issue-1", issueKind: "missing-local-file", itemId: "track-1", playlistId: null,
    collectionId: "source-1", rootId: "root-1", relativePath: "Album/song.flac",
    actionRequired: true, message: "The local media location is missing.", createdAtMs: 1,
  };
}

describe("MusicReviewIssuesPanel", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
  });

  it("drills from compact categories into actionable affected tracks", async () => {
    const onSelectGroup = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MusicReviewIssuesPanel, {
      target,
      props: {
        issues: [issue()],
        items: [item()],
        sources: [],
        selectedGroup: null,
        activeItemId: null,
        onSelectGroup,
        onBack: vi.fn(),
        onSelectIssue: vi.fn(),
        onRepair: vi.fn(),
        onRefresh: vi.fn(),
      },
    });
    await tick();

    [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.includes("Missing local files"))?.click();
    expect(onSelectGroup).toHaveBeenCalledWith("missing-local-file");
  });

  it("selects and repairs an issue without replacing the Review workspace", async () => {
    const onSelectIssue = vi.fn();
    const onRepair = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MusicReviewIssuesPanel, {
      target,
      props: {
        issues: [issue()],
        items: [item()],
        sources: [],
        selectedGroup: "missing-local-file",
        activeItemId: "track-1",
        onSelectGroup: vi.fn(),
        onBack: vi.fn(),
        onSelectIssue,
        onRepair,
        onRefresh: vi.fn(),
      },
    });
    await tick();

    [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.includes("Missing song"))?.click();
    [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.trim() === "Repair")?.click();

    expect(onSelectIssue).toHaveBeenCalledWith(issue());
    expect(onRepair).toHaveBeenCalledWith(issue());
  });
});
