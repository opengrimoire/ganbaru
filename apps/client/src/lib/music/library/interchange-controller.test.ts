import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MusicTransferCommit, MusicTransferPreview, MusicTransferSource } from "./interchange";
import { MusicInterchangeController } from "./interchange-controller.svelte";
import { MusicLibraryApiError } from "$lib/api/music-library";

const calls = vi.hoisted(() => ({
  preview: vi.fn<(vaultId: string, source: MusicTransferSource) => Promise<MusicTransferPreview>>(),
  commit: vi.fn<(vaultId: string, request: MusicTransferCommit) => Promise<{ playlistCount: number; itemCount: number; membershipCount: number; assignmentCount: number }>>(),
  export: vi.fn(async () => true),
  read: vi.fn(async () => "#EXTM3U\ntrack.flac"),
  pickRoot: vi.fn(async () => "/music"),
  bindRoot: vi.fn(async () => undefined),
}));

vi.mock("$lib/api/music", () => ({ pickAndReadMusicInterchangeFile: calls.read, pickMusicRootBindingFolder: calls.pickRoot }));
vi.mock("$lib/api/music-library", () => ({
  previewMusicTransfer: calls.preview, commitMusicTransfer: calls.commit, exportMusicTransfer: calls.export,
  setLocalRootBinding: calls.bindRoot,
  MusicLibraryApiError: class extends Error { constructor(readonly code: string, message: string) { super(message); } },
}));

const preview: MusicTransferPreview = {
  format: "m3u8", revision: "a".repeat(64), roots: [], availableRoots: [{ id: "root", name: "Music" }],
  boundRootIds: [], contextAssignmentCount: 0, newPlaylists: 1, matchedPlaylists: 0, newItems: 0,
  matchedItems: 0, duplicateItems: 0, missingLocalBindings: 0, conflicts: [], unsupported: [],
  localCount: 1, youtubeCount: 0, unsupportedCount: 0, unresolvedLocalCount: 1,
};

describe("native Music transfer dialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    calls.preview.mockResolvedValue(preview);
    calls.commit.mockResolvedValue({ playlistCount: 1, itemCount: 1, membershipCount: 1, assignmentCount: 0 });
  });

  it("prepares one selected source and exports with one native command", async () => {
    const controller = new MusicInterchangeController(() => [], () => [], () => "vault", () => 100);
    controller.show("import");
    expect(await controller.readImport()).toBe(true);
    expect(calls.preview).toHaveBeenCalledExactlyOnceWith("vault", { contents: "#EXTM3U\ntrack.flac", playlistName: "Imported playlist", relativeRootId: null, selectedAtMs: 100 });
    expect(controller.unresolvedM3uCount()).toBe(1);
    controller.show("export", "playlist");
    expect(await controller.exportSelected()).toBe(true);
    expect(calls.export).toHaveBeenCalledExactlyOnceWith("vault", ["playlist"], "json");
  });

  it("retains the exact action after an uncertain response even if drafts change", async () => {
    const controller = new MusicInterchangeController(() => [], () => [], () => "vault", () => 100);
    controller.show("import");
    await controller.readImport();
    calls.commit.mockRejectedValueOnce(new Error("Response lost"));
    expect(await controller.commitImport()).toBe(false);
    const accepted = JSON.stringify(calls.commit.mock.calls[0]![1]);
    controller.importPlaylistName = "Changed after transport failure";
    controller.show("export");
    expect(controller.mode).toBe("import");
    expect(await controller.commitImport()).toBe(true);
    expect(JSON.stringify(calls.commit.mock.calls[1]![1])).toBe(accepted);
  });

  it("shows a changed native preview and requires another explicit import action", async () => {
    const controller = new MusicInterchangeController(() => [], () => [], () => "vault", () => 100);
    controller.show("import");
    await controller.readImport();
    calls.commit.mockRejectedValueOnce(new MusicLibraryApiError("stale-write", "Preview changed"));
    calls.preview.mockResolvedValueOnce({ ...preview, revision: "b".repeat(64), newItems: 1 });
    expect(await controller.commitImport()).toBe(false);
    expect(calls.commit).toHaveBeenCalledTimes(1);
    expect(controller.m3u8Preview?.revision).toBe("b".repeat(64));
    expect(controller.error).toBe("Preview changed");
    expect(await controller.commitImport()).toBe(true);
    expect(calls.commit.mock.calls[1]![1].expectedRevision).toBe("b".repeat(64));
  });

  it("refreshes changed mapping input before it can be committed and rejects vault changes", async () => {
    let vault = "vault";
    const controller = new MusicInterchangeController(() => [], () => [], () => vault, () => 100);
    controller.show("import");
    await controller.readImport();
    controller.m3u8RootId = "root";
    expect(await controller.commitImport()).toBe(false);
    expect(calls.commit).not.toHaveBeenCalled();
    expect(calls.preview.mock.calls[1]![1].relativeRootId).toBe("root");
    vault = "different-vault";
    expect(await controller.commitImport()).toBe(false);
    expect(calls.commit).not.toHaveBeenCalled();
  });
});
