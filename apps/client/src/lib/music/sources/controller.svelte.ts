import { detectDefaultMusicFolder, pickMediaFile, pickMediaFolder, type MediaFolderSelection } from "$lib/api/music";
import {
  applyMusicRelinkPlan,
  applyMusicItemRepair,
  applyMusicYouTubePlaylistSnapshot,
  cancelMusicRelinkPlan,
  clearLocalRootBinding,
  createMusicLocalRoot,
  createMusicRelinkPlan,
  getLocalRootBindings,
  getMusicLocalRoots,
  getMusicRelinkPlanEntries,
  getMusicSourceCollections,
  getMusicSourceRemovalImpact,
  getMusicYouTubeDuplicateCount,
  removeMusicSource,
  previewMusicItemRepair,
  setLocalRootBinding,
  upsertMusicYouTubeVideo,
  upsertMusicSourceCollection,
  undoMusicItemRepair,
} from "$lib/api/music-library";
import {
  type LocalRootBinding,
  type MusicLocalRoot,
  type MusicItemRepairPreview,
  type MusicRelinkDecision,
  type MusicRelinkPlanEntry,
  type MusicRelinkPlanSummary,
  type MusicSourceCollection,
  type MusicSourceRemovalImpact,
} from "$lib/music/library/contracts";
import {
  musicFolderDisplayName,
  musicFolderRelationship,
  type MusicLocalSourceSelection,
} from "./drafts";
import {
  createMusicSourceRefreshController,
  type MusicSourceRefreshPlan,
  type MusicSourceRefreshController,
  type MusicSourceRefreshStatus,
  type MusicSourceRefreshTarget,
} from "./refresh";
import { parseMusicSourceInput, type YouTubePlaylistSource, type YouTubeVideoSource } from "$lib/music/sources";
import { notifyMusicLibraryChanged } from "$lib/music/library/events";
import {
  resolveMusicYouTubeSource,
  type MusicYouTubeSourcePreview,
} from "./youtube-source-resolver";

export interface MusicSourcesControllerApi {
  roots(offset: number, limit: number): Promise<MusicLocalRoot[]>;
  collections(offset: number, limit: number): Promise<MusicSourceCollection[]>;
  bindings(vaultId: string, rootIds: string[]): Promise<LocalRootBinding[]>;
  pickFolder(): Promise<MediaFolderSelection | null>;
  detectDefaultFolder(): Promise<MediaFolderSelection | null>;
  pickFile(): Promise<string | null>;
  createRoot(request: Parameters<typeof createMusicLocalRoot>[0]): ReturnType<typeof createMusicLocalRoot>;
  bindRoot(vaultId: string, rootId: string, folderPath: string): ReturnType<typeof setLocalRootBinding>;
  clearBinding(vaultId: string, rootId: string): ReturnType<typeof clearLocalRootBinding>;
  previewItemRepair(itemId: string, filePath: string): ReturnType<typeof previewMusicItemRepair>;
  applyItemRepair(request: Parameters<typeof applyMusicItemRepair>[0]): ReturnType<typeof applyMusicItemRepair>;
  undoItemRepair(locationId: string, rootId: string): ReturnType<typeof undoMusicItemRepair>;
  resolveYouTube(source: YouTubeVideoSource | YouTubePlaylistSource, signal: AbortSignal): Promise<MusicYouTubeSourcePreview>;
  youtubeDuplicateCount(videoIds: string[]): Promise<number>;
  saveYouTubeVideo(request: Parameters<typeof upsertMusicYouTubeVideo>[0]): ReturnType<typeof upsertMusicYouTubeVideo>;
  saveYouTubePlaylist(request: Parameters<typeof applyMusicYouTubePlaylistSnapshot>[0]): ReturnType<typeof applyMusicYouTubePlaylistSnapshot>;
  removalImpact(collectionId: string): Promise<MusicSourceRemovalImpact>;
  removeSource(request: Parameters<typeof removeMusicSource>[0]): ReturnType<typeof removeMusicSource>;
  saveCollection(request: Parameters<typeof upsertMusicSourceCollection>[0]): ReturnType<typeof upsertMusicSourceCollection>;
  createRelink(request: Parameters<typeof createMusicRelinkPlan>[0]): Promise<MusicRelinkPlanSummary>;
  relinkEntries(planId: string, offset: number, limit: number): Promise<{ entries: MusicRelinkPlanEntry[]; totalCount: number; offset: number; limit: number }>;
  applyRelink(request: Parameters<typeof applyMusicRelinkPlan>[0]): Promise<MusicRelinkPlanSummary>;
  cancelRelink(planId: string, cancelledAt: number): Promise<MusicRelinkPlanSummary>;
}

const defaultApi: MusicSourcesControllerApi = {
  roots: getMusicLocalRoots,
  collections: getMusicSourceCollections,
  bindings: getLocalRootBindings,
  pickFolder: pickMediaFolder,
  detectDefaultFolder: detectDefaultMusicFolder,
  pickFile: pickMediaFile,
  createRoot: createMusicLocalRoot,
  bindRoot: setLocalRootBinding,
  clearBinding: clearLocalRootBinding,
  previewItemRepair: previewMusicItemRepair,
  applyItemRepair: applyMusicItemRepair,
  undoItemRepair: undoMusicItemRepair,
  resolveYouTube: resolveMusicYouTubeSource,
  youtubeDuplicateCount: getMusicYouTubeDuplicateCount,
  saveYouTubeVideo: upsertMusicYouTubeVideo,
  saveYouTubePlaylist: applyMusicYouTubePlaylistSnapshot,
  removalImpact: getMusicSourceRemovalImpact,
  removeSource: removeMusicSource,
  saveCollection: upsertMusicSourceCollection,
  createRelink: createMusicRelinkPlan,
  relinkEntries: getMusicRelinkPlanEntries,
  applyRelink: applyMusicRelinkPlan,
  cancelRelink: cancelMusicRelinkPlan,
};

function refreshFailure(statuses: readonly MusicSourceRefreshStatus[]): string | null {
  const failed = statuses.find((status) => status.state === "failed");
  return failed ? failed.error ?? `Failed to refresh ${failed.name}.` : null;
}

export class MusicSourcesController {
  vaultId = $state<string | null>(null);
  roots = $state<MusicLocalRoot[]>([]);
  collections = $state<MusicSourceCollection[]>([]);
  bindings = $state<LocalRootBinding[]>([]);
  refreshStatuses = $state<Record<string, MusicSourceRefreshStatus>>({});
  busy = $state(false);
  loaded = $state(false);
  error = $state<string | null>(null);
  resolving = $state(false);
  resolutionError = $state<string | null>(null);
  youtubePreview = $state<MusicYouTubeSourcePreview | null>(null);
  removalImpact = $state<MusicSourceRemovalImpact | null>(null);
  relinkPlan = $state<MusicRelinkPlanSummary | null>(null);
  relinkEntries = $state<MusicRelinkPlanEntry[]>([]);
  itemRepairPreview = $state<MusicItemRepairPreview | null>(null);
  itemRepairApplied = $state<{ locationId: string; rootId: string } | null>(null);
  detectedDefaultFolder = $state<MediaFolderSelection | null>(null);
  detectingDefaultFolder = $state(false);
  addingDefaultFolder = $state(false);
  preparingDefaultFolder = $state(false);
  preparingDefaultFolderPath = $state<string | null>(null);
  defaultFolderPreparationError = $state<string | null>(null);
  firstUseSession = $state(false);

  private readonly api: MusicSourcesControllerApi;
  private readonly now: () => number;
  private readonly id: () => string;
  private readonly refresh: MusicSourceRefreshController;
  private resolutionController: AbortController | null = null;
  private loadGeneration = 0;
  private pendingLoad: Promise<boolean> | null = null;
  private defaultFolderChecked = false;
  private defaultFolderDismissed = false;

  constructor(
    api: MusicSourcesControllerApi = defaultApi,
    now: () => number = Date.now,
    id: () => string = () => crypto.randomUUID(),
    refreshOverride: MusicSourceRefreshController | null = null,
  ) {
    this.api = api;
    this.now = now;
    this.id = id;
    this.refresh = refreshOverride ?? createMusicSourceRefreshController(
      async (target, signal) => {
        const collection = this.collections.find((entry) => entry.id === target.collectionId);
        if (!collection?.youtubePlaylistId) throw new Error("The YouTube collection is unavailable.");
        const source: YouTubePlaylistSource = {
          kind: "youtube-playlist",
          originalInput: `https://www.youtube.com/playlist?list=${encodeURIComponent(collection.youtubePlaylistId)}`,
          identity: `youtube:playlist:${collection.youtubePlaylistId}`,
          title: collection.name,
          playlistId: collection.youtubePlaylistId,
          videoId: null,
          startMs: null,
          endMs: null,
        };
        const preview = await this.api.resolveYouTube(source, signal);
        await this.api.saveYouTubePlaylist({
          collectionId: collection.id,
          playlistId: collection.youtubePlaylistId,
          name: collection.name,
          videoIds: preview.videoIds,
          videos: preview.videos
            .filter((video) => video.metadataResolved)
            .map(({ videoId, title, channel }) => ({ videoId, title, channel })),
          resolvedAtMs: this.now(),
        });
      },
      {
        onStatus: (status) => {
          this.refreshStatuses[status.collectionId] = status;
        },
      },
    );
  }

  setVault(vaultId: string | null): void {
    const normalized = vaultId?.trim() || null;
    if (normalized === this.vaultId) return;
    this.vaultId = normalized;
    this.loadGeneration += 1;
    this.pendingLoad = null;
    this.cancelResolution();
    this.roots = [];
    this.collections = [];
    this.bindings = [];
    this.refreshStatuses = {};
    this.error = null;
    this.loaded = false;
    this.detectedDefaultFolder = null;
    this.detectingDefaultFolder = false;
    this.addingDefaultFolder = false;
    this.preparingDefaultFolder = false;
    this.preparingDefaultFolderPath = null;
    this.defaultFolderPreparationError = null;
    this.firstUseSession = false;
    this.defaultFolderChecked = false;
    this.defaultFolderDismissed = false;
  }

  load(): Promise<boolean> {
    if (this.pendingLoad) return this.pendingLoad;
    const task = this.loadProjection();
    this.pendingLoad = task;
    void task.then(() => {
      if (this.pendingLoad === task) this.pendingLoad = null;
    }, () => {
      if (this.pendingLoad === task) this.pendingLoad = null;
    });
    return task;
  }

  private async loadProjection(): Promise<boolean> {
    if (!this.vaultId) return false;
    const generation = ++this.loadGeneration;
    const vaultId = this.vaultId;
    this.busy = true;
    this.error = null;
    try {
      const [roots, collections] = await Promise.all([
        this.api.roots(0, 500),
        this.api.collections(0, 500),
      ]);
      const bindings = await this.api.bindings(vaultId, roots.map((root) => root.id));
      if (generation !== this.loadGeneration || vaultId !== this.vaultId) return false;
      this.roots = roots;
      this.collections = collections;
      this.bindings = bindings;
      if (roots.length === 0 && !this.defaultFolderChecked && !this.defaultFolderDismissed) {
        this.firstUseSession = true;
        await this.detectSystemMusicFolder();
      } else if (roots.length > 0) {
        this.detectedDefaultFolder = null;
      }
      return true;
    } catch (error) {
      if (generation !== this.loadGeneration || vaultId !== this.vaultId) return false;
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      if (generation === this.loadGeneration) {
        this.busy = false;
        this.loaded = true;
      }
    }
  }

  async detectSystemMusicFolder(): Promise<MediaFolderSelection | null> {
    if (!this.vaultId || this.defaultFolderChecked || this.defaultFolderDismissed || this.roots.length > 0) return null;
    const generation = this.loadGeneration;
    const vaultId = this.vaultId;
    this.defaultFolderChecked = true;
    this.detectingDefaultFolder = true;
    this.preparingDefaultFolder = true;
    this.defaultFolderPreparationError = null;
    try {
      const selection = await this.api.detectDefaultFolder();
      if (generation !== this.loadGeneration || vaultId !== this.vaultId || this.defaultFolderDismissed) return null;
      this.detectedDefaultFolder = selection;
      this.preparingDefaultFolderPath = selection?.folderPath ?? null;
      if (selection) await this.addDetectedDefaultFolder(true);
      return selection;
    } catch (error) {
      if (vaultId === this.vaultId) {
        this.defaultFolderPreparationError = error instanceof Error ? error.message : String(error);
      }
      return null;
    } finally {
      if (vaultId === this.vaultId) {
        this.detectingDefaultFolder = false;
        this.preparingDefaultFolder = false;
      }
    }
  }

  dismissDefaultMusicFolder(): void {
    this.defaultFolderDismissed = true;
    this.detectedDefaultFolder = null;
  }

  /** Ends the one-time Builder-first presentation after its library is fully ready. */
  completeFirstUseSession(): void {
    this.firstUseSession = false;
  }

  async addDetectedDefaultFolder(waitForRefresh = false): Promise<string | null> {
    const selection = this.detectedDefaultFolder;
    if (!selection || this.addingDefaultFolder) return null;
    this.addingDefaultFolder = true;
    this.defaultFolderDismissed = true;
    try {
      const collectionId = await this.addLocalFolder(
        selection,
        selection.displayName ?? musicFolderDisplayName(selection.folderPath),
        waitForRefresh,
      );
      this.detectedDefaultFolder = null;
      return collectionId;
    } finally {
      this.addingDefaultFolder = false;
    }
  }

  async chooseLocalFolder(): Promise<MusicLocalSourceSelection | null> {
    const selection = await this.api.pickFolder();
    if (!selection) return null;
    const existingPaths = this.bindings.flatMap((binding) => binding.folderPath ? [binding.folderPath] : []);
    return {
      selection,
      name: selection.displayName ?? musicFolderDisplayName(selection.folderPath),
      relationship: musicFolderRelationship(selection.folderPath, existingPaths),
    };
  }

  async addLocalFolder(selection: MediaFolderSelection, name: string, waitForRefresh = false): Promise<string> {
    if (!this.vaultId) throw new Error("The active Ganbaru AI folder is unavailable.");
    const trimmedName = name.trim();
    if (!trimmedName) throw new Error("Enter a source name.");
    const rootId = this.id();
    const collectionId = this.id();
    const createdAtMs = this.now();
    await this.api.createRoot({
      rootId,
      collectionId,
      identityKey: `local-root:${rootId}`,
      name: trimmedName,
      createdAtMs,
    });
    await this.api.bindRoot(this.vaultId, rootId, selection.folderPath);
    await this.loadProjection();
    const target = this.localTarget(collectionId, rootId, trimmedName, selection.folderPath, createdAtMs);
    const plan = this.refresh.prepare([target]);
    if (waitForRefresh) {
      const failure = refreshFailure(await this.runRefresh(plan, true));
      if (failure) {
        this.error = failure;
        throw new Error(failure);
      }
      notifyMusicLibraryChanged();
    } else {
      void this.runRefresh(plan, true)
        .then((statuses) => {
          const failure = refreshFailure(statuses);
          if (failure) this.error = failure;
          else notifyMusicLibraryChanged();
        })
        .catch((error: unknown) => { this.error = error instanceof Error ? error.message : String(error); });
    }
    return collectionId;
  }

  parseYouTubeInput(input: string):
    | { source: YouTubeVideoSource | YouTubePlaylistSource; error: null }
    | { source: null; error: string } {
    const parsed = parseMusicSourceInput(input);
    if (!parsed.source || parsed.error) return { source: null, error: parsed.error ?? "Enter a YouTube link." };
    if (parsed.source.kind !== "youtube-video" && parsed.source.kind !== "youtube-playlist") {
      return { source: null, error: "Enter a YouTube video or playlist link." };
    }
    return { source: parsed.source, error: null };
  }

  async renameCollection(collectionId: string, name: string): Promise<void> {
    const collection = this.collections.find((entry) => entry.id === collectionId);
    const trimmedName = name.trim();
    if (!collection) throw new Error("The music source is unavailable.");
    if (!trimmedName) throw new Error("Enter a source name.");
    await this.api.saveCollection({
      id: collection.id,
      kind: collection.kind,
      identityKey: collection.identityKey,
      name: trimmedName,
      localRootId: collection.localRootId,
      youtubePlaylistId: collection.youtubePlaylistId,
      updatedAtMs: this.now(),
    });
    await this.loadProjection();
    notifyMusicLibraryChanged();
  }

  async resolveYouTube(source: YouTubeVideoSource | YouTubePlaylistSource): Promise<MusicYouTubeSourcePreview | null> {
    this.cancelResolution();
    const controller = new AbortController();
    this.resolutionController = controller;
    this.resolving = true;
    this.resolutionError = null;
    this.youtubePreview = null;
    try {
      const resolved = await this.api.resolveYouTube(source, controller.signal);
      if (this.resolutionController !== controller) return null;
      const duplicateCount = await this.api.youtubeDuplicateCount(resolved.videoIds);
      if (this.resolutionController !== controller) return null;
      const preview = { ...resolved, duplicateCount };
      this.youtubePreview = preview;
      return preview;
    } catch (error) {
      if (this.resolutionController !== controller || controller.signal.aborted) return null;
      this.resolutionError = error instanceof Error ? error.message : String(error);
      return null;
    } finally {
      if (this.resolutionController === controller) {
        this.resolutionController = null;
        this.resolving = false;
      }
    }
  }

  cancelResolution(): void {
    this.resolutionController?.abort();
    this.resolutionController = null;
    this.resolving = false;
  }

  async addYouTube(preview: MusicYouTubeSourcePreview, name: string): Promise<string> {
    const resolvedAtMs = this.now();
    if (preview.kind === "youtube-video" && preview.videoId) {
      await this.api.saveYouTubeVideo({
        videoId: preview.videoId,
        title: preview.title,
        channel: preview.channel,
        durationMs: preview.durationMs,
        resolutionState: "ready",
        resolvedAtMs,
      });
      return preview.videoId;
    }
    if (!preview.playlistId || preview.videoIds.length === 0) throw new Error("The playlist has no available videos.");
    const collectionId = this.id();
    await this.api.saveYouTubePlaylist({
      collectionId,
      playlistId: preview.playlistId,
      name: name.trim() || preview.title || preview.playlistId,
      videoIds: preview.videoIds,
      videos: preview.videos
        .filter((video) => video.metadataResolved)
        .map(({ videoId, title, channel }) => ({ videoId, title, channel })),
      resolvedAtMs,
    });
    await this.load();
    return collectionId;
  }

  prepareRefresh(collectionIds?: readonly string[]): MusicSourceRefreshPlan {
    const selected = collectionIds ? new Set(collectionIds) : null;
    const bindings = new Map(this.bindings.map((binding) => [binding.rootId, binding]));
    const targets: MusicSourceRefreshTarget[] = [];
    for (const collection of this.collections) {
      if (selected && !selected.has(collection.id)) continue;
      if (collection.kind === "local-root" && collection.localRootId) {
        const binding = bindings.get(collection.localRootId);
        if (!binding?.folderPath) continue;
        targets.push(this.localTarget(collection.id, collection.localRootId, collection.name, binding.folderPath, this.now()));
      } else if (collection.kind === "youtube-playlist") {
        targets.push({ collectionId: collection.id, kind: "youtube-playlist", name: collection.name });
      }
    }
    return this.refresh.prepare(targets);
  }

  prepareUninitializedLocalRefresh(): MusicSourceRefreshPlan {
    const availableRootIds = new Set(this.bindings.flatMap((binding) =>
      binding.status === "available" && binding.folderPath ? [binding.rootId] : []));
    return this.prepareRefresh(this.collections.flatMap((collection) =>
      collection.kind === "local-root"
        && collection.localRootId
        && collection.discoveryEnabled
        && collection.snapshotGeneration === 0
        && availableRootIds.has(collection.localRootId)
        ? [collection.id]
        : []));
  }

  async runRefresh(plan: MusicSourceRefreshPlan, allowNetwork: boolean): Promise<MusicSourceRefreshStatus[]> {
    const statuses = await this.refresh.run(plan, { allowNetwork });
    const failure = refreshFailure(statuses);
    await this.loadProjection();
    if (failure) this.error = failure;
    return statuses;
  }

  cancelRefresh(collectionId?: string): Promise<void> { return this.refresh.cancel(collectionId); }

  async inspectRemoval(collectionId: string): Promise<MusicSourceRemovalImpact> {
    this.removalImpact = await this.api.removalImpact(collectionId);
    return this.removalImpact;
  }

  async forgetBinding(rootId: string): Promise<void> {
    if (!this.vaultId) throw new Error("The active Ganbaru AI folder is unavailable.");
    await this.api.clearBinding(this.vaultId, rootId);
    await this.load();
  }

  /** Replace an Android document-tree grant while preserving the logical source identity. */
  async reselectLocalRoot(collection: MusicSourceCollection): Promise<boolean> {
    if (!this.vaultId) throw new Error("The active Ganbaru AI folder is unavailable.");
    if (collection.kind !== "local-root" || !collection.localRootId) {
      throw new Error("The local music source is unavailable.");
    }
    const selection = await this.api.pickFolder();
    if (!selection) return false;
    await this.api.bindRoot(this.vaultId, collection.localRootId, selection.folderPath);
    await this.load();
    const target = this.localTarget(
      collection.id,
      collection.localRootId,
      collection.name,
      selection.folderPath,
      this.now(),
    );
    const failure = refreshFailure(await this.runRefresh(this.refresh.prepare([target]), true));
    if (failure) {
      this.error = failure;
      throw new Error(failure);
    }
    notifyMusicLibraryChanged();
    return true;
  }

  async chooseItemRepair(itemId: string): Promise<MusicItemRepairPreview | null> {
    const filePath = await this.api.pickFile();
    if (!filePath) return null;
    this.itemRepairPreview = await this.api.previewItemRepair(itemId, filePath);
    this.itemRepairApplied = null;
    return this.itemRepairPreview;
  }

  async applyItemRepair(acceptWeakMismatch: boolean): Promise<void> {
    if (!this.vaultId || !this.itemRepairPreview) throw new Error("No item repair preview is ready.");
    const preview = this.itemRepairPreview;
    const rootId = this.id();
    const locationId = this.id();
    const rootName = musicFolderDisplayName(preview.folderPath);
    await this.api.applyItemRepair({
      itemId: preview.itemId,
      rootId,
      locationId,
      rootName,
      folderPath: preview.folderPath,
      relativePath: preview.relativePath,
      expectedStrongFingerprint: preview.strongFingerprint,
      acceptWeakMismatch,
      appliedAt: this.now(),
    });
    await this.api.bindRoot(this.vaultId, rootId, preview.folderPath);
    this.itemRepairApplied = { locationId, rootId };
  }

  async undoItemRepair(): Promise<void> {
    if (!this.vaultId || !this.itemRepairApplied) return;
    const applied = this.itemRepairApplied;
    await this.api.undoItemRepair(applied.locationId, applied.rootId);
    await this.api.clearBinding(this.vaultId, applied.rootId);
    this.itemRepairApplied = null;
  }

  clearItemRepair(): void {
    this.itemRepairPreview = null;
    this.itemRepairApplied = null;
  }

  async confirmRemoval(collection: MusicSourceCollection, removeOrphanedItems: boolean): Promise<void> {
    const impact = this.removalImpact?.collectionId === collection.id
      ? this.removalImpact
      : await this.inspectRemoval(collection.id);
    await this.api.removeSource({
      collectionId: collection.id,
      expectedVersion: collection.version,
      expectedImpact: impact,
      removeOrphanedItems,
      removedAtMs: this.now(),
    });
    this.removalImpact = null;
    await this.load();
  }

  async planRelink(rootId: string, replacementFolderPath: string): Promise<MusicRelinkPlanSummary> {
    const plan = await this.api.createRelink({
      planId: this.id(),
      rootId,
      replacementFolderPath,
      createdAtMs: this.now(),
    });
    const entries = await this.api.relinkEntries(plan.id, 0, 500);
    this.relinkPlan = plan;
    this.relinkEntries = entries.entries;
    return plan;
  }

  async applyRelink(decisions: MusicRelinkDecision[]): Promise<void> {
    if (!this.relinkPlan) throw new Error("No relink plan is ready.");
    this.relinkPlan = await this.api.applyRelink({
      planId: this.relinkPlan.id,
      decisions,
      appliedAt: this.now(),
    });
    await this.load();
  }

  async cancelRelink(): Promise<void> {
    if (!this.relinkPlan) return;
    await this.api.cancelRelink(this.relinkPlan.id, this.now());
    this.relinkPlan = null;
    this.relinkEntries = [];
  }

  private localTarget(
    collectionId: string,
    rootId: string,
    name: string,
    folderPath: string,
    requestedAtMs: number,
  ): Extract<MusicSourceRefreshTarget, { kind: "local-root" }> {
    return {
      collectionId,
      kind: "local-root",
      name,
      request: {
        jobId: this.id(),
        rootId,
        collectionId,
        folderPath,
        availableRoots: this.bindings.flatMap((binding) => binding.folderPath
          ? [{ rootId: binding.rootId, folderPath: binding.folderPath }]
          : []),
        requestedAtMs,
      },
    };
  }
}

export function createMusicSourcesController(
  api: MusicSourcesControllerApi = defaultApi,
  now: () => number = Date.now,
  id: () => string = () => crypto.randomUUID(),
  refreshOverride: MusicSourceRefreshController | null = null,
): MusicSourcesController {
  return new MusicSourcesController(api, now, id, refreshOverride);
}

let sharedMusicSourcesController: MusicSourcesController | null = null;

/** Returns the process-wide source controller shared by startup discovery and the builder. */
export function getMusicSourcesController(): MusicSourcesController {
  sharedMusicSourcesController ??= createMusicSourcesController();
  return sharedMusicSourcesController;
}
