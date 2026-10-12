import { getMusicInspectorDetail, saveMusicAdvancedMembership, setMusicItemSignals, setMusicMetadataOverrides } from "$lib/api/music-library";
import type { MusicInspectorDetail, MusicItemSignal, MusicMembershipSkipRange, MusicPlaylistMembership } from "$lib/music/library/contracts";

export interface MusicBuilderInspectorApi {
  detail(itemId: string): Promise<MusicInspectorDetail>;
  setSignals(itemId: string, signals: MusicItemSignal[], updatedAtMs: number): Promise<number>;
}

const defaultApi: MusicBuilderInspectorApi = {
  detail: getMusicInspectorDetail,
  async setSignals(itemId, signals, updatedAtMs) {
    const [receipt] = await setMusicItemSignals({ itemIds: [itemId], signals, updatedAtMs });
    if (!receipt) throw new Error("The signal update did not return a receipt.");
    return receipt.version;
  },
};

const MAX_CACHED_DETAILS = 12;
const MAX_PREFETCH_BATCH = 6;

export class MusicBuilderInspectorController {
  itemId = $state<string | null>(null);
  detail = $state<MusicInspectorDetail | null>(null);
  busy = $state(false);
  error = $state<Error | null>(null);
  saving = $state(false);
  expandedSections = $state<Set<string>>(new Set(["details", "memberships"]));
  signalUndo = $state<MusicItemSignal[] | null>(null);

  private generation = 0;
  private cacheEpoch = 0;
  private readonly detailCache = new Map<string, MusicInspectorDetail>();
  private readonly pendingDetails = new Map<string, Promise<MusicInspectorDetail>>();
  private readonly api: MusicBuilderInspectorApi;

  constructor(api: MusicBuilderInspectorApi = defaultApi) {
    this.api = api;
  }

  async select(itemId: string | null): Promise<boolean> {
    if (!itemId) {
      this.clear();
      return true;
    }
    if (this.itemId === itemId && this.detail) return true;
    const generation = ++this.generation;
    this.itemId = itemId;
    this.error = null;
    const cached = this.readCachedDetail(itemId);
    if (cached) {
      this.detail = cached;
      this.busy = false;
      return true;
    }
    this.busy = true;
    try {
      const detail = await this.loadDetail(itemId);
      if (generation !== this.generation || this.itemId !== itemId) return false;
      this.detail = detail;
      return true;
    } catch (error) {
      if (generation !== this.generation || this.itemId !== itemId) return false;
      this.detail = null;
      this.error = error instanceof Error ? error : new Error(String(error));
      return false;
    } finally {
      if (generation === this.generation) this.busy = false;
    }
  }

  /** Switch synchronously to an already-prefetched builder detail. */
  selectCached(itemId: string): boolean {
    const cached = this.readCachedDetail(itemId);
    if (!cached) return false;
    this.generation += 1;
    this.itemId = itemId;
    this.detail = cached;
    this.busy = false;
    this.error = null;
    return true;
  }

  /** Preload a small set of likely builder selections without changing presentation state. */
  async prefetch(itemIds: readonly string[]): Promise<MusicInspectorDetail[]> {
    const uniqueIds = [...new Set(itemIds.filter((itemId) => itemId.trim()))]
      .slice(0, MAX_PREFETCH_BATCH);
    const results = await Promise.allSettled(uniqueIds.map((itemId) => this.loadDetail(itemId)));
    return results.flatMap((result) => result.status === "fulfilled" ? [result.value] : []);
  }

  clear(): void {
    this.generation += 1;
    this.itemId = null;
    this.detail = null;
    this.error = null;
    this.busy = false;
  }

  /** Clear selection and all builder-only cached details when the active vault changes. */
  reset(): void {
    this.cacheEpoch += 1;
    this.detailCache.clear();
    this.pendingDetails.clear();
    this.clear();
  }

  invalidate(itemId: string): void {
    this.detailCache.delete(itemId);
  }

  toggleSection(section: string): void {
    const next = new Set(this.expandedSections);
    if (next.has(section)) next.delete(section);
    else next.add(section);
    this.expandedSections = next;
  }

  async saveMetadataOverrides(overrides: {
    titleOverride: string | null;
    artistOverride: string | null;
    albumOverride: string | null;
    artworkOverride: string | null;
  }): Promise<boolean> {
    const detail = this.detail;
    if (!detail || this.saving) return false;
    this.saving = true;
    this.error = null;
    try {
      const receipt = await setMusicMetadataOverrides({
        itemId: detail.item.id,
        ...overrides,
        expectedVersion: detail.item.version,
        updatedAtMs: Date.now(),
      });
      Object.assign(detail.item, overrides, { version: receipt.version });
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error : new Error(String(error));
      return false;
    } finally {
      this.saving = false;
    }
  }

  async saveSignals(signals: MusicItemSignal[]): Promise<boolean> {
    const detail = this.detail;
    if (!detail || this.saving) return false;
    const previousSignals = [...detail.signals];
    this.saving = true;
    this.error = null;
    detail.signals = [...signals];
    try {
      detail.item.version = await this.api.setSignals(detail.item.id, signals, Date.now());
      this.signalUndo = previousSignals;
      return true;
    } catch (error) {
      detail.signals = previousSignals;
      this.error = error instanceof Error ? error : new Error(String(error));
      return false;
    } finally {
      this.saving = false;
    }
  }

  async undoSignals(): Promise<boolean> {
    const detail = this.detail;
    const previous = this.signalUndo;
    if (!detail || !previous || this.saving) return false;
    this.saving = true;
    this.error = null;
    try {
      detail.item.version = await this.api.setSignals(detail.item.id, previous, Date.now());
      detail.signals = [...previous];
      this.signalUndo = null;
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error : new Error(String(error));
      return false;
    } finally {
      this.saving = false;
    }
  }

  async saveAdvancedMembership(
    membership: MusicPlaylistMembership,
    skipRanges: MusicMembershipSkipRange[],
  ): Promise<boolean> {
    if (this.saving) return false;
    this.saving = true;
    this.error = null;
    try {
      const receipt = await saveMusicAdvancedMembership({
        membership: { ...membership, expectedVersion: membership.version, updatedAtMs: Date.now() },
        skipRanges,
      });
      membership.version = receipt.version;
      if (this.detail) {
        this.detail.membershipSkipRanges = [
          ...this.detail.membershipSkipRanges.filter((range) => range.membershipId !== membership.id),
          ...skipRanges,
        ];
      }
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error : new Error(String(error));
      return false;
    } finally {
      this.saving = false;
    }
  }

  private readCachedDetail(itemId: string): MusicInspectorDetail | null {
    const cached = this.detailCache.get(itemId);
    if (!cached) return null;
    this.detailCache.delete(itemId);
    this.detailCache.set(itemId, cached);
    return cached;
  }

  private cacheDetail(detail: MusicInspectorDetail): void {
    this.detailCache.delete(detail.item.id);
    this.detailCache.set(detail.item.id, detail);
    while (this.detailCache.size > MAX_CACHED_DETAILS) {
      const oldestId = this.detailCache.keys().next().value;
      if (typeof oldestId !== "string") break;
      this.detailCache.delete(oldestId);
    }
  }

  private loadDetail(itemId: string): Promise<MusicInspectorDetail> {
    const cached = this.readCachedDetail(itemId);
    if (cached) return Promise.resolve(cached);
    const pending = this.pendingDetails.get(itemId);
    if (pending) return pending;
    const epoch = this.cacheEpoch;
    const request = this.api.detail(itemId).then((detail) => {
      if (epoch === this.cacheEpoch) this.cacheDetail(detail);
      return detail;
    }).finally(() => {
      if (this.pendingDetails.get(itemId) === request) this.pendingDetails.delete(itemId);
    });
    this.pendingDetails.set(itemId, request);
    return request;
  }
}

export function createMusicBuilderInspectorController(
  api: MusicBuilderInspectorApi = defaultApi,
): MusicBuilderInspectorController {
  return new MusicBuilderInspectorController(api);
}
