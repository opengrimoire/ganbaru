import { describe, expect, it, vi } from "vitest";
import {
  createMusicLibraryController,
  type MusicLibraryControllerApi,
} from "./controller.svelte";
import type { MusicItemWindow } from "./contracts";

function window(id: string, title = id): MusicItemWindow {
  return {
    items: [{
      id, identityKey: `local:${id}`, sourceKind: "local-file", mediaKind: "audio",
      title, artist: "", album: "", localRootId: "root-1", relativePath: `${title}.flac`, sourceCollectionIds: ["source-1"], originalArtworkIdentity: null, artworkOverride: null, durationMs: null, availability: "available",
      reviewState: "unreviewed", discoveredAtMs: 1, updatedAtMs: 1, version: 1,
      playlistCount: 0, activeSnoozeCount: 0, lastPlayedAtMs: null, playCount: 0,
      membershipId: null, membershipPosition: null, membershipWeight: null,
      membershipEnabled: null, membershipVersion: null,
    }],
    groups: [], totalCount: 1, offset: 0, limit: 50,
  };
}

function api(itemWindow: MusicLibraryControllerApi["itemWindow"]): MusicLibraryControllerApi {
  return {
    itemWindow,
    playlistSummaries: vi.fn(async () => []),
    sourceSummaries: vi.fn(async () => []),
    issues: vi.fn(async () => []),
  };
}

describe("Music library controller", () => {
  it("preserves independent filters, selection, and scroll for every destination", async () => {
    const controller = createMusicLibraryController(api(async (request) => window(request.destination)));
    controller.setVault("vault-1");
    controller.navigate({ kind: "library" });
    controller.patchCurrentState({ search: "rain", availability: "available" });
    controller.setScrollTop(320);
    await controller.refresh();
    controller.selectItem("library");

    controller.navigate({ kind: "review" });
    controller.patchCurrentState({ search: "new" });
    controller.setScrollTop(40);
    controller.navigate({ kind: "library" });

    expect(controller.currentState).toMatchObject({
      search: "rain",
      availability: "available",
      scrollTop: 320,
      selectedItemId: "library",
    });
    expect(controller.selectedItem?.id).toBe("library");
  });

  it("preloads core windows once and reuses them during destination navigation", async () => {
    const itemWindow = vi.fn(async (request) => window(request.destination));
    const controller = createMusicLibraryController(api(itemWindow));
    controller.setVault("vault-1");

    expect(await controller.preloadCoreDestinations()).toBe(true);
    expect(itemWindow).toHaveBeenCalledTimes(2);
    expect(controller.windows.review?.items[0]?.id).toBe("review");
    expect(controller.windows.library?.items[0]?.id).toBe("library");

    controller.navigate({ kind: "library" });
    expect(await controller.ensureCurrentDestination()).toBe(true);
    controller.navigate({ kind: "review" });
    expect(await controller.ensureCurrentDestination()).toBe(true);
    expect(itemWindow).toHaveBeenCalledTimes(2);
  });

  it("refreshes the retained Review projection while Sources is open", async () => {
    let reviewId = "review-old";
    const itemWindow = vi.fn(async (request) => window(
      request.destination === "review" ? reviewId : request.destination,
    ));
    const controller = createMusicLibraryController(api(itemWindow));
    controller.setVault("vault-1");
    await controller.preloadCoreDestinations();
    controller.navigate({ kind: "sources" });
    reviewId = "review-new";

    expect(await controller.refreshAfterMutation()).toBe(true);
    expect(controller.windows.review?.items[0]?.id).toBe("review-new");
  });

  it("loads the complete Review window before exposing the preloaded workspace", async () => {
    const first = window("review-1");
    first.totalCount = 3;
    const itemWindow = vi.fn(async (request) => {
      if (request.destination !== "review") return window("library");
      if (request.offset === 0) return first;
      return {
        ...window("review-2"),
        items: [window("review-2").items[0]!, window("review-3").items[0]!],
        totalCount: 3,
        offset: request.offset,
      };
    });
    const controller = createMusicLibraryController(api(itemWindow));
    controller.setVault("vault-1");

    expect(await controller.preloadCoreDestinations()).toBe(true);
    expect(controller.windows.review?.items.map((item) => item.id)).toEqual([
      "review-1",
      "review-2",
      "review-3",
    ]);
    expect(itemWindow).toHaveBeenCalledTimes(3);
  });

  it("keeps the retained Review window visible until a refreshed projection is complete", async () => {
    let refreshing = false;
    let releaseFinalPage!: () => void;
    const itemWindow = vi.fn(async (request) => {
      if (request.destination === "library") return window("library");
      if (!refreshing) return window("review-old");
      if (request.offset === 0) {
        const first = window("review-new-1");
        first.totalCount = 3;
        return first;
      }
      await new Promise<void>((resolve) => { releaseFinalPage = resolve; });
      return {
        ...window("review-new-2"),
        items: [window("review-new-2").items[0]!, window("review-new-3").items[0]!],
        totalCount: 3,
        offset: request.offset,
      };
    });
    const controller = createMusicLibraryController(api(itemWindow));
    controller.setVault("vault-1");
    expect(await controller.preloadCoreDestinations()).toBe(true);

    refreshing = true;
    const refresh = controller.refreshAfterMutation();
    await vi.waitFor(() => expect(itemWindow).toHaveBeenCalledTimes(4));

    expect(controller.currentWindow.items.map((item) => item.id)).toEqual(["review-old"]);

    releaseFinalPage();
    expect(await refresh).toBe(true);
    expect(controller.currentWindow.items.map((item) => item.id)).toEqual([
      "review-new-1",
      "review-new-2",
      "review-new-3",
    ]);
  });

  it("keeps stale windows visible while refreshing them after a mutation", async () => {
    const itemWindow = vi.fn(async (request) => window(`${request.destination}-${itemWindow.mock.calls.length}`));
    const controller = createMusicLibraryController(api(itemWindow));
    controller.setVault("vault-1");
    await controller.preloadCoreDestinations();
    controller.navigate({ kind: "review" });

    await controller.refreshAfterMutation();
    controller.navigate({ kind: "library" });
    expect(controller.currentWindow.items).toHaveLength(1);
    await controller.ensureCurrentDestination();

    expect(itemWindow).toHaveBeenCalledTimes(4);
    expect(controller.currentWindow.items[0]?.id).toBe("library-4");
  });

  it("refreshes mutation summaries without replacing a locally updated Review window", async () => {
    const itemWindow = vi.fn(async (request) => window(request.destination));
    const controllerApi = api(itemWindow);
    const controller = createMusicLibraryController(controllerApi);
    controller.setVault("vault-1");
    await controller.preloadCoreDestinations();
    controller.navigate({ kind: "review" });
    controller.currentWindow.items[0]!.reviewState = "reviewed";

    expect(await controller.refreshSummariesAfterMutation()).toBe(true);

    expect(itemWindow).toHaveBeenCalledTimes(2);
    expect(controller.currentWindow.items[0]?.reviewState).toBe("reviewed");
    expect(controllerApi.playlistSummaries).toHaveBeenCalledTimes(2);
  });

  it("supports stable toggle and range selection without clearing it on filters", async () => {
    const controller = createMusicLibraryController(api(async () => ({
      ...window("first"),
      items: [window("first").items[0], window("second").items[0], window("third").items[0]],
      totalCount: 3,
    })));
    controller.setVault("vault-1");
    controller.navigate({ kind: "library" });
    await controller.refresh();

    controller.selectItemRange("first", "third");
    expect(controller.currentState.selectedItemIds).toEqual(["first", "second", "third"]);
    controller.toggleItemSelection("second");
    controller.patchCurrentState({ search: "soundtrack" });

    expect(controller.currentState.selectedItemIds).toEqual(["first", "third"]);
    expect(controller.currentState.selectedItemId).toBe("third");
  });

  it("ignores an older refresh that resolves after the newest request", async () => {
    let resolveFirst!: (value: MusicItemWindow) => void;
    const first = new Promise<MusicItemWindow>((resolve) => { resolveFirst = resolve; });
    const itemWindow = vi.fn()
      .mockImplementationOnce(async () => first)
      .mockResolvedValueOnce(window("newest"));
    const controller = createMusicLibraryController(api(itemWindow));
    controller.setVault("vault-1");
    controller.navigate({ kind: "library" });

    const older = controller.refresh();
    const newest = controller.refresh();
    expect(await newest).toBe(true);
    resolveFirst(window("stale"));
    expect(await older).toBe(false);

    expect(controller.currentWindow.items[0]?.id).toBe("newest");
  });

  it("rolls back the current optimistic mutation when persistence fails", async () => {
    const controller = createMusicLibraryController(api(async () => window("item-1")));
    let checked = false;
    await expect(controller.runOptimistic({
      key: "item-1:playlist-1",
      label: "Add to Focus",
      apply: () => { checked = true; },
      rollback: () => { checked = false; },
      persist: async () => { throw new Error("save failed"); },
    })).rejects.toThrow("save failed");
    expect(checked).toBe(false);
  });

  it("does not let an older failure reverse newer optimistic intent", async () => {
    const controller = createMusicLibraryController(api(async () => window("item-1")));
    let releaseOlder!: () => void;
    const olderFailure = new Promise<void>((_resolve, reject) => {
      releaseOlder = () => reject(new Error("old failure"));
    });
    let value = 0;
    const persistNewer = vi.fn(async () => undefined);
    const older = controller.runOptimistic({
      key: "weight:item-1:playlist-1", label: "Change weight",
      apply: () => { value = 1; }, rollback: () => { value = 0; },
      persist: async () => olderFailure,
    });
    const newer = controller.runOptimistic({
      key: "weight:item-1:playlist-1", label: "Change weight",
      apply: () => { value = 2; }, rollback: () => { value = 1; },
      persist: persistNewer,
    });
    expect(persistNewer).not.toHaveBeenCalled();
    releaseOlder();
    await expect(older).rejects.toThrow("old failure");
    await newer;
    expect(persistNewer).toHaveBeenCalledOnce();
    expect(value).toBe(2);
  });

  it("clears vault-scoped state and invalidates pending refreshes on vault switch", async () => {
    let resolve!: (value: MusicItemWindow) => void;
    const pending = new Promise<MusicItemWindow>((done) => { resolve = done; });
    const controller = createMusicLibraryController(api(async () => pending));
    controller.setVault("vault-1");
    controller.navigate({ kind: "library" });
    const refresh = controller.refresh();
    controller.setVault("vault-2");
    resolve(window("old-vault"));

    expect(await refresh).toBe(false);
    expect(controller.currentWindow.items).toEqual([]);
    expect(controller.playlistSummaries).toEqual([]);
    expect(controller.undoCount).toBe(0);
  });

  it("keeps a bounded contextual Undo journal", async () => {
    const controller = createMusicLibraryController(api(async () => window("item-1")));
    const undo = vi.fn(async () => undefined);
    await controller.runOptimistic({
      key: "membership:item-1", label: "Add to Focus",
      apply: () => undefined, rollback: () => undefined,
      persist: async () => undefined, undo,
    });
    expect(controller.lastUndoLabel).toBe("Add to Focus");
    expect(await controller.undoLast()).toBe(true);
    expect(undo).toHaveBeenCalledOnce();
    expect(controller.undoCount).toBe(0);
  });

  it("appends bounded pages without duplicating retained rows", async () => {
    const itemWindow = vi.fn(async (request) => ({
      ...window(request.offset === 0 ? "first" : "second"),
      totalCount: 2,
      offset: request.offset,
      limit: request.limit,
    }));
    const controller = createMusicLibraryController(api(itemWindow));
    controller.setVault("vault-1");
    controller.navigate({ kind: "library" });
    await controller.refresh();
    await controller.loadMore();

    expect(controller.currentWindow.items.map((item) => item.id)).toEqual(["first", "second"]);
    expect(itemWindow).toHaveBeenLastCalledWith(expect.objectContaining({ offset: 1, limit: 200 }));
  });

  it("loads every remaining page before a first-use projection is presented", async () => {
    const itemWindow = vi.fn(async (request) => ({
      ...window(request.offset === 0 ? "first" : request.offset === 1 ? "second" : "third"),
      totalCount: 3,
      offset: request.offset,
      limit: request.limit,
    }));
    const controller = createMusicLibraryController(api(itemWindow));
    controller.setVault("vault-1");
    controller.navigate({ kind: "review" });
    await controller.refresh();

    expect(await controller.loadAllCurrentItems()).toBe(true);

    expect(controller.currentWindow.items.map((item) => item.id)).toEqual(["first", "second", "third"]);
    expect(itemWindow).toHaveBeenLastCalledWith(expect.objectContaining({ offset: 2, limit: 200 }));
  });

  it("stops full projection loading when a page makes no progress", async () => {
    const itemWindow = vi.fn(async (request) => ({
      ...window("first"),
      totalCount: 2,
      offset: request.offset,
      limit: request.limit,
    }));
    const controller = createMusicLibraryController(api(itemWindow));
    controller.setVault("vault-1");
    controller.navigate({ kind: "review" });

    expect(await controller.refresh()).toBe(false);
    expect(controller.error?.message).toBe("The complete Review list could not be loaded.");
    expect(itemWindow).toHaveBeenCalledTimes(2);
  });
});
