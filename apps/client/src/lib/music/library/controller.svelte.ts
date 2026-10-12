import {
  getMusicIssues,
  getMusicItemWindow,
  getMusicPlaylistSummaries,
  getMusicSourceSummaries,
} from "$lib/api/music-library";
import type {
  MusicGroupBy,
  MusicIssue,
  MusicItemAvailability,
  MusicItemListEntry,
  MusicItemSort,
  MusicItemWindow,
  MusicItemWindowRequest,
  MusicLibrarySourceKind,
  MusicPlaylistSummary,
  MusicReviewState,
  MusicSortDirection,
  MusicSourceSummary,
} from "./contracts";

export type MusicBuilderLocation =
  | { kind: "review" }
  | { kind: "playlists" }
  | { kind: "library" }
  | { kind: "playlist"; playlistId: string }
  | { kind: "sources" }
  | { kind: "soundscapes" };

export interface MusicDestinationState {
  search: string;
  sourceKind: MusicLibrarySourceKind | null;
  availability: MusicItemAvailability | null;
  reviewState: MusicReviewState | null;
  sourceCollectionId: string | null;
  membershipPlaylistId: string | null;
  snoozed: boolean | null;
  sort: MusicItemSort;
  direction: MusicSortDirection;
  groupBy: MusicGroupBy;
  offset: number;
  limit: number;
  scrollTop: number;
  selectedItemId: string | null;
  selectedItemIds: string[];
}

export interface MusicLibraryControllerApi {
  itemWindow(request: MusicItemWindowRequest): Promise<MusicItemWindow>;
  playlistSummaries(nowMs: number, offset: number, limit: number): Promise<MusicPlaylistSummary[]>;
  sourceSummaries(nowMs: number, offset: number, limit: number): Promise<MusicSourceSummary[]>;
  issues(offset: number, limit: number): Promise<MusicIssue[]>;
}

export interface OptimisticMutation<T> {
  key: string;
  label: string;
  apply(): void;
  rollback(): void;
  persist(): Promise<T>;
  undo?(): Promise<void>;
}

interface UndoEntry {
  label: string;
  run(): Promise<void>;
}

const defaultApi: MusicLibraryControllerApi = {
  itemWindow: getMusicItemWindow,
  playlistSummaries: getMusicPlaylistSummaries,
  sourceSummaries: getMusicSourceSummaries,
  issues: getMusicIssues,
};

const emptyWindow: MusicItemWindow = {
  items: [],
  groups: [],
  totalCount: 0,
  offset: 0,
  limit: 50,
};

function locationKey(location: MusicBuilderLocation): string {
  return location.kind === "playlist" ? `playlist:${location.playlistId}` : location.kind;
}

function defaultDestinationState(location: MusicBuilderLocation): MusicDestinationState {
  return {
    search: "",
    sourceKind: null,
    availability: null,
    reviewState: null,
    sourceCollectionId: null,
    membershipPlaylistId: null,
    snoozed: null,
    sort: location.kind === "review" ? "discovered-at" : "title",
    direction: "ascending",
    groupBy: "none",
    offset: 0,
    limit: 200,
    scrollTop: 0,
    selectedItemId: null,
    selectedItemIds: [],
  };
}

function hasItemWindow(location: MusicBuilderLocation): boolean {
  return location.kind === "review" || location.kind === "library" || location.kind === "playlist";
}

function itemWindowRequest(
  location: MusicBuilderLocation,
  state: MusicDestinationState,
  nowMs: number,
): MusicItemWindowRequest {
  return {
    destination: location.kind === "playlist" ? "playlist" : location.kind === "review" ? "review" : "library",
    playlistId: location.kind === "playlist" ? location.playlistId : null,
    search: state.search,
    sourceKind: state.sourceKind,
    availability: state.availability,
    reviewState: state.reviewState,
    sourceCollectionId: state.sourceCollectionId,
    membershipPlaylistId: state.membershipPlaylistId,
    snoozed: state.snoozed,
    sort: state.sort,
    direction: state.direction,
    groupBy: state.groupBy,
    nowMs,
    offset: state.offset,
    limit: state.limit,
  };
}

export class MusicLibraryController {
  vaultId = $state<string | null>(null);
  location = $state<MusicBuilderLocation>({ kind: "review" });
  destinationStates = $state<Record<string, MusicDestinationState>>({
    review: defaultDestinationState({ kind: "review" }),
    playlists: defaultDestinationState({ kind: "playlists" }),
    library: defaultDestinationState({ kind: "library" }),
    sources: defaultDestinationState({ kind: "sources" }),
    soundscapes: defaultDestinationState({ kind: "soundscapes" }),
  });
  windows = $state<Record<string, MusicItemWindow>>({});
  playlistSummaries = $state<MusicPlaylistSummary[]>([]);
  sourceSummaries = $state<MusicSourceSummary[]>([]);
  issues = $state<MusicIssue[]>([]);
  busy = $state(false);
  loadingMore = $state(false);
  loadMoreError = $state<Error | null>(null);
  error = $state<Error | null>(null);
  undoCount = $state(0);
  lastUndoLabel = $state<string | null>(null);

  currentKey = $derived(locationKey(this.location));
  currentState = $derived(this.destinationStates[this.currentKey] ?? defaultDestinationState(this.location));
  currentWindow = $derived(this.windows[this.currentKey] ?? emptyWindow);
  selectedItem = $derived.by<MusicItemListEntry | null>(() => {
    const selectedId = this.currentState.selectedItemId;
    if (!selectedId) return null;
    return this.currentWindow.items.find((item) => item.id === selectedId) ?? null;
  });

  private readonly api: MusicLibraryControllerApi;
  private readonly now: () => number;
  private refreshGeneration = 0;
  private staleWindowKeys = new Set<string>();
  private mutationRevisions: Record<string, number> = {};
  private mutationTails: Record<string, Promise<void>> = {};
  private undoEntries: UndoEntry[] = [];

  constructor(api: MusicLibraryControllerApi = defaultApi, now: () => number = Date.now) {
    this.api = api;
    this.now = now;
  }

  setVault(vaultId: string | null): void {
    const normalized = vaultId?.trim() || null;
    if (normalized === this.vaultId) return;
    this.vaultId = normalized;
    this.refreshGeneration += 1;
    this.windows = {};
    this.staleWindowKeys.clear();
    this.playlistSummaries = [];
    this.sourceSummaries = [];
    this.issues = [];
    this.error = null;
    this.busy = false;
    this.loadingMore = false;
    this.loadMoreError = null;
    this.mutationRevisions = {};
    this.mutationTails = {};
    this.undoEntries = [];
    this.syncUndoProjection();
    for (const state of Object.values(this.destinationStates)) {
      state.selectedItemId = null;
      state.selectedItemIds = [];
    }
  }

  navigate(location: MusicBuilderLocation): void {
    const key = locationKey(location);
    if (!this.destinationStates[key]) {
      this.destinationStates[key] = defaultDestinationState(location);
    }
    this.location = location;
  }

  patchCurrentState(patch: Partial<MusicDestinationState>): void {
    const key = this.currentKey;
    const current = this.destinationStates[key] ?? defaultDestinationState(this.location);
    this.destinationStates[key] = { ...current, ...patch };
  }

  selectItem(itemId: string | null): void {
    this.patchCurrentState({
      selectedItemId: itemId,
      selectedItemIds: itemId ? [itemId] : [],
    });
  }

  setItemSelection(itemIds: readonly string[], activeItemId: string | null): void {
    const uniqueIds = [...new Set(itemIds.filter((itemId) => itemId.trim()))];
    this.patchCurrentState({
      selectedItemIds: uniqueIds,
      selectedItemId: activeItemId && uniqueIds.includes(activeItemId)
        ? activeItemId
        : uniqueIds.at(-1) ?? null,
    });
  }

  toggleItemSelection(itemId: string): void {
    const selected = new Set(this.currentState.selectedItemIds);
    if (selected.has(itemId)) selected.delete(itemId);
    else selected.add(itemId);
    this.setItemSelection([...selected], itemId);
  }

  selectItemRange(anchorItemId: string, itemId: string): void {
    const ids = this.currentWindow.items.map((item) => item.id);
    const anchor = ids.indexOf(anchorItemId);
    const target = ids.indexOf(itemId);
    if (anchor < 0 || target < 0) {
      this.selectItem(itemId);
      return;
    }
    const start = Math.min(anchor, target);
    const end = Math.max(anchor, target);
    this.setItemSelection(ids.slice(start, end + 1), itemId);
  }

  setScrollTop(scrollTop: number): void {
    this.patchCurrentState({ scrollTop: Math.max(0, scrollTop) });
  }

  private async completeReviewWindow(
    window: MusicItemWindow,
    state: MusicDestinationState,
    nowMs: number,
    generation: number,
    vaultId: string,
  ): Promise<boolean> {
    const reviewLocation = { kind: "review" } as const;
    while (window.items.length < window.totalCount) {
      const next = await this.api.itemWindow(itemWindowRequest(reviewLocation, {
        ...state,
        offset: window.items.length,
      }, nowMs));
      if (!this.isCurrent(generation, vaultId)) return false;
      const knownIds = new Set(window.items.map((item) => item.id));
      const nextItems = next.items.filter((item) => !knownIds.has(item.id));
      if (nextItems.length === 0) throw new Error("The complete Review list could not be loaded.");
      window.items = [...window.items, ...nextItems];
      window.totalCount = next.totalCount;
      window.groups = next.groups;
    }
    return true;
  }

  /** Loads shared builder summaries plus the Review and Library windows in one initialization pass. */
  async preloadCoreDestinations(): Promise<boolean> {
    if (!this.vaultId) return false;
    const generation = ++this.refreshGeneration;
    const vaultId = this.vaultId;
    const reviewLocation = { kind: "review" } as const;
    const libraryLocation = { kind: "library" } as const;
    const reviewState = { ...(this.destinationStates.review ?? defaultDestinationState(reviewLocation)) };
    const libraryState = { ...(this.destinationStates.library ?? defaultDestinationState(libraryLocation)) };
    const nowMs = this.now();
    this.busy = true;
    this.error = null;
    try {
      const [reviewWindow, libraryWindow, playlists, sources, issues] = await Promise.all([
        this.api.itemWindow(itemWindowRequest(reviewLocation, reviewState, nowMs)),
        this.api.itemWindow(itemWindowRequest(libraryLocation, libraryState, nowMs)),
        this.api.playlistSummaries(nowMs, 0, 500),
        this.api.sourceSummaries(nowMs, 0, 500),
        this.api.issues(0, 500),
      ]);
      if (!this.isCurrent(generation, vaultId)) return false;
      if (!await this.completeReviewWindow(reviewWindow, reviewState, nowMs, generation, vaultId)) return false;
      this.windows.review = reviewWindow;
      this.windows.library = libraryWindow;
      this.staleWindowKeys.delete("review");
      this.staleWindowKeys.delete("library");
      this.playlistSummaries = playlists;
      this.sourceSummaries = sources;
      this.issues = issues;
      return true;
    } catch (error) {
      if (!this.isCurrent(generation, vaultId)) return false;
      this.error = error instanceof Error ? error : new Error(String(error));
      return false;
    } finally {
      if (this.isCurrent(generation, vaultId)) this.busy = false;
    }
  }

  /** Loads the active item window only when the destination has not been visited or preloaded. */
  ensureCurrentDestination(): Promise<boolean> {
    if (!hasItemWindow(this.location) || (this.windows[this.currentKey] && !this.staleWindowKeys.has(this.currentKey))) return Promise.resolve(true);
    return this.refresh();
  }

  /** Marks retained item windows for background refresh the next time each destination is opened. */
  markRetainedWindowsStale(): void {
    for (const key of Object.keys(this.windows)) this.staleWindowKeys.add(key);
  }

  /** Refreshes the current view after a canonical mutation while retaining other windows on screen. */
  refreshAfterMutation(): Promise<boolean> {
    this.markRetainedWindowsStale();
    return this.refresh();
  }

  /** Refreshes shared counts after a local window was updated without replacing its visible rows. */
  async refreshSummariesAfterMutation(): Promise<boolean> {
    if (!this.vaultId) return false;
    this.markRetainedWindowsStale();
    const generation = ++this.refreshGeneration;
    const vaultId = this.vaultId;
    const nowMs = this.now();
    this.error = null;
    try {
      const [playlists, sources, issues] = await Promise.all([
        this.api.playlistSummaries(nowMs, 0, 500),
        this.api.sourceSummaries(nowMs, 0, 500),
        this.api.issues(0, 500),
      ]);
      if (!this.isCurrent(generation, vaultId)) return false;
      this.playlistSummaries = playlists;
      this.sourceSummaries = sources;
      this.issues = issues;
      return true;
    } catch (error) {
      if (!this.isCurrent(generation, vaultId)) return false;
      this.error = error instanceof Error ? error : new Error(String(error));
      return false;
    }
  }

  async refresh(): Promise<boolean> {
    if (!this.vaultId) return false;
    const generation = ++this.refreshGeneration;
    const vaultId = this.vaultId;
    const location = this.location;
    const key = locationKey(location);
    const state = { ...(this.destinationStates[key] ?? defaultDestinationState(location)) };
    const reviewLocation = { kind: "review" } as const;
    const reviewState = { ...(this.destinationStates.review ?? defaultDestinationState(reviewLocation)) };
    const nowMs = this.now();
    this.busy = true;
    this.error = null;
    try {
      const [window, sourceReviewWindow, playlists, sources, issues] = await Promise.all([
        hasItemWindow(location)
          ? this.api.itemWindow(itemWindowRequest(location, state, nowMs))
          : Promise.resolve(null),
        location.kind === "sources"
          ? this.api.itemWindow(itemWindowRequest(reviewLocation, reviewState, nowMs))
          : Promise.resolve(null),
        this.api.playlistSummaries(nowMs, 0, 500),
        this.api.sourceSummaries(nowMs, 0, 500),
        this.api.issues(0, 500),
      ]);
      if (!this.isCurrent(generation, vaultId)) return false;
      if (window && location.kind === "review"
        && !await this.completeReviewWindow(window, state, nowMs, generation, vaultId)) return false;
      if (sourceReviewWindow
        && !await this.completeReviewWindow(sourceReviewWindow, reviewState, nowMs, generation, vaultId)) return false;
      if (window) {
        this.windows[key] = window;
        this.staleWindowKeys.delete(key);
      }
      if (sourceReviewWindow) {
        this.windows.review = sourceReviewWindow;
        this.staleWindowKeys.delete("review");
      }
      this.playlistSummaries = playlists;
      this.sourceSummaries = sources;
      this.issues = issues;
      return true;
    } catch (error) {
      if (!this.isCurrent(generation, vaultId)) return false;
      this.error = error instanceof Error ? error : new Error(String(error));
      return false;
    } finally {
      if (this.isCurrent(generation, vaultId)) this.busy = false;
    }
  }

  async loadMore(): Promise<boolean> {
    if (!this.vaultId || this.loadingMore || this.busy || !hasItemWindow(this.location)) return false;
    const current = this.currentWindow;
    if (current.items.length >= current.totalCount) return false;
    const vaultId = this.vaultId;
    const location = this.location;
    const key = this.currentKey;
    const state = { ...this.currentState, offset: current.items.length };
    this.loadingMore = true;
    this.loadMoreError = null;
    try {
      const next = await this.api.itemWindow(itemWindowRequest(location, state, this.now()));
      if (this.vaultId !== vaultId || this.currentKey !== key) return false;
      const known = new Set(current.items.map((item) => item.id));
      current.items = [...current.items, ...next.items.filter((item) => !known.has(item.id))];
      current.totalCount = next.totalCount;
      current.groups = next.groups;
      return true;
    } catch (error) {
      if (this.vaultId !== vaultId || this.currentKey !== key) return false;
      this.loadMoreError = error instanceof Error ? error : new Error(String(error));
      return false;
    } finally {
      if (this.vaultId === vaultId && this.currentKey === key) this.loadingMore = false;
    }
  }

  /** Loads every remaining page for the active destination before it is presented. */
  async loadAllCurrentItems(): Promise<boolean> {
    while (this.currentWindow.items.length < this.currentWindow.totalCount) {
      const previousCount = this.currentWindow.items.length;
      if (!await this.loadMore() || this.currentWindow.items.length <= previousCount) return false;
    }
    return true;
  }

  async runOptimistic<T>(mutation: OptimisticMutation<T>): Promise<T> {
    const revision = (this.mutationRevisions[mutation.key] ?? 0) + 1;
    this.mutationRevisions[mutation.key] = revision;
    mutation.apply();
    const previous = this.mutationTails[mutation.key] ?? Promise.resolve();
    const persistence = previous
      .catch(() => undefined)
      .then(mutation.persist);
    const tail = persistence.then(() => undefined, () => undefined);
    this.mutationTails[mutation.key] = tail;
    try {
      const result = await persistence;
      if (this.mutationRevisions[mutation.key] === revision && mutation.undo) {
        this.pushUndo(mutation.label, mutation.undo);
      }
      return result;
    } catch (error) {
      if (this.mutationRevisions[mutation.key] === revision) mutation.rollback();
      throw error;
    } finally {
      if (this.mutationTails[mutation.key] === tail) delete this.mutationTails[mutation.key];
    }
  }

  async undoLast(): Promise<boolean> {
    const entry = this.undoEntries.pop();
    this.syncUndoProjection();
    if (!entry) return false;
    await entry.run();
    return true;
  }

  clearUndo(): void {
    this.undoEntries = [];
    this.syncUndoProjection();
  }

  private isCurrent(generation: number, vaultId: string): boolean {
    return generation === this.refreshGeneration && vaultId === this.vaultId;
  }

  private pushUndo(label: string, run: () => Promise<void>): void {
    this.undoEntries.push({ label, run });
    if (this.undoEntries.length > 50) this.undoEntries.splice(0, this.undoEntries.length - 50);
    this.syncUndoProjection();
  }

  private syncUndoProjection(): void {
    this.undoCount = this.undoEntries.length;
    this.lastUndoLabel = this.undoEntries.at(-1)?.label ?? null;
  }
}

export function createMusicLibraryController(
  api: MusicLibraryControllerApi = defaultApi,
  now: () => number = Date.now,
): MusicLibraryController {
  return new MusicLibraryController(api, now);
}
