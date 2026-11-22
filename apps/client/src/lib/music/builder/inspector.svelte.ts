import { getMusicInspectorDetail } from "$lib/api/music-library";
import type { MusicInspectorDetail } from "$lib/music/library/contracts";

export interface MusicBuilderInspectorApi {
  detail(itemId: string): Promise<MusicInspectorDetail>;
}

const defaultApi: MusicBuilderInspectorApi = {
  detail: getMusicInspectorDetail,
};

const MAX_CACHED_DETAILS = 12;
const MAX_PREFETCH_BATCH = 6;

export class MusicBuilderInspectorController {
  itemId = $state<string | null>(null);
  detail = $state<MusicInspectorDetail | null>(null);
  busy = $state(false);
  error = $state<Error | null>(null);
  expandedSections = $state<Set<string>>(new Set(["details", "memberships"]));

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
