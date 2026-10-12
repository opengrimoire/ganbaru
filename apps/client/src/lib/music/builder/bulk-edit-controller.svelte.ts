import { applyMusicReviewSelection, bulkEditMusicMemberships, bulkSetMusicReviewState, bulkSnoozeMusicItems, createMusicPlaylist, getMusicInspectorDetail, getMusicMembershipMatrix, setMusicItemSignals } from "$lib/api/music-library";
import type { MusicLibraryController } from "$lib/music/library/controller.svelte";
import type { MusicItemSignal, MusicPlaylistSummary, MusicReviewState, MusicSnoozeScope, MusicWeight } from "$lib/music/library/contracts";

export type MusicBulkPlaylistState = "checked" | "mixed" | "unchecked";

export class MusicBulkEditController {
  itemIds = $state<string[]>([]);
  states = $state<Record<string, MusicBulkPlaylistState>>({});
  initialCounts = $state<Record<string, number>>({});
  touchedPlaylistIds = $state<Set<string>>(new Set());
  search = $state("");
  loading = $state(false);
  saving = $state(false);
  creatingPlaylist = $state(false);
  error = $state<string | null>(null);
  createError = $state<string | null>(null);
  selectionStale = $state(false);
  private openGeneration = 0;

  constructor(
    private readonly library: MusicLibraryController,
    private readonly now: () => number = Date.now,
    private readonly id: () => string = () => crypto.randomUUID(),
  ) {}

  get checkedIds(): Set<string> {
    return new Set(Object.entries(this.states).filter(([, state]) => state === "checked").map(([id]) => id));
  }

  get mixedIds(): Set<string> {
    return new Set(Object.entries(this.states).filter(([, state]) => state === "mixed").map(([id]) => id));
  }

  get membershipsChanged(): boolean {
    return Object.entries(this.states).some(([playlistId, state]) => {
      const initialCount = this.initialCounts[playlistId] ?? 0;
      return (state === "checked" && initialCount < this.itemIds.length)
        || (state === "unchecked" && initialCount > 0);
    });
  }

  get hasExistingMemberships(): boolean {
    return Object.values(this.initialCounts).some((count) => count > 0);
  }

  useSelection(itemIds: readonly string[]): void {
    this.itemIds = [...new Set(itemIds)];
    this.error = null;
    this.selectionStale = false;
  }

  async open(
    itemIds: readonly string[],
    playlists: readonly MusicPlaylistSummary[],
    preserveIntent = false,
  ): Promise<boolean> {
    const nextItemIds = [...new Set(itemIds)];
    const generation = ++this.openGeneration;
    if (!preserveIntent) this.touchedPlaylistIds = new Set();
    this.itemIds = nextItemIds;
    this.search = "";
    this.error = null;
    this.createError = null;
    this.selectionStale = false;
    this.loading = true;
    try {
      const matrix = await getMusicMembershipMatrix(nextItemIds);
      if (generation !== this.openGeneration) return false;
      const counts: Record<string, number> = {};
      for (const entry of matrix) counts[entry.playlistId] = (counts[entry.playlistId] ?? 0) + 1;
      this.initialCounts = counts;
      this.states = Object.fromEntries(playlists.map((playlist) => {
        const retained = preserveIntent && this.touchedPlaylistIds.has(playlist.id)
          ? this.states[playlist.id]
          : undefined;
        if (retained) return [playlist.id, retained];
        const count = counts[playlist.id] ?? 0;
        return [playlist.id, count === 0 ? "unchecked" : count === nextItemIds.length ? "checked" : "mixed"];
      }));
      return true;
    } catch (error) {
      if (generation !== this.openGeneration) return false;
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      if (generation === this.openGeneration) this.loading = false;
    }
  }

  toggle(playlistId: string): void {
    const state = this.states[playlistId] ?? "unchecked";
    const initialCount = this.initialCounts[playlistId] ?? 0;
    const initialState: MusicBulkPlaylistState = initialCount === 0
      ? "unchecked"
      : initialCount === this.itemIds.length ? "checked" : "mixed";
    const nextState = initialState === "mixed"
      ? state === "mixed" ? "checked" : state === "checked" ? "unchecked" : "mixed"
      : state === "checked" ? "unchecked" : "checked";
    this.states = {
      ...this.states,
      [playlistId]: nextState,
    };
    const nextTouched = new Set(this.touchedPlaylistIds);
    if (nextState === initialState) nextTouched.delete(playlistId);
    else nextTouched.add(playlistId);
    this.touchedPlaylistIds = nextTouched;
  }

  async createPlaylistAndSelect(nameInput: string, iconInput: string): Promise<string | null> {
    const name = nameInput.trim();
    if (!name || this.itemIds.length === 0 || this.creatingPlaylist) return null;
    this.creatingPlaylist = true;
    this.createError = null;
    const playlistId = this.id();
    try {
      await createMusicPlaylist({
        id: playlistId,
        name,
        icon: iconInput.trim(),
        shuffleEnabled: true,
        mixEnabled: false,
        repeatMode: "all",
        intendedUses: [],
        createdAtMs: this.now(),
      });
      if (!await this.library.refreshSummariesAfterMutation()) {
        throw this.library.error ?? new Error("The new playlist could not be loaded.");
      }
      this.initialCounts = { ...this.initialCounts, [playlistId]: 0 };
      this.states = { ...this.states, [playlistId]: "checked" };
      this.touchedPlaylistIds = new Set([...this.touchedPlaylistIds, playlistId]);
      return playlistId;
    } catch (error) {
      this.createError = error instanceof Error ? error.message : String(error);
      return null;
    } finally {
      this.creatingPlaylist = false;
    }
  }

  async saveMemberships(): Promise<boolean> {
    if (this.saving || this.itemIds.length === 0) return false;
    const addPlaylistIds: string[] = [];
    const removePlaylistIds: string[] = [];
    for (const [playlistId, state] of Object.entries(this.states)) {
      const initialCount = this.initialCounts[playlistId] ?? 0;
      if (state === "checked" && initialCount < this.itemIds.length) addPlaylistIds.push(playlistId);
      if (state === "unchecked" && initialCount > 0) removePlaylistIds.push(playlistId);
    }
    if (addPlaylistIds.length === 0 && removePlaylistIds.length === 0) return true;
    const saved = await this.persist({ addPlaylistIds, removePlaylistIds, weightPlaylistIds: [], weight: null });
    if (saved) {
      const nextCounts = { ...this.initialCounts };
      for (const playlistId of addPlaylistIds) nextCounts[playlistId] = this.itemIds.length;
      for (const playlistId of removePlaylistIds) nextCounts[playlistId] = 0;
      this.initialCounts = nextCounts;
    }
    return saved;
  }

  async saveReviewSelection(): Promise<boolean> {
    if (this.saving || this.itemIds.length === 0) return false;
    const addPlaylistIds: string[] = [];
    const removePlaylistIds: string[] = [];
    for (const [playlistId, state] of Object.entries(this.states)) {
      const initialCount = this.initialCounts[playlistId] ?? 0;
      if (state === "checked" && initialCount < this.itemIds.length) addPlaylistIds.push(playlistId);
      if (state === "unchecked" && initialCount > 0) removePlaylistIds.push(playlistId);
    }

    return this.applyReviewSelection("reviewed", addPlaylistIds, removePlaylistIds);
  }

  async ignoreReviewSelection(): Promise<boolean> {
    if (this.saving || this.itemIds.length === 0) return false;
    return this.applyReviewSelection("ignored", [], []);
  }

  private async applyReviewSelection(
    reviewState: "reviewed" | "ignored",
    addPlaylistIds: string[],
    removePlaylistIds: string[],
  ): Promise<boolean> {
    const itemIds = [...this.itemIds];
    const window = this.library.currentWindow;
    const itemsById = new Map(window.items.map((item) => [item.id, item]));
    const items = itemIds.flatMap((itemId) => {
      const item = itemsById.get(itemId);
      return item ? [{ itemId, expectedVersion: item.version }] : [];
    });
    if (items.length !== itemIds.length) {
      this.selectionStale = true;
      return false;
    }

    const updatedAtMs = this.now();
    const ignoredSnapshots = reviewState === "ignored"
      ? itemIds.flatMap((itemId) => {
          const item = itemsById.get(itemId);
          return item ? [{ item, reviewState: item.reviewState, updatedAtMs: item.updatedAtMs }] : [];
        })
      : [];
    this.saving = true;
    this.error = null;
    try {
      for (const snapshot of ignoredSnapshots) {
        snapshot.item.reviewState = "ignored";
        snapshot.item.updatedAtMs = updatedAtMs;
      }
      const result = await applyMusicReviewSelection({
        actionId: this.id(),
        items,
        reviewState,
        addPlaylistIds,
        removePlaylistIds,
        updatedAtMs,
      });
      const versions = new Map(result.items.map((receipt) => [receipt.id, receipt.version]));
      const versionedItemIds = itemIds.map((itemId) => {
        const version = versions.get(itemId);
        if (version === undefined) throw new Error(`The review result omitted item '${itemId}'.`);
        return { itemId, version };
      });
      if (reviewState === "reviewed") {
        for (const { itemId, version } of versionedItemIds) {
          const item = itemsById.get(itemId);
          if (!item) throw new Error(`The review window omitted item '${itemId}'.`);
          item.reviewState = "reviewed";
          item.updatedAtMs = updatedAtMs;
          item.version = version;
        }
      } else {
        for (const { itemId, version } of versionedItemIds) {
          const item = itemsById.get(itemId);
          if (!item) throw new Error(`The review window omitted item '${itemId}'.`);
          item.version = version;
        }
      }
      const nextCounts = { ...this.initialCounts };
      for (const playlistId of addPlaylistIds) nextCounts[playlistId] = items.length;
      for (const playlistId of removePlaylistIds) nextCounts[playlistId] = 0;
      this.initialCounts = nextCounts;
      this.touchedPlaylistIds = new Set();
      await this.library.refreshSummariesAfterMutation();
      return true;
    } catch (error) {
      for (const snapshot of ignoredSnapshots) {
        snapshot.item.reviewState = snapshot.reviewState;
        snapshot.item.updatedAtMs = snapshot.updatedAtMs;
      }
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      this.saving = false;
    }
  }

  async setWeight(playlistId: string, weight: MusicWeight): Promise<boolean> {
    return this.persist({ addPlaylistIds: [], removePlaylistIds: [], weightPlaylistIds: [playlistId], weight });
  }

  async removeFromPlaylist(playlistId: string): Promise<boolean> {
    return this.persist({ addPlaylistIds: [], removePlaylistIds: [playlistId], weightPlaylistIds: [], weight: null });
  }

  async setSignals(signals: MusicItemSignal[]): Promise<boolean> {
    if (this.saving || this.itemIds.length === 0) return false;
    this.saving = true;
    this.error = null;
    try {
      const prior = await Promise.all(this.itemIds.map(async (itemId) => ({ itemId, signals: (await getMusicInspectorDetail(itemId)).signals })));
      await this.library.runOptimistic({
        key: `bulk-signals:${this.itemIds.join(":")}`,
        label: "Describe selected tracks",
        apply: () => undefined,
        rollback: () => undefined,
        persist: () => setMusicItemSignals({ itemIds: this.itemIds, signals, updatedAtMs: this.now() }),
        undo: async () => {
          for (const entry of prior) await setMusicItemSignals({ itemIds: [entry.itemId], signals: entry.signals, updatedAtMs: this.now() });
          await this.library.refreshAfterMutation();
        },
      });
      await this.library.refreshAfterMutation();
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      this.saving = false;
    }
  }

  async setReviewState(reviewState: MusicReviewState): Promise<boolean> {
    if (this.saving || this.itemIds.length === 0) return false;
    const versions = new Map(this.library.currentWindow.items.map((item) => [item.id, item.version]));
    const items = this.itemIds.flatMap((itemId) => {
      const expectedVersion = versions.get(itemId);
      return expectedVersion === undefined ? [] : [{ itemId, expectedVersion }];
    });
    if (items.length !== this.itemIds.length) {
      this.selectionStale = true;
      return false;
    }
    this.saving = true;
    this.error = null;
    try {
      await bulkSetMusicReviewState({ items, reviewState, deferredUntil: null, updatedAtMs: this.now() });
      await this.library.refreshAfterMutation();
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      this.saving = false;
    }
  }

  async snooze(scope: MusicSnoozeScope, playlistId: string | null, endsAtMs: number | null): Promise<boolean> {
    if (this.saving || this.itemIds.length === 0) return false;
    const now = this.now();
    this.saving = true;
    this.error = null;
    try {
      await bulkSnoozeMusicItems({
        actionId: this.id(), itemIds: this.itemIds, scope, playlistId,
        startsAtMs: now, endsAtMs, reason: "", createdAtMs: now,
      });
      await this.library.refreshAfterMutation();
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      this.saving = false;
    }
  }

  clear(): void {
    this.openGeneration += 1;
    this.itemIds = [];
    this.states = {};
    this.initialCounts = {};
    this.touchedPlaylistIds = new Set();
    this.search = "";
    this.error = null;
    this.createError = null;
    this.selectionStale = false;
    this.loading = false;
  }

  private async persist(edit: {
    addPlaylistIds: string[];
    removePlaylistIds: string[];
    weightPlaylistIds: string[];
    weight: MusicWeight | null;
  }): Promise<boolean> {
    if (this.saving || this.itemIds.length === 0) return false;
    this.saving = true;
    this.error = null;
    try {
      await bulkEditMusicMemberships({
        actionId: this.id(),
        itemIds: this.itemIds,
        ...edit,
        updatedAtMs: this.now(),
      });
      await this.library.refreshAfterMutation();
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      this.saving = false;
    }
  }
}

export function createMusicBulkEditController(library: MusicLibraryController): MusicBulkEditController {
  return new MusicBulkEditController(library);
}
