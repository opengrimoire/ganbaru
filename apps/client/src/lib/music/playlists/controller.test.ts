import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MusicPlaylist, MusicPlaylistDeleteImpact } from "$lib/music/library/contracts";
import type { MusicLibraryController } from "$lib/music/library/controller.svelte";

const mocks = vi.hoisted(() => ({
  getPlaylist: vi.fn<(playlistId: string) => Promise<MusicPlaylist>>(),
  deleteImpact: vi.fn<(playlistId: string) => Promise<MusicPlaylistDeleteImpact>>(),
}));

vi.mock("$lib/api/music-library", () => ({
  getMusicPlaylist: mocks.getPlaylist,
  getMusicPlaylistDeleteImpact: mocks.deleteImpact,
}));
vi.mock("$lib/stores/music-player.svelte", () => ({ getMusicPlayer: () => ({}) }));

const { MusicPlaylistController } = await import("./controller.svelte");

function playlist(id: string): MusicPlaylist {
  return { id, name: id, version: 1 } as MusicPlaylist;
}

function impact(membershipCount: number): MusicPlaylistDeleteImpact {
  return {
    membershipCount,
    projectFocusAssignmentCount: 0,
    projectBreakAssignmentCount: 0,
    calendarAssignmentCount: 0,
    contextAssignmentCount: 0,
    assignments: [],
  };
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function controller(): InstanceType<typeof MusicPlaylistController> {
  return new MusicPlaylistController({} as MusicLibraryController);
}

describe("Music playlist controller", () => {
  beforeEach(() => {
    mocks.getPlaylist.mockReset();
    mocks.deleteImpact.mockReset();
  });

  it("keeps the shown playlist when the view returns to it while another one loads", async () => {
    const subject = controller();
    mocks.getPlaylist.mockResolvedValueOnce(playlist("a"));
    await subject.load("a");
    const other = deferred<MusicPlaylist>();
    mocks.getPlaylist.mockReturnValueOnce(other.promise);

    const loadingOther = subject.load("b");
    expect(subject.busy).toBe(true);
    await expect(subject.load("a")).resolves.toBe(true);
    other.resolve(playlist("b"));
    await loadingOther;

    expect(subject.detail?.id).toBe("a");
    expect(subject.busy).toBe(false);
  });

  it("does not offer the delete impact of a playlist that is no longer shown", async () => {
    const subject = controller();
    mocks.getPlaylist.mockResolvedValueOnce(playlist("a"));
    await subject.load("a");
    mocks.deleteImpact.mockResolvedValueOnce(impact(3));
    await subject.inspectDelete();
    expect(subject.deleteImpact?.membershipCount).toBe(3);

    mocks.getPlaylist.mockResolvedValueOnce(playlist("b"));
    await subject.load("b");
    expect(subject.deleteImpact).toBeNull();
  });

  it("drops a delete impact read that lands after the playlist changed", async () => {
    const subject = controller();
    mocks.getPlaylist.mockResolvedValueOnce(playlist("a"));
    await subject.load("a");
    const read = deferred<MusicPlaylistDeleteImpact>();
    mocks.deleteImpact.mockReturnValueOnce(read.promise);
    const inspecting = subject.inspectDelete();

    mocks.getPlaylist.mockResolvedValueOnce(playlist("b"));
    await subject.load("b");
    read.resolve(impact(3));
    await expect(inspecting).resolves.toBe(false);

    expect(subject.deleteImpact).toBeNull();
  });
});
