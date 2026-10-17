import { getMusicPlaylistSummaries } from "$lib/api/music-library";
import type { MusicPlaylistSummary } from "$lib/music/library/contracts";

export interface MusicPlaylistSummaryCacheApi {
  summaries(nowMs: number, offset: number, limit: number): Promise<MusicPlaylistSummary[]>;
}

const defaultApi: MusicPlaylistSummaryCacheApi = {
  summaries: getMusicPlaylistSummaries,
};

export class MusicPlaylistSummaryCache {
  vaultId = $state<string | null>(null);
  playlists = $state<MusicPlaylistSummary[]>([]);
  loaded = $state(false);
  loading = $state(false);
  error = $state<string | null>(null);

  private generation = 0;
  private pending: Promise<boolean> | null = null;

  constructor(
    private readonly api: MusicPlaylistSummaryCacheApi = defaultApi,
    private readonly now: () => number = Date.now,
  ) {}

  setVault(vaultId: string | null): void {
    const normalized = vaultId?.trim() || null;
    if (normalized === this.vaultId) return;
    this.vaultId = normalized;
    this.generation += 1;
    this.pending = null;
    this.playlists = [];
    this.loaded = false;
    this.loading = false;
    this.error = null;
  }

  load(force = false): Promise<boolean> {
    if (!this.vaultId) return Promise.resolve(false);
    if (this.pending) return this.pending;
    if (this.loaded && !force) return Promise.resolve(true);

    const generation = ++this.generation;
    const vaultId = this.vaultId;
    this.loading = true;
    this.error = null;
    const task = (async (): Promise<boolean> => {
      try {
        const playlists = await this.api.summaries(this.now(), 0, 500);
        if (generation !== this.generation || vaultId !== this.vaultId) return false;
        this.playlists = playlists;
        this.loaded = true;
        return true;
      } catch (error) {
        if (generation !== this.generation || vaultId !== this.vaultId) return false;
        this.error = error instanceof Error ? error.message : String(error);
        return false;
      } finally {
        if (generation === this.generation && vaultId === this.vaultId) this.loading = false;
      }
    })();
    this.pending = task;
    void task.then(() => {
      if (this.pending === task) this.pending = null;
    });
    return task;
  }

  async refresh(): Promise<boolean> {
    if (this.pending) await this.pending;
    return this.load(true);
  }
}

const musicPlaylistSummaryCache = new MusicPlaylistSummaryCache();

export function getMusicPlaylistSummaryCache(): MusicPlaylistSummaryCache {
  return musicPlaylistSummaryCache;
}
