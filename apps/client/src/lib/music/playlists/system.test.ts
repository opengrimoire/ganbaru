import { describe, expect, it } from "vitest";
import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
import {
  isSystemMusicPlaylistId,
  orderMusicPlaylists,
  SYSTEM_MUSIC_PLAYLIST_IDS,
  systemMusicPlaylistName,
} from "./system";

describe("system music playlists", () => {
  it("localizes protected playlist names from stable ids", () => {
    const t = ((key: string) => key === "music.builder.defaultPlaylist.workFocus" ? "Trabajo (concentración)" : key) as Parameters<typeof systemMusicPlaylistName>[2];
    expect(systemMusicPlaylistName("playlist-default-work-focus", "Work (focus)", t)).toBe("Trabajo (concentración)");
    expect(systemMusicPlaylistName("custom", "Focus", t)).toBe("Focus");
    expect(isSystemMusicPlaylistId("playlist-default-work-focus")).toBe(true);
    expect(isSystemMusicPlaylistId("custom")).toBe(false);
  });

  it("keeps built-in playlists in their intended review order", () => {
    expect(SYSTEM_MUSIC_PLAYLIST_IDS).toEqual([
      "playlist-default-start-of-day",
      "playlist-default-work-focus",
      "playlist-default-work-ganbare",
      "playlist-default-break-calm",
      "playlist-default-break-active",
      "playlist-default-meditate",
      "playlist-default-exercise",
      "playlist-default-hygiene",
      "playlist-default-chores",
      "playlist-default-cooking",
      "playlist-default-commute",
    ]);
  });

  it("orders default and custom playlists by their persisted order keys", () => {
    const playlist = (id: string, name: string, orderKey: string) => ({ id, name, orderKey }) as MusicPlaylistSummary;
    const ordered = orderMusicPlaylists([
      playlist("custom-z", "Zen", "a4"),
      playlist("playlist-default-commute", "Commute", "a1"),
      playlist("custom-a", "Ambient", "a0"),
      playlist("playlist-default-start-of-day", "Start of the day!", "a2"),
      playlist("playlist-default-work-focus", "Work (focus)", "a3"),
    ]);

    expect(ordered.map(({ id }) => id)).toEqual([
      "custom-a",
      "playlist-default-commute",
      "playlist-default-start-of-day",
      "playlist-default-work-focus",
      "custom-z",
    ]);
  });

  it("compares order keys by code unit and breaks ties by id", () => {
    const playlist = (id: string, orderKey: string) => ({ id, name: id, orderKey }) as MusicPlaylistSummary;
    const ordered = orderMusicPlaylists([
      playlist("lower", "a0V"),
      playlist("upper", "a0B"),
      playlist("tie-b", "Zz"),
      playlist("tie-a", "Zz"),
    ]);

    expect(ordered.map(({ id }) => id)).toEqual(["tie-a", "tie-b", "upper", "lower"]);
  });
});
