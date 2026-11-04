import { describe, expect, it, vi } from "vitest";
import { listenForMusicReplication, touchesMusic, type MusicSyncAppliedListen } from "./replication";

describe("music replication notices", () => {
  it("recognizes Music tables and ignores other domains", () => {
    expect(touchesMusic(["quick_notes", "music_playlist_memberships"])).toBe(true);
    expect(touchesMusic(["quick_notes", "themes"])).toBe(false);
    expect(touchesMusic([])).toBe(false);
  });

  it("runs the change handler only for applied Music tables", async () => {
    let applied: (tables: string[]) => void = () => undefined;
    const listen: MusicSyncAppliedListen = async (handler) => {
      applied = handler;
      return () => undefined;
    };
    const onChange = vi.fn();
    listenForMusicReplication(listen, onChange);
    await Promise.resolve();
    applied(["quick_notes"]);
    expect(onChange).not.toHaveBeenCalled();
    applied(["music_library_items", "quick_notes"]);
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("unsubscribes even when stopped before the subscription resolves", async () => {
    const unsubscribe = vi.fn();
    let resolve: (stop: () => void) => void = () => undefined;
    const listen: MusicSyncAppliedListen = () => new Promise((done) => { resolve = done; });
    const stop = listenForMusicReplication(listen, vi.fn());
    stop();
    resolve(unsubscribe);
    await Promise.resolve();
    expect(unsubscribe).toHaveBeenCalledTimes(1);
  });

  it("stops delivering changes after stop", async () => {
    let applied: (tables: string[]) => void = () => undefined;
    const unsubscribe = vi.fn();
    const listen: MusicSyncAppliedListen = async (handler) => {
      applied = handler;
      return unsubscribe;
    };
    const onChange = vi.fn();
    const stop = listenForMusicReplication(listen, onChange);
    await Promise.resolve();
    await Promise.resolve();
    stop();
    applied(["music_playlists"]);
    expect(onChange).not.toHaveBeenCalled();
    expect(unsubscribe).toHaveBeenCalledTimes(1);
  });
});
