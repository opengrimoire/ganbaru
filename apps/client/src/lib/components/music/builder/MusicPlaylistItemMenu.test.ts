// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MusicItemListEntry } from "$lib/music/library/contracts";
import { musicSnoozeEndsAt } from "$lib/music/session/snooze";

const api = vi.hoisted(() => ({ getMusicInspectorDetail: vi.fn() }));

vi.mock("$lib/api/music-library", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/music-library")>(),
  ...api,
}));

const item: MusicItemListEntry = {
  id: "song", identityKey: "local:song", sourceKind: "local-file", mediaKind: "audio",
  title: "Song", artist: "Artist", album: "Album", localRootId: "root", relativePath: "song.flac",
  sourceCollectionIds: [], originalArtworkIdentity: null, artworkOverride: null, durationMs: null,
  availability: "available", reviewState: "reviewed", discoveredAtMs: 1, updatedAtMs: 1, version: 1,
  playlistCount: 1, activeSnoozeCount: 1, lastPlayedAtMs: null, playCount: 0,
  membershipId: "membership", membershipPosition: 0, membershipWeight: "normal",
  membershipEnabled: true, membershipVersion: 1,
};

describe("MusicPlaylistItemMenu Snooze", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    target = null;
    component = null;
    vi.clearAllMocks();
  });

  it("shows the active duration without an indefinite choice and allows selecting it again", async () => {
    const startsAtMs = Date.now() - 1_000;
    api.getMusicInspectorDetail.mockResolvedValue({
      memberships: [{ id: "membership", playlistId: "playlist" }],
      snoozes: [{ id: "snooze", itemId: "song", scope: "playlist", playlistId: "playlist",
        startsAtMs, endsAtMs: musicSnoozeEndsAt("week", startsAtMs, Intl.DateTimeFormat().resolvedOptions().timeZone),
        reason: "", createdAtMs: startsAtMs }],
    });
    const onSnooze = vi.fn().mockResolvedValue(undefined);
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicPlaylistItemMenu } = await import("./MusicPlaylistItemMenu.svelte");
    component = mount(MusicPlaylistItemMenu, { target, props: {
      item, onShowLocation: vi.fn().mockResolvedValue(undefined), onSnooze,
    } });
    await tick();

    target.querySelector<HTMLButtonElement>(".menu-trigger")?.click();
    await tick();
    [...document.querySelectorAll<HTMLButtonElement>(".panel-row")]
      .find((button) => button.textContent?.includes("Snooze"))?.click();
    await vi.waitFor(() => expect(document.querySelector(".option-row[aria-pressed='true']")?.textContent).toBe("1 week"));
    expect([...document.querySelectorAll<HTMLButtonElement>(".option-row")].map((button) => button.textContent))
      .toEqual(["1 day", "1 week", "1 month"]);
    document.querySelector<HTMLButtonElement>(".option-row[aria-pressed='true']")?.click();
    await vi.waitFor(() => expect(onSnooze).toHaveBeenCalledWith(item, "week", false));
  });
});
