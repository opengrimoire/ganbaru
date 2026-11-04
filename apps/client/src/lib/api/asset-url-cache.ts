import {
  onActiveVaultIdentityChange,
  requireActiveVaultIdentity,
} from "$lib/vault/active-vault";

const ASSET_URL_CACHE_MAX_ENTRIES = 96;
const ASSET_URL_CACHE_MAX_ENCODED_BYTES = 16 * 1024 * 1024;
const CACHE_KEY_SEPARATOR = "\u0000";

export type AssetUrlKind =
  | "music-artwork"
  | "notes-page-icon"
  | "notes-page-cover"
  | "notes-file"
  | "profile-image"
  | "project-icon";

export interface ByteAwareLruCacheOptions<Value> {
  readonly maxEntries: number;
  readonly maxBytes: number;
  readonly sizeOf: (value: Value) => number;
}

export interface ByteAwareLruCacheStats {
  readonly entries: number;
  readonly bytes: number;
  readonly inFlight: number;
}

interface CacheEntry<Value> {
  readonly value: Value;
  readonly bytes: number;
}

interface InFlightLoad<Value> {
  cancelled: boolean;
  promise: Promise<Value> | null;
}

/** A fixed-entry, fixed-byte LRU with single-flight asynchronous loading. */
export class ByteAwareLruCache<Key, Value> {
  private readonly entries = new Map<Key, CacheEntry<Value>>();
  private readonly inFlight = new Map<Key, InFlightLoad<Value>>();
  private totalBytes = 0;

  constructor(private readonly options: ByteAwareLruCacheOptions<Value>) {
    if (!Number.isInteger(options.maxEntries) || options.maxEntries < 1) {
      throw new Error("LRU maxEntries must be a positive integer");
    }
    if (!Number.isFinite(options.maxBytes) || options.maxBytes < 1) {
      throw new Error("LRU maxBytes must be positive");
    }
  }

  /** Reads and promotes a retained value. */
  get(key: Key): Value | undefined {
    const entry = this.entries.get(key);
    if (!entry) return undefined;
    this.entries.delete(key);
    this.entries.set(key, entry);
    return entry.value;
  }

  /** Returns a retained value or shares one asynchronous loader for the key. */
  async load(key: Key, loader: () => Promise<Value>): Promise<Value> {
    const cached = this.get(key);
    if (cached !== undefined) return cached;
    const activeLoad = this.inFlight.get(key);
    if (activeLoad?.promise) return activeLoad.promise;

    const pending: InFlightLoad<Value> = {
      cancelled: false,
      promise: null,
    };
    const promise = Promise.resolve()
      .then(loader)
      .then((value) => {
        if (!pending.cancelled) this.set(key, value);
        return value;
      })
      .finally(() => {
        if (this.inFlight.get(key) === pending) this.inFlight.delete(key);
      });
    pending.promise = promise;
    this.inFlight.set(key, pending);
    return promise;
  }

  /** Removes one retained value and prevents its current load from being retained. */
  invalidate(key: Key): void {
    this.deleteEntry(key);
    const pending = this.inFlight.get(key);
    if (!pending) return;
    pending.cancelled = true;
    this.inFlight.delete(key);
  }

  /** Invalidates every retained or loading key accepted by the predicate. */
  invalidateWhere(predicate: (key: Key) => boolean): void {
    for (const key of [...this.entries.keys()]) {
      if (predicate(key)) this.deleteEntry(key);
    }
    for (const [key, pending] of [...this.inFlight.entries()]) {
      if (!predicate(key)) continue;
      pending.cancelled = true;
      this.inFlight.delete(key);
    }
  }

  /** Removes all retained values and cancels retention for every current load. */
  clear(): void {
    this.entries.clear();
    this.totalBytes = 0;
    for (const pending of this.inFlight.values()) pending.cancelled = true;
    this.inFlight.clear();
  }

  /** Returns current bounded-cache accounting for diagnostics and tests. */
  stats(): ByteAwareLruCacheStats {
    return {
      entries: this.entries.size,
      bytes: this.totalBytes,
      inFlight: this.inFlight.size,
    };
  }

  private set(key: Key, value: Value): void {
    this.deleteEntry(key);
    const measuredBytes = this.options.sizeOf(value);
    const bytes = Number.isFinite(measuredBytes) ? Math.max(0, measuredBytes) : 0;
    if (bytes > this.options.maxBytes) return;
    this.entries.set(key, { value, bytes });
    this.totalBytes += bytes;
    this.evictToLimits();
  }

  private deleteEntry(key: Key): void {
    const entry = this.entries.get(key);
    if (!entry) return;
    this.entries.delete(key);
    this.totalBytes = Math.max(0, this.totalBytes - entry.bytes);
  }

  private evictToLimits(): void {
    while (
      this.entries.size > this.options.maxEntries
      || this.totalBytes > this.options.maxBytes
    ) {
      const oldestKey = this.entries.keys().next().value as Key | undefined;
      if (oldestKey === undefined) break;
      this.deleteEntry(oldestKey);
    }
  }
}

/** Builds a stable cache key from vault, asset kind, and managed relative path. */
export function assetUrlCacheKey(
  vaultId: string,
  kind: AssetUrlKind,
  relativePath: string,
): string {
  return [vaultId.trim(), kind, relativePath.trim()].join(CACHE_KEY_SEPARATOR);
}

function keyHasKind(key: string, kind: AssetUrlKind): boolean {
  return key.split(CACHE_KEY_SEPARATOR, 3)[1] === kind;
}

const assetUrlCache = new ByteAwareLruCache<string, string>({
  maxEntries: ASSET_URL_CACHE_MAX_ENTRIES,
  maxBytes: ASSET_URL_CACHE_MAX_ENCODED_BYTES,
  sizeOf: (value) => value.length,
});

onActiveVaultIdentityChange(() => assetUrlCache.clear());

/** Loads one vault-scoped asset URL through the shared bounded cache. */
export function loadAssetUrl(
  kind: AssetUrlKind,
  relativePath: string,
  loader: () => Promise<string>,
): Promise<string> {
  const vaultId = requireActiveVaultIdentity();
  const key = assetUrlCacheKey(vaultId, kind, relativePath);
  return assetUrlCache.load(key, loader).then((value) => {
    if (requireActiveVaultIdentity() !== vaultId) {
      throw new Error("asset URL load belongs to an inactive vault");
    }
    return value;
  });
}

/** Invalidates one asset path in the active vault. */
export function invalidateAssetUrl(kind: AssetUrlKind, relativePath: string): void {
  const key = assetUrlCacheKey(requireActiveVaultIdentity(), kind, relativePath);
  assetUrlCache.invalidate(key);
}

/** Invalidates all URLs of one asset kind across retained vault keys. */
export function invalidateAssetUrlKind(kind: AssetUrlKind): void {
  assetUrlCache.invalidateWhere((key) => keyHasKind(key, kind));
}

/** Clears all managed Notes URL kinds after page or block cleanup. */
export function invalidateNotesAssetUrls(): void {
  invalidateAssetUrlKind("notes-page-icon");
  invalidateAssetUrlKind("notes-page-cover");
  invalidateAssetUrlKind("notes-file");
}

/** Clears all cached URLs and prevents current in-flight loads from being retained. */
export function clearAssetUrlCache(): void {
  assetUrlCache.clear();
}
