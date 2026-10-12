import { describe, expect, it } from "vitest";
import { projectMusicBuilderDockItems } from "./navigation";

describe("music builder navigation dock", () => {
  it("projects all destinations with playlist details activating Playlists", () => {
    const items = projectMusicBuilderDockItems({ kind: "playlist", playlistId: "focus" }, 12);
    expect(items.map((item) => item.kind)).toEqual([
      "review", "playlists", "sources", "soundscapes",
    ]);
    expect(items.find((item) => item.active)?.kind).toBe("playlists");
    expect(items.map((item) => item.shortcut)).toEqual(["1", "2", "3", "4"]);
  });

  it("shows a badge only when Review has pending tracks", () => {
    const items = projectMusicBuilderDockItems({ kind: "sources" }, 0);
    expect(items.find((item) => item.kind === "review")?.badge).toBeNull();
    expect(items.every((item) => item.badge === null)).toBe(true);
  });

  it("omits soundscapes when the platform has no soundscape engine", () => {
    const items = projectMusicBuilderDockItems({ kind: "playlists" }, 0, false);

    expect(items.map((item) => item.kind)).toEqual(["review", "playlists", "sources"]);
  });
});
