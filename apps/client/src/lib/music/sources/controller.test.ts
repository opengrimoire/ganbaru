import { describe, expect, it, vi } from "vitest";
import type { MusicRelinkPlanSummary, MusicSourceCollection } from "$lib/music/library/contracts";
import type { MusicSourceRefreshController } from "./refresh";
import {
  createMusicSourcesController,
  type MusicSourcesControllerApi,
} from "./controller.svelte";

function refreshStub(): MusicSourceRefreshController {
  return {
    prepare: (targets) => ({
      id: 1,
      targets,
      localCount: targets.filter((target) => target.kind === "local-root").length,
      onlineCount: targets.filter((target) => target.kind === "youtube-playlist").length,
      requiresNetworkConfirmation: targets.some((target) => target.kind === "youtube-playlist"),
    }),
    run: vi.fn(async () => []),
    cancel: vi.fn(async () => undefined),
    statuses: () => [],
  };
}

/** Mirror the native derivation of a local root collection key closely enough for controller assertions. */
function rootCollectionId(rootId: string): string {
  return rootId.replace(/^root-/, "collection-");
}

function api(overrides: Partial<MusicSourcesControllerApi> = {}): MusicSourcesControllerApi {
  return {
    roots: vi.fn(async () => []),
    collections: vi.fn(async () => []),
    bindings: vi.fn(async () => []),
    pickFolder: vi.fn(async () => null),
    detectDefaultFolder: vi.fn(async () => null),
    pickFile: vi.fn(async () => null),
    createRoot: vi.fn(async (request) => ({ id: rootCollectionId(request.rootId), version: 1 })),
    bindRoot: vi.fn(async (_vaultId, rootId, folderPath) => ({ rootId, folderPath, status: "available" as const })),
    clearBinding: vi.fn(async (_vaultId, rootId) => ({ rootId, folderPath: null, status: "needs-relink" as const })),
    previewItemRepair: vi.fn(async (itemId, filePath) => ({
      itemId, folderPath: filePath, relativePath: "track.flac", title: "Track", artist: "", album: "",
      durationMs: 1, fileSizeBytes: 1, lightweightFingerprint: "sample:1", strongFingerprint: "sha256:1",
      matchStrength: "exact" as const, reasons: ["match"],
    })),
    applyItemRepair: vi.fn(async (request) => ({ id: "repair-location", version: 1 })),
    undoItemRepair: vi.fn(async () => undefined),
    resolveYouTube: vi.fn(async (source) => ({
      kind: source.kind,
      videoId: source.kind === "youtube-video" ? source.videoId : source.videoId,
      playlistId: source.kind === "youtube-playlist" ? source.playlistId : null,
      title: source.title,
      channel: "Channel",
      durationMs: 120_000,
      videoIds: source.kind === "youtube-video" ? [source.videoId] : ["video-1", "video-2"],
      videos: (source.kind === "youtube-video" ? [source.videoId] : ["video-1", "video-2"]).map((videoId) => ({
        videoId,
        title: videoId,
        channel: "Channel",
        durationMs: source.kind === "youtube-video" ? 120_000 : null,
        metadataResolved: true,
      })),
      metadataTruncated: false,
      duplicateCount: 0,
    })),
    youtubeDuplicateCount: vi.fn(async () => 0),
    saveYouTubeVideo: vi.fn(async (request) => ({ id: request.videoId, version: 1 })),
    saveCollection: vi.fn(async () => ({ id: "collection-1", version: 1 })),
    saveYouTubePlaylist: vi.fn(async (request) => ({
      collectionId: `collection-${request.playlistId}`,
      canonicalItemCount: request.videoIds.length,
      newlyDiscoveredCount: request.videoIds.length,
      repeatedVideoCount: 0,
      generation: 1,
    })),
    removalImpact: vi.fn(async (collectionId) => ({
      collectionId, itemCount: 0, membershipCount: 0, sharedItemCount: 0,
      orphanedItemCount: 0, activeRefreshCount: 0,
    })),
    removeSource: vi.fn(async (request) => request.expectedImpact),
    createRelink: vi.fn(async (request) => ({
      id: request.planId, rootId: request.rootId, state: "ready" as const, exactCount: 0,
      likelyCount: 0, ambiguousCount: 0, missingCount: 0, newCount: 0,
      createdAtMs: request.createdAtMs, updatedAtMs: request.createdAtMs,
    })),
    relinkEntries: vi.fn(async (_planId, offset, limit) => ({ entries: [], totalCount: 0, offset, limit })),
    applyRelink: vi.fn(async (request) => ({
      id: request.planId, rootId: "root-1", state: "applied" as const, exactCount: 0,
      likelyCount: 0, ambiguousCount: 0, missingCount: 0, newCount: 0,
      createdAtMs: request.appliedAt, updatedAtMs: request.appliedAt,
    })),
    cancelRelink: vi.fn(async (planId, cancelledAt) => ({
      id: planId, rootId: "root-1", state: "cancelled" as const, exactCount: 0,
      likelyCount: 0, ambiguousCount: 0, missingCount: 0, newCount: 0,
      createdAtMs: cancelledAt, updatedAtMs: cancelledAt,
    })),
    ...overrides,
  };
}

function localCollection(id: string, localRootId: string): MusicSourceCollection {
  return {
    id,
    kind: "local-root",
    identityKey: `local-root:${localRootId}`,
    name: id,
    localRootId,
    youtubePlaylistId: null,
    discoveryEnabled: true,
    refreshState: "idle",
    lastSuccessfulRefreshAtMs: null,
    previousSuccessfulRefreshAtMs: null,
    lastRefreshErrorCode: null,
    snapshotGeneration: 0,
    createdAtMs: 1,
    updatedAtMs: 1,
    version: 1,
    removedAtMs: null,
  };
}

describe("MusicSourcesController", () => {
  it("loads roots, collections, and device bindings as one vault projection", async () => {
    const controller = createMusicSourcesController(api({
      roots: vi.fn(async () => [{ id: "root-1", name: "OST", createdAtMs: 1, updatedAtMs: 1, version: 1 }]),
      bindings: vi.fn(async () => [{ rootId: "root-1", folderPath: "/music/ost", status: "available" as const }]),
    }), () => 10, () => "id", refreshStub());
    controller.setVault("vault-1");
    expect(await controller.load()).toBe(true);
    expect(controller.loaded).toBe(true);
    expect(controller.roots[0]?.name).toBe("OST");
    expect(controller.bindings[0]?.folderPath).toBe("/music/ost");
  });

  it("renames an existing collection without changing its source identity", async () => {
    const collection = {
      id: "collection-1", kind: "local-root" as const, identityKey: "local-root:root-1",
      name: "Music", localRootId: "root-1", youtubePlaylistId: null, refreshState: "idle" as const,
      lastSuccessfulRefreshAtMs: null, previousSuccessfulRefreshAtMs: null, lastRefreshErrorCode: null,
      snapshotGeneration: 1, createdAtMs: 1, updatedAtMs: 1, version: 1, discoveryEnabled: true, removedAtMs: null,
    };
    const saveCollection = vi.fn(async (request: Parameters<MusicSourcesControllerApi["saveCollection"]>[0]) => ({ id: `collection-${request.localRootId ?? request.youtubePlaylistId}`, version: 2 }));
    const controller = createMusicSourcesController(api({
      collections: vi.fn(async () => [collection]),
      saveCollection,
    }), () => 20, () => "id", refreshStub());
    controller.setVault("vault-1");
    await controller.load();

    await controller.renameCollection("collection-1", "Soundtracks");

    expect(saveCollection).toHaveBeenCalledWith(expect.objectContaining({
      kind: "local-root",
      name: "Soundtracks",
      localRootId: "root-1",
      youtubePlaylistId: null,
    }));
    expect(saveCollection.mock.calls[0]?.[0]).not.toHaveProperty("id");
  });

  it("preserves folder preview and reports duplicate relationships before writing", async () => {
    const createRoot = vi.fn(async (request: Parameters<MusicSourcesControllerApi["createRoot"]>[0]) => ({ id: rootCollectionId(request.rootId), version: 1 }));
    const controller = createMusicSourcesController(api({
      pickFolder: vi.fn(async () => ({ folderPath: "/music/ost", tracks: [], truncated: false })),
      createRoot,
    }), () => 10, () => "id", refreshStub());
    controller.setVault("vault-1");
    controller.bindings = [{ rootId: "root-1", folderPath: "/music/ost/", status: "available" }];
    const draft = await controller.chooseLocalFolder();
    expect(draft).toMatchObject({ name: "ost", relationship: "duplicate" });
    expect(createRoot).not.toHaveBeenCalled();
  });

  it("reselects an Android document tree without replacing the logical source", async () => {
    const collection = {
      id: "collection-1",
      kind: "local-root" as const,
      identityKey: "local-root:root-1",
      name: "Music",
      localRootId: "root-1",
      youtubePlaylistId: null,
      discoveryEnabled: true,
      refreshState: "idle" as const,
      lastSuccessfulRefreshAtMs: null,
      previousSuccessfulRefreshAtMs: null,
      lastRefreshErrorCode: null,
      snapshotGeneration: 0,
      createdAtMs: 1,
      updatedAtMs: 1,
      version: 1,
      removedAtMs: null,
    };
    const bindRoot = vi.fn(async (_vaultId: string, rootId: string, folderPath: string) => ({
      rootId,
      folderPath,
      status: "available" as const,
    }));
    const refresh = refreshStub();
    const controller = createMusicSourcesController(api({
      roots: vi.fn(async () => [{ id: "root-1", name: "Music", createdAtMs: 1, updatedAtMs: 1, version: 1 }]),
      collections: vi.fn(async () => [collection]),
      bindings: vi.fn(async () => [{ rootId: "root-1", folderPath: "content://new-tree", status: "available" as const }]),
      pickFolder: vi.fn(async () => ({ folderPath: "content://new-tree", tracks: [], truncated: false })),
      bindRoot,
    }), () => 20, () => "job-1", refresh);
    controller.setVault("vault-1");

    expect(await controller.reselectLocalRoot(collection)).toBe(true);
    expect(bindRoot).toHaveBeenCalledWith("vault-1", "root-1", "content://new-tree");
    expect(refresh.run).toHaveBeenCalledOnce();
    expect(refresh.prepare).toBeDefined();
  });

  it("lists only local sources whose folder is unusable on this device", () => {
    const controller = createMusicSourcesController(api(), () => 20, () => "id", refreshStub());
    controller.collections = [
      localCollection("never-linked", "never-root"),
      localCollection("moved", "moved-root"),
      localCollection("ready", "ready-root"),
      localCollection("loading", "loading-root"),
    ];
    controller.bindings = [
      { rootId: "never-root", folderPath: null, status: "needs-relink" },
      { rootId: "moved-root", folderPath: "/music/old", status: "missing" },
      { rootId: "ready-root", folderPath: "/music/ready", status: "available" },
    ];

    expect(controller.unlinkedLocalSources().map(({ collection, status }) => [collection.id, status])).toEqual([
      ["never-linked", "needs-relink"],
      ["moved", "missing"],
    ]);
    expect(controller.unlinkedLocalSources(["moved", "ready"]).map(({ collection }) => collection.id)).toEqual(["moved"]);
    expect(controller.prepareRefresh(["never-linked"]).targets).toEqual([]);
  });

  it("points this device at the relinked folder and scans it after applying the plan", async () => {
    const collection = localCollection("collection-1", "root-1");
    const bindRoot = vi.fn(async (_vaultId: string, rootId: string, folderPath: string) => ({
      rootId,
      folderPath,
      status: "available" as const,
    }));
    const refresh = refreshStub();
    const controller = createMusicSourcesController(api({
      collections: vi.fn(async () => [collection]),
      bindRoot,
    }), () => 20, () => "plan-1", refresh);
    controller.setVault("vault-1");

    await controller.planRelink("root-1", "/music/moved");
    await controller.applyRelink([]);

    expect(bindRoot).toHaveBeenCalledWith("vault-1", "root-1", "/music/moved");
    await vi.waitFor(() => expect(refresh.run).toHaveBeenCalledOnce());
    const [plan] = vi.mocked(refresh.run).mock.calls[0];
    expect(plan.targets).toEqual([expect.objectContaining({
      collectionId: "collection-1",
      request: expect.objectContaining({ rootId: "root-1", folderPath: "/music/moved" }),
    })]);
  });

  it("does not bind a folder when the relink plan was cancelled", async () => {
    const bindRoot = vi.fn();
    const controller = createMusicSourcesController(api({ bindRoot }), () => 20, () => "plan-1", refreshStub());
    controller.setVault("vault-1");

    await controller.planRelink("root-1", "/music/moved");
    await controller.cancelRelink();

    await expect(controller.applyRelink([])).rejects.toThrow("No relink plan is ready.");
    expect(bindRoot).not.toHaveBeenCalled();
  });

  it("cancels a relink plan that is still scanning and does not open it for review", async () => {
    let finishScan: (state: "ready" | "cancelled") => void = () => undefined;
    const createRelink = vi.fn((request: { planId: string; rootId: string; createdAtMs: number }) => new Promise<MusicRelinkPlanSummary>((resolve) => {
      finishScan = (state) => resolve({
        id: request.planId, rootId: request.rootId, state, exactCount: 0,
        likelyCount: 0, ambiguousCount: 0, missingCount: 0, newCount: 0,
        createdAtMs: request.createdAtMs, updatedAtMs: request.createdAtMs,
      });
    }));
    const cancelRelink = vi.fn(async (planId: string, cancelledAt: number) => ({
      id: planId, rootId: "root-1", state: "cancelled" as const, exactCount: 0,
      likelyCount: 0, ambiguousCount: 0, missingCount: 0, newCount: 0,
      createdAtMs: cancelledAt, updatedAtMs: cancelledAt,
    }));
    const relinkEntries = vi.fn(async (_planId: string, offset: number, limit: number) => ({ entries: [], totalCount: 0, offset, limit }));
    const controller = createMusicSourcesController(api({ createRelink, cancelRelink, relinkEntries }), () => 20, () => "plan-1", refreshStub());

    const planning = controller.planRelink("root-1", "/music/moved");
    expect(controller.relinkCancellable).toBe(true);
    await controller.cancelRelink();
    finishScan("cancelled");
    const plan = await planning;

    expect(cancelRelink).toHaveBeenCalledWith("plan-1", 20);
    expect(plan.state).toBe("cancelled");
    expect(relinkEntries).not.toHaveBeenCalled();
    expect(controller.relinkPlan).toBeNull();
    expect(controller.relinkCancellable).toBe(false);
  });

  it("surfaces a failed initial folder refresh instead of opening an empty review", async () => {
    const refresh = refreshStub();
    refresh.run = vi.fn(async (plan) => [{
      collectionId: plan.targets[0]?.collectionId ?? "collection-1",
      kind: "local-root" as const,
      name: "Music",
      state: "failed" as const,
      progress: null,
      error: "The Android music refresh could not be saved.",
    }]);
    const ids = ["root-1", "job-1"];
    const controller = createMusicSourcesController(
      api(),
      () => 20,
      () => ids.shift() ?? "id",
      refresh,
    );
    controller.setVault("vault-1");

    await expect(controller.addLocalFolder({
      folderPath: "content://music",
      tracks: [],
      truncated: false,
    }, "Music", true)).rejects.toThrow("The Android music refresh could not be saved.");
    expect(controller.error).toBe("The Android music refresh could not be saved.");
  });

  it("recovers only uninitialized local sources with an available folder grant", async () => {
    const controller = createMusicSourcesController(api(), () => 20, () => "job-1", refreshStub());
    controller.collections = [
      {
        id: "recover",
        kind: "local-root",
        identityKey: "local-root:recover-root",
        name: "Music",
        localRootId: "recover-root",
        youtubePlaylistId: null,
        discoveryEnabled: true,
        refreshState: "idle",
        lastSuccessfulRefreshAtMs: null,
        previousSuccessfulRefreshAtMs: null,
        lastRefreshErrorCode: null,
        snapshotGeneration: 0,
        createdAtMs: 1,
        updatedAtMs: 1,
        version: 1,
        removedAtMs: null,
      },
      {
        id: "ready",
        kind: "local-root",
        identityKey: "local-root:ready-root",
        name: "Ready",
        localRootId: "ready-root",
        youtubePlaylistId: null,
        discoveryEnabled: true,
        refreshState: "idle",
        lastSuccessfulRefreshAtMs: 10,
        previousSuccessfulRefreshAtMs: null,
        lastRefreshErrorCode: null,
        snapshotGeneration: 1,
        createdAtMs: 1,
        updatedAtMs: 10,
        version: 2,
        removedAtMs: null,
      },
    ];
    controller.bindings = [
      { rootId: "recover-root", folderPath: "content://music", status: "available" },
      { rootId: "ready-root", folderPath: "content://ready", status: "available" },
    ];

    const plan = controller.prepareUninitializedLocalRefresh();

    expect(plan.targets.map((target) => target.collectionId)).toEqual(["recover"]);
  });

  it("does not finish loading sources before first-use folder detection finishes", async () => {
    let releaseDetection!: () => void;
    const detectDefaultFolder = vi.fn(() => new Promise<null>((resolve) => {
      releaseDetection = () => resolve(null);
    }));
    const controller = createMusicSourcesController(api({ detectDefaultFolder }), () => 10, () => "id", refreshStub());
    controller.setVault("vault-1");
    let settled = false;

    const loading = controller.load().then((result) => {
      settled = true;
      return result;
    });
    const concurrentLoad = controller.load();
    await vi.waitFor(() => expect(controller.preparingDefaultFolder).toBe(true));

    expect(settled).toBe(false);
    releaseDetection();
    expect(await loading).toBe(true);
    expect(await concurrentLoad).toBe(true);
    expect(controller.preparingDefaultFolder).toBe(false);
    expect(detectDefaultFolder).toHaveBeenCalledOnce();
  });

  it("automatically adopts and scans the system Music folder once when no local root exists", async () => {
    const detectDefaultFolder = vi.fn(async () => ({
      folderPath: "/home/user/Music",
      tracks: [{ path: "/home/user/Music/focus.flac", title: "focus", artworkPath: null }],
      truncated: false,
    }));
    const createRoot = vi.fn(async (request: Parameters<MusicSourcesControllerApi["createRoot"]>[0]) => ({ id: rootCollectionId(request.rootId), version: 1 }));
    const bindRoot = vi.fn(async (_vaultId: string, rootId: string, folderPath: string) => ({ rootId, folderPath, status: "available" as const }));
    const refresh = refreshStub();
    const ids = ["root-1", "job-1"];
    const controller = createMusicSourcesController(api({ detectDefaultFolder, createRoot, bindRoot }), () => 10, () => ids.shift() ?? "id", refresh);
    controller.setVault("vault-1");

    await controller.load();
    await vi.waitFor(() => expect(refresh.run).toHaveBeenCalledOnce());
    await controller.load();

    expect(detectDefaultFolder).toHaveBeenCalledOnce();
    expect(bindRoot).toHaveBeenCalledWith("vault-1", "root-1", "/home/user/Music");
    expect(createRoot).toHaveBeenCalledOnce();
    expect(controller.detectedDefaultFolder).toBeNull();
    expect(controller.preparingDefaultFolder).toBe(false);
    expect(controller.preparingDefaultFolderPath).toBe("/home/user/Music");
    expect(controller.firstUseSession).toBe(true);

    controller.completeFirstUseSession();

    expect(controller.firstUseSession).toBe(false);
  });

  it("does not detect a default folder when a local root already exists", async () => {
    const detectDefaultFolder = vi.fn(async () => null);
    const controller = createMusicSourcesController(api({
      roots: vi.fn(async () => [{ id: "root-1", name: "OST", createdAtMs: 1, updatedAtMs: 1, version: 1 }]),
      detectDefaultFolder,
    }), () => 10, () => "id", refreshStub());
    controller.setVault("vault-1");

    await controller.load();

    expect(detectDefaultFolder).not.toHaveBeenCalled();
    expect(controller.firstUseSession).toBe(false);
  });

  it("adds the detected Music folder with one confirmed action", async () => {
    const createRoot = vi.fn(async (request: Parameters<MusicSourcesControllerApi["createRoot"]>[0]) => ({ id: rootCollectionId(request.rootId), version: 1 }));
    const bindRoot = vi.fn(async (_vaultId: string, rootId: string, folderPath: string) => ({ rootId, folderPath, status: "available" as const }));
    const ids = ["root-1"];
    const controller = createMusicSourcesController(api({ createRoot, bindRoot }), () => 20, () => ids.shift()!, refreshStub());
    controller.setVault("vault-1");
    controller.detectedDefaultFolder = {
      folderPath: "/home/user/Music",
      tracks: [{ path: "/home/user/Music/focus.flac", title: "focus", artworkPath: null }],
      truncated: false,
    };

    expect(await controller.addDetectedDefaultFolder()).toBe("collection-1");
    expect(createRoot).toHaveBeenCalledWith(expect.objectContaining({
      rootId: "root-1",
      name: "Music",
    }));
    expect(bindRoot).toHaveBeenCalledWith("vault-1", "root-1", "/home/user/Music");
    expect(controller.detectedDefaultFolder).toBeNull();
  });

  it("prepares a chosen first-use folder through the default folder preparation state", async () => {
    const refresh = refreshStub();
    let finishRefresh: () => void = () => undefined;
    refresh.run = vi.fn(() => new Promise<[]>((resolve) => { finishRefresh = () => resolve([]); }));
    const ids = ["root-1", "job-1"];
    const controller = createMusicSourcesController(api(), () => 20, () => ids.shift() ?? "id", refresh);
    controller.setVault("vault-1");
    const selection = { folderPath: "content://music", tracks: [], truncated: false };

    const prepared = controller.prepareFirstUseFolder(selection, "Music");

    expect(controller.preparingDefaultFolder).toBe(true);
    expect(controller.preparingDefaultFolderPath).toBe("content://music");
    expect(await controller.prepareFirstUseFolder(selection, "Music")).toBeNull();
    await vi.waitFor(() => expect(refresh.run).toHaveBeenCalledOnce());
    finishRefresh();

    expect(await prepared).toBe("collection-1");
    expect(controller.preparingDefaultFolder).toBe(false);
    expect(controller.defaultFolderPreparationError).toBeNull();
  });

  it("reports a failed first-use folder preparation without rejecting", async () => {
    const controller = createMusicSourcesController(api({
      bindRoot: vi.fn(async () => { throw new Error("The folder grant was revoked."); }),
    }), () => 20, () => "id", refreshStub());
    controller.setVault("vault-1");

    expect(await controller.prepareFirstUseFolder({ folderPath: "content://music", tracks: [], truncated: false }, "Music")).toBeNull();
    expect(controller.preparingDefaultFolder).toBe(false);
    expect(controller.defaultFolderPreparationError).toBe("The folder grant was revoked.");
  });

  it("distinguishes videos from playlists and saves a resolved video", async () => {
    const saveYouTubeVideo = vi.fn(async (request: Parameters<MusicSourcesControllerApi["saveYouTubeVideo"]>[0]) => ({ id: request.videoId, version: 1 }));
    const controller = createMusicSourcesController(api({ saveYouTubeVideo }), () => 20, () => "id", refreshStub());
    const parsed = controller.parseYouTubeInput("https://youtu.be/abcDEF_1234");
    expect(parsed.source?.kind).toBe("youtube-video");
    const preview = await controller.resolveYouTube(parsed.source!);
    await controller.addYouTube(preview!, "");
    expect(saveYouTubeVideo).toHaveBeenCalledWith(expect.objectContaining({
      videoId: "abcDEF_1234",
      resolutionState: "ready",
    }));
  });

  it("saves resolved playlist metadata with the playlist snapshot", async () => {
    const saveYouTubePlaylist = vi.fn(async (request: Parameters<MusicSourcesControllerApi["saveYouTubePlaylist"]>[0]) => ({
      collectionId: `collection-${request.playlistId}`,
      canonicalItemCount: request.videoIds.length,
      newlyDiscoveredCount: request.videoIds.length,
      repeatedVideoCount: 0,
      generation: 1,
    }));
    const controller = createMusicSourcesController(api({ saveYouTubePlaylist }), () => 20, () => "id", refreshStub());
    const parsed = controller.parseYouTubeInput("https://youtube.com/playlist?list=PLabcdef12345");
    const preview = await controller.resolveYouTube(parsed.source!);
    await controller.addYouTube(preview!, "Focus soundtrack");

    expect(saveYouTubePlaylist).toHaveBeenCalledWith(expect.objectContaining({
      playlistId: "PLabcdef12345",
      name: "Focus soundtrack",
      videoIds: ["video-1", "video-2"],
      videos: [
        { videoId: "video-1", title: "video-1", channel: "Channel" },
        { videoId: "video-2", title: "video-2", channel: "Channel" },
      ],
    }));
  });

  it("cancels an obsolete YouTube resolution without exposing its result", async () => {
    let release!: () => void;
    const resolveYouTube = vi.fn(async (source: Parameters<MusicSourcesControllerApi["resolveYouTube"]>[0], signal: AbortSignal) => {
      await new Promise<void>((resolve, reject) => {
        release = resolve;
        signal.addEventListener("abort", () => reject(new DOMException("cancelled", "AbortError")), { once: true });
      });
      return {
        kind: source.kind,
        videoId: null,
        playlistId: null,
        title: "Late",
        channel: "",
        durationMs: null,
        videoIds: [],
        videos: [],
        metadataTruncated: false,
        duplicateCount: 0,
      };
    });
    const controller = createMusicSourcesController(api({ resolveYouTube }), () => 20, () => "id", refreshStub());
    const parsed = controller.parseYouTubeInput("https://youtu.be/abcDEF_1234");
    const pending = controller.resolveYouTube(parsed.source!);
    controller.cancelResolution();
    release();
    expect(await pending).toBeNull();
    expect(controller.youtubePreview).toBeNull();
  });

  it("applies a confirmed item repair as another location and can undo it", async () => {
    const applyItemRepair = vi.fn(async (request: Parameters<MusicSourcesControllerApi["applyItemRepair"]>[0]) => ({ id: "repair-location", version: 1 }));
    const bindRoot = vi.fn(async (_vaultId: string, rootId: string, folderPath: string) => ({ rootId, folderPath, status: "available" as const }));
    const undoItemRepair = vi.fn(async () => undefined);
    const clearBinding = vi.fn(async (_vaultId: string, rootId: string) => ({ rootId, folderPath: null, status: "needs-relink" as const }));
    const ids = ["repair-root"];
    const controller = createMusicSourcesController(api({ applyItemRepair, bindRoot, undoItemRepair, clearBinding }), () => 30, () => ids.shift()!, refreshStub());
    controller.setVault("vault-1");
    controller.itemRepairPreview = {
      itemId: "item-1", folderPath: "/music/recovered", relativePath: "track.flac",
      title: "Track", artist: "", album: "", durationMs: 1, fileSizeBytes: 1,
      lightweightFingerprint: "sample:1", strongFingerprint: "sha256:1",
      matchStrength: "weak", reasons: ["weak"],
    };

    await controller.applyItemRepair(true);
    expect(applyItemRepair).toHaveBeenCalledWith(expect.objectContaining({
      itemId: "item-1",
      acceptWeakMismatch: true,
    }));
    expect(bindRoot).toHaveBeenCalledWith("vault-1", "repair-root", "/music/recovered");
    await controller.undoItemRepair();
    expect(undoItemRepair).toHaveBeenCalledWith("repair-location", "repair-root");
    expect(clearBinding).toHaveBeenCalledWith("vault-1", "repair-root");
  });
});
