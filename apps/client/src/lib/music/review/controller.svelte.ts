import {
  createMusicPlaylist,
  removeMusicMemberships,
  setMusicReviewState,
  upsertMusicMemberships,
} from "$lib/api/music-library";
import type { MusicBuilderInspectorController } from "$lib/music/builder/inspector.svelte";
import type { MusicLibraryController } from "$lib/music/library/controller.svelte";
import { nextMusicWeight } from "$lib/music/review";
import type {
  MusicPlaylistMembership,
  MusicPlaylistSummary,
  MusicReviewState,
} from "$lib/music/library/contracts";

export class MusicReviewController {
  actionBusy = $state(false);
  creatingPlaylist = $state(false);
  createError = $state<string | null>(null);
  membershipBusy = $state<Set<string>>(new Set());
  membershipErrors = $state<Record<string, string>>({});

  constructor(
    private readonly library: MusicLibraryController,
    private readonly inspector: MusicBuilderInspectorController,
    private readonly now: () => number = Date.now,
    private readonly id: () => string = () => crypto.randomUUID(),
  ) {}

  membershipFor(playlistId: string): MusicPlaylistMembership | null {
    return this.inspector.detail?.memberships.find((membership) => membership.playlistId === playlistId) ?? null;
  }

  async toggleMembership(playlist: MusicPlaylistSummary): Promise<void> {
    const detail = this.inspector.detail;
    if (!detail) return;
    const existing = this.membershipFor(playlist.id);
    this.setMembershipBusy(playlist.id, true);
    this.membershipErrors = { ...this.membershipErrors, [playlist.id]: "" };
    try {
      if (existing) {
        const write = { ...existing, expectedVersion: null as number | null };
        await this.library.runOptimistic({
          key: `review-membership:${detail.item.id}:${playlist.id}`,
          label: `Remove ${playlist.name}`,
          apply: () => {
            detail.memberships = detail.memberships.filter((membership) => membership.id !== existing.id);
            this.changePlaylistCounts(playlist, -1);
          },
          rollback: () => {
            if (detail.memberships.some((membership) => membership.id === existing.id)) return;
            detail.memberships = [...detail.memberships, existing];
            this.changePlaylistCounts(playlist, 1);
          },
          persist: () => removeMusicMemberships({ membershipIds: [existing.id] }),
          undo: async () => {
            await upsertMusicMemberships({ memberships: [write] });
            if (detail.memberships.some((membership) => membership.id === existing.id)) return;
            detail.memberships = [...detail.memberships, existing];
            this.changePlaylistCounts(playlist, 1);
          },
        });
      } else {
        const timestamp = this.now();
        const membership: MusicPlaylistMembership = {
          id: this.id(), playlistId: playlist.id, itemId: detail.item.id,
          position: playlist.totalCount, weight: "normal", enabled: true,
          startMs: null, endMs: null, volume: null, rate: null,
          updatedAtMs: timestamp, createdAtMs: timestamp, version: 0,
        };
        const write = { ...membership, expectedVersion: null as number | null };
        const receipts = await this.library.runOptimistic({
          key: `review-membership:${detail.item.id}:${playlist.id}`,
          label: `Add ${playlist.name}`,
          apply: () => { detail.memberships = [...detail.memberships, membership]; this.changePlaylistCounts(playlist, 1); },
          rollback: () => { detail.memberships = detail.memberships.filter((entry) => entry.id !== membership.id); this.changePlaylistCounts(playlist, -1); },
          persist: () => upsertMusicMemberships({ memberships: [write] }),
          undo: async () => {
            await removeMusicMemberships({ membershipIds: [membership.id] });
            detail.memberships = detail.memberships.filter((entry) => entry.id !== membership.id);
            this.changePlaylistCounts(playlist, -1);
          },
        });
        membership.version = receipts[0]?.version ?? membership.version;
      }
    } catch (error) {
      this.recordMembershipError(playlist.id, error);
    } finally {
      this.setMembershipBusy(playlist.id, false);
    }
  }

  async cycleMembershipWeight(membership: MusicPlaylistMembership): Promise<void> {
    if (this.membershipBusy.has(membership.playlistId)) return;
    const previous = membership.weight;
    const next = nextMusicWeight(previous);
    this.setMembershipBusy(membership.playlistId, true);
    try {
      const receipts = await this.library.runOptimistic({
        key: `review-membership:${membership.itemId}:${membership.playlistId}`,
        label: `Change ${membership.playlistId} probability`,
        apply: () => { membership.weight = next; },
        rollback: () => { membership.weight = previous; },
        persist: () => upsertMusicMemberships({
          memberships: [{ ...membership, weight: next, expectedVersion: membership.version, updatedAtMs: this.now() }],
        }),
        undo: async () => {
          const [receipt] = await upsertMusicMemberships({
            memberships: [{ ...membership, weight: previous, expectedVersion: membership.version, updatedAtMs: this.now() }],
          });
          membership.weight = previous;
          membership.version = receipt?.version ?? membership.version;
        },
      });
      membership.version = receipts[0]?.version ?? membership.version;
    } catch (error) {
      this.recordMembershipError(membership.playlistId, error);
    } finally {
      this.setMembershipBusy(membership.playlistId, false);
    }
  }

  async clearMemberships(): Promise<void> {
    const detail = this.inspector.detail;
    if (!detail || detail.memberships.length === 0) return;
    const before = [...detail.memberships];
    const changeAllCounts = (delta: -1 | 1) => {
      for (const membership of before) {
        const playlist = this.library.playlistSummaries.find((entry) => entry.id === membership.playlistId);
        if (playlist) this.changePlaylistCounts(playlist, delta);
      }
    };
    try {
      await this.library.runOptimistic({
        key: `review-memberships:${detail.item.id}:clear`,
        label: "Clear playlist memberships",
        apply: () => { changeAllCounts(-1); detail.memberships = []; },
        rollback: () => { detail.memberships = before; changeAllCounts(1); },
        persist: () => removeMusicMemberships({ membershipIds: before.map((membership) => membership.id) }),
        undo: async () => {
          await upsertMusicMemberships({ memberships: before.map((membership) => ({ ...membership, expectedVersion: null })) });
          detail.memberships = before;
          changeAllCounts(1);
        },
      });
    } catch (error) {
      this.library.error = error instanceof Error ? error : new Error(String(error));
    }
  }

  async createPlaylistAndAdd(nameInput: string, iconInput: string): Promise<string | null> {
    const detail = this.inspector.detail;
    const name = nameInput.trim();
    if (!detail || !name || this.creatingPlaylist) return null;
    this.creatingPlaylist = true;
    this.createError = null;
    const playlistId = this.id();
    try {
      await createMusicPlaylist({
        id: playlistId, name, icon: iconInput.trim(), shuffleEnabled: true, mixEnabled: false,
        repeatMode: "all", intendedUses: [], createdAtMs: this.now(),
      });
      await this.library.refreshAfterMutation();
      const playlist = this.library.playlistSummaries.find((entry) => entry.id === playlistId);
      if (!playlist) throw new Error("The new playlist could not be loaded.");
      await this.toggleMembership(playlist);
      return playlistId;
    } catch (error) {
      this.createError = error instanceof Error ? error.message : String(error);
      return null;
    } finally {
      this.creatingPlaylist = false;
    }
  }

  async changeReviewState(
    reviewState: MusicReviewState,
    deferredUntil: number | null = null,
    nextItemId: string | null = null,
  ): Promise<boolean> {
    const detail = this.inspector.detail;
    if (!detail || this.actionBusy) return false;
    const previousReviewState = detail.item.reviewState;
    const previousDeferredUntil = detail.item.reviewDeferredUntilMs;
    const previousChangedAt = detail.item.reviewChangedAtMs;
    const previousUpdatedAt = detail.item.updatedAtMs;
    const previousSelectedItemId = this.library.currentState.selectedItemId;
    const listItem = this.library.currentWindow.items.find((item) => item.id === detail.item.id);
    const previousListState = listItem
      ? { reviewState: listItem.reviewState, updatedAtMs: listItem.updatedAtMs }
      : null;
    const updatedAtMs = this.now();
    this.actionBusy = true;
    detail.item.reviewState = reviewState;
    detail.item.reviewDeferredUntilMs = deferredUntil;
    detail.item.reviewChangedAtMs = updatedAtMs;
    detail.item.updatedAtMs = updatedAtMs;
    if (listItem) {
      listItem.reviewState = reviewState;
      listItem.updatedAtMs = updatedAtMs;
    }
    if (nextItemId) this.inspector.selectCached(nextItemId);
    this.library.selectItem(nextItemId);
    try {
      const receipt = await setMusicReviewState({
        itemId: detail.item.id, reviewState, deferredUntil,
        expectedVersion: detail.item.version, updatedAtMs,
      });
      detail.item.version = receipt.version;
      if (listItem) listItem.version = receipt.version;
      if (reviewState === "reviewed") {
        if (previousReviewState !== "reviewed") {
          for (const sourceId of detail.sourceCollectionIds) {
            const source = this.library.sourceSummaries.find((entry) => entry.id === sourceId);
            if (source) source.unreviewedCount = Math.max(0, source.unreviewedCount - 1);
          }
        }
        return true;
      }
      this.inspector.invalidate(detail.item.id);
      if (this.inspector.itemId === detail.item.id) this.inspector.clear();
      await this.library.refreshSummariesAfterMutation();
      return true;
    } catch (error) {
      detail.item.reviewState = previousReviewState;
      detail.item.reviewDeferredUntilMs = previousDeferredUntil;
      detail.item.reviewChangedAtMs = previousChangedAt;
      detail.item.updatedAtMs = previousUpdatedAt;
      if (listItem && previousListState) {
        listItem.reviewState = previousListState.reviewState;
        listItem.updatedAtMs = previousListState.updatedAtMs;
      }
      this.inspector.clear();
      this.inspector.itemId = detail.item.id;
      this.inspector.detail = detail;
      this.library.selectItem(previousSelectedItemId);
      this.library.error = error instanceof Error ? error : new Error(String(error));
      return false;
    } finally {
      this.actionBusy = false;
    }
  }

  private changePlaylistCounts(playlist: MusicPlaylistSummary, delta: -1 | 1): void {
    const detail = this.inspector.detail;
    if (!detail) return;
    playlist.totalCount = Math.max(0, playlist.totalCount + delta);
    if (detail.item.availability === "available") playlist.eligibleCount = Math.max(0, playlist.eligibleCount + delta);
    else playlist.unavailableCount = Math.max(0, playlist.unavailableCount + delta);
    if (detail.item.sourceKind === "local-file") playlist.localCount = Math.max(0, playlist.localCount + delta);
    else playlist.onlineCount = Math.max(0, playlist.onlineCount + delta);
  }

  private setMembershipBusy(key: string, busy: boolean): void {
    const next = new Set(this.membershipBusy);
    if (busy) next.add(key);
    else next.delete(key);
    this.membershipBusy = next;
  }

  private recordMembershipError(playlistId: string, error: unknown): void {
    const message = error instanceof Error ? error.message : String(error);
    this.library.error = error instanceof Error ? error : new Error(message);
    this.membershipErrors = { ...this.membershipErrors, [playlistId]: message };
  }
}

export function createMusicReviewController(
  library: MusicLibraryController,
  inspector: MusicBuilderInspectorController,
): MusicReviewController {
  return new MusicReviewController(library, inspector);
}
