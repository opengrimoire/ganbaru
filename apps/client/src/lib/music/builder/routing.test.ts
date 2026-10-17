import { describe, expect, it } from "vitest";
import {
  backMusicBuilderRoute,
  initialMusicBuilderRoute,
  musicBuilderDestinationForKey,
  musicBuilderDestinationShortcut,
  pushMusicBuilderRoute,
  repairMusicBuilderHistory,
} from "./routing";

const context = {
  playlistIds: new Set(["focus"]),
  itemIds: new Set(["track-1"]),
};

describe("music builder routing", () => {
  it("maps primary destinations to the visible 1 through 4 shortcuts", () => {
    expect(["1", "2", "3", "4"].map(musicBuilderDestinationForKey)).toEqual([
      { kind: "review" },
      { kind: "playlists" },
      { kind: "sources" },
      { kind: "soundscapes" },
    ]);
    expect(musicBuilderDestinationForKey("0")).toBeNull();
    expect(musicBuilderDestinationForKey("Digit1")).toBeNull();
    expect(musicBuilderDestinationShortcut("playlist")).toBe("2");
    expect(musicBuilderDestinationShortcut("soundscapes")).toBe("4");
  });

  it("opens Review first only when work is waiting", () => {
    expect(initialMusicBuilderRoute(3, { kind: "sources" }, context).current.destination.kind).toBe("sources");
    expect(initialMusicBuilderRoute(3, null, context).current.destination.kind).toBe("review");
    expect(initialMusicBuilderRoute(0, null, context).current.destination.kind).toBe("playlists");
  });

  it("supports deep inspector routes and browser-like Back", () => {
    let history = initialMusicBuilderRoute(0, null, context);
    history = pushMusicBuilderRoute(history, {
      destination: { kind: "playlist", playlistId: "focus" },
      inspectorItemId: "track-1",
    }, context);
    expect(history.current.inspectorItemId).toBe("track-1");
    const back = backMusicBuilderRoute(history, context);
    expect(back?.current.destination.kind).toBe("playlists");
    expect(backMusicBuilderRoute(back!, context)).toBeNull();
  });

  it("repairs deleted playlists and items without corrupting history", () => {
    let history = initialMusicBuilderRoute(0, null, context);
    history = pushMusicBuilderRoute(history, {
      destination: { kind: "playlist", playlistId: "focus" },
      inspectorItemId: "track-1",
    }, context);
    const repaired = repairMusicBuilderHistory(history, {
      playlistIds: new Set(),
      itemIds: new Set(),
    });
    expect(repaired.current).toEqual({ destination: { kind: "playlists" }, inspectorItemId: null });
  });
});
