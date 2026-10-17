import {
  createMusicPlaylist,
  deleteMusicPlaylist,
  duplicateMusicPlaylist,
  getMusicPlaylist,
  getMusicPlaylistDeleteImpact,
  getMusicPlaylistPlaybackEntries,
  updateMusicPlaylist,
} from "$lib/api/music-library";
import { getMusicPlayer } from "$lib/stores/music-player.svelte";
import { projectMusicPlaylistPlayback, type MusicPlaylistPlaybackProjection } from "./playback";
import type { MusicLibraryController } from "$lib/music/library/controller.svelte";
import type {
  MusicIntendedUse,
  MusicPlaylist,
  MusicPlaylistDeleteImpact,
  MusicRepeatMode,
  LocalRootBinding,
} from "$lib/music/library/contracts";

export interface MusicPlaylistDraft {
  name: string;
  icon: string;
  shuffleEnabled: boolean;
  mixEnabled: boolean;
  repeatMode: MusicRepeatMode;
  intendedUses: MusicIntendedUse[];
}

export class MusicPlaylistController {
  detail = $state<MusicPlaylist | null>(null);
  busy = $state(false);
  saving = $state(false);
  error = $state<string | null>(null);
  deleteImpact = $state<MusicPlaylistDeleteImpact | null>(null);
  playbackProjection = $state<MusicPlaylistPlaybackProjection | null>(null);
  playbackIssue = $state<"no-eligible-items" | null>(null);
  private loadGeneration = 0;

  constructor(
    private readonly library: MusicLibraryController,
    private readonly now: () => number = Date.now,
    private readonly id: () => string = () => crypto.randomUUID(),
  ) {}

  async load(playlistId: string | null): Promise<boolean> {
    if (!playlistId) { this.clear(); return true; }
    if (this.detail?.id === playlistId) return true;
    const generation = ++this.loadGeneration;
    this.busy = true;
    this.error = null;
    try {
      const detail = await getMusicPlaylist(playlistId);
      if (generation !== this.loadGeneration) return false;
      this.detail = detail;
      return true;
    } catch (error) {
      if (generation !== this.loadGeneration) return false;
      this.error = error instanceof Error ? error.message : String(error);
      this.detail = null;
      return false;
    } finally {
      if (generation === this.loadGeneration) this.busy = false;
    }
  }

  clear(): void {
    this.loadGeneration += 1;
    this.detail = null;
    this.deleteImpact = null;
    this.error = null;
    this.busy = false;
  }

  async create(draft: MusicPlaylistDraft): Promise<string | null> {
    if (this.saving || !draft.name.trim()) return null;
    this.saving = true;
    this.error = null;
    const playlistId = this.id();
    try {
      await createMusicPlaylist({
        id: playlistId,
        ...normalizedDraft(draft),
        createdAtMs: this.now(),
      });
      await this.library.refreshAfterMutation();
      await this.load(playlistId);
      return playlistId;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return null;
    } finally {
      this.saving = false;
    }
  }

  async update(draft: MusicPlaylistDraft): Promise<boolean> {
    const detail = this.detail;
    if (!detail || this.saving || !draft.name.trim()) return false;
    const previous = { ...detail, intendedUses: [...detail.intendedUses] };
    const next = normalizedDraft(draft);
    this.saving = true;
    this.error = null;
    try {
      const receipt = await this.library.runOptimistic({
        key: `playlist:${detail.id}:details`,
        label: `Edit ${previous.name}`,
        apply: () => { Object.assign(detail, next); this.patchSummary(detail.id, next); },
        rollback: () => { Object.assign(detail, previous); this.patchSummary(detail.id, previous); },
        persist: () => updateMusicPlaylist({
          id: detail.id, ...next, expectedVersion: previous.version, updatedAtMs: this.now(),
        }),
        undo: async () => {
          const receipt = await updateMusicPlaylist({
            id: previous.id, name: previous.name, icon: previous.icon,
            shuffleEnabled: previous.shuffleEnabled, mixEnabled: previous.mixEnabled, repeatMode: previous.repeatMode,
            intendedUses: previous.intendedUses, expectedVersion: detail.version, updatedAtMs: this.now(),
          });
          Object.assign(detail, previous, { version: receipt.version });
          this.patchSummary(detail.id, previous);
        },
      });
      detail.version = receipt.version;
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      this.saving = false;
    }
  }

  async duplicate(name: string): Promise<string | null> {
    const detail = this.detail;
    if (!detail || this.saving || !name.trim()) return null;
    this.saving = true;
    this.error = null;
    const playlistId = this.id();
    try {
      await duplicateMusicPlaylist({
        sourcePlaylistId: detail.id,
        newPlaylistId: playlistId,
        name: name.trim(),
        createdAtMs: this.now(),
      });
      await this.library.refreshAfterMutation();
      return playlistId;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return null;
    } finally {
      this.saving = false;
    }
  }

  async inspectDelete(): Promise<boolean> {
    const detail = this.detail;
    if (!detail) return false;
    this.error = null;
    try {
      this.deleteImpact = await getMusicPlaylistDeleteImpact(detail.id);
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    }
  }

  async play(bindings: readonly LocalRootBinding[], explicitItemId: string | null = null): Promise<boolean> {
    const detail = this.detail;
    if (!detail || this.saving) return false;
    this.saving = true;
    this.error = null;
    this.playbackIssue = null;
    try {
      const entries = await getMusicPlaylistPlaybackEntries(detail.id, this.now());
      const player = getMusicPlayer();
      const projection = projectMusicPlaylistPlayback(entries, bindings, {
        nowMs: this.now(),
        online: player.online,
        explicitItemId,
      });
      this.playbackProjection = projection;
      const loaded = await player.loadSavedPlaylist(
        detail.id,
        detail.name,
        projection.entries,
        detail.shuffleEnabled,
        detail.repeatMode,
        detail.mixEnabled,
        {
          explicitItemId,
          structuralSkipped: projection.structuralSkipped,
        },
      );
      if (!loaded) this.playbackIssue = "no-eligible-items";
      return loaded;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      this.saving = false;
    }
  }

  async remove(replacementPlaylistId: string | null): Promise<boolean> {
    const detail = this.detail;
    const impact = this.deleteImpact;
    if (!detail || !impact || this.saving) return false;
    this.saving = true;
    this.error = null;
    try {
      await deleteMusicPlaylist({ playlistId: detail.id, replacementPlaylistId, expectedVersion: detail.version, expectedImpact: impact });
      this.clear();
      await this.library.refreshAfterMutation();
      return true;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      this.saving = false;
    }
  }

  async refreshActivePlayback(bindings: readonly LocalRootBinding[]): Promise<void> {
    const detail = this.detail;
    const player = getMusicPlayer();
    if (!detail || player.activePlaylistId !== detail.id) return;
    const entries = await getMusicPlaylistPlaybackEntries(detail.id, this.now());
    const projection = projectMusicPlaylistPlayback(entries, bindings, {
      nowMs: this.now(),
      online: player.online,
    });
    player.reconcileSavedPlaylist(projection.entries, projection.structuralSkipped);
  }

  private patchSummary(playlistId: string, draft: MusicPlaylistDraft): void {
    const summary = this.library.playlistSummaries.find((entry) => entry.id === playlistId);
    if (!summary) return;
    summary.name = draft.name;
    summary.icon = draft.icon;
    summary.shuffleEnabled = draft.shuffleEnabled;
    summary.mixEnabled = draft.mixEnabled;
    summary.repeatMode = draft.repeatMode;
    summary.intendedUses = [...draft.intendedUses];
  }
}

function normalizedDraft(draft: MusicPlaylistDraft): MusicPlaylistDraft {
  return {
    name: draft.name.trim(),
    icon: draft.icon.trim(),
    shuffleEnabled: draft.shuffleEnabled,
    mixEnabled: draft.mixEnabled,
    repeatMode: draft.repeatMode,
    intendedUses: [...new Set(draft.intendedUses)],
  };
}

export function createMusicPlaylistController(library: MusicLibraryController): MusicPlaylistController {
  return new MusicPlaylistController(library);
}
