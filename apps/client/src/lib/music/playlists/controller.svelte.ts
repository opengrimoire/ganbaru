import {
  createMusicPlaylist,
  deleteMusicPlaylist,
  duplicateMusicPlaylist,
  getMusicPlaylist,
  getMusicPlaylistDeleteImpact,
  getMusicPlaylistPlaybackEntries,
  updateMusicPlaylist,
} from "$lib/api/music-library";
import { musicErrorMessage } from "$lib/music/user-errors";
import { translate } from "$lib/i18n/translator.svelte";
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

/** The read of one playlist detail. */
type MusicPlaylistLoad = { phase: "idle" } | { phase: "loading"; token: symbol };

/** A delete impact, tied to the playlist it was read for. */
interface MusicPlaylistDeleteInspection {
  playlistId: string;
  impact: MusicPlaylistDeleteImpact;
}

export class MusicPlaylistController {
  detail = $state<MusicPlaylist | null>(null);
  saving = $state(false);
  error = $state<string | null>(null);
  playbackProjection = $state<MusicPlaylistPlaybackProjection | null>(null);
  playbackIssue = $state<"no-eligible-items" | null>(null);
  private loadRequest = $state.raw<MusicPlaylistLoad>({ phase: "idle" });
  private deleteInspection = $state.raw<MusicPlaylistDeleteInspection | null>(null);

  constructor(
    private readonly library: MusicLibraryController,
    private readonly now: () => number = Date.now,
    private readonly id: () => string = () => crypto.randomUUID(),
  ) {}

  /** Whether a playlist detail is loading. */
  get busy(): boolean {
    return this.loadRequest.phase === "loading";
  }

  /** The delete impact read for the shown playlist, or null before it is inspected. */
  get deleteImpact(): MusicPlaylistDeleteImpact | null {
    const inspection = this.deleteInspection;
    return inspection && inspection.playlistId === this.detail?.id ? inspection.impact : null;
  }

  async load(playlistId: string | null): Promise<boolean> {
    if (!playlistId) { this.clear(); return true; }
    if (this.detail?.id === playlistId) {
      this.loadRequest = { phase: "idle" };
      return true;
    }
    const token = Symbol(playlistId);
    this.loadRequest = { phase: "loading", token };
    const isCurrent = () => this.loadRequest.phase === "loading" && this.loadRequest.token === token;
    this.error = null;
    try {
      const detail = await getMusicPlaylist(playlistId);
      if (!isCurrent()) return false;
      this.detail = detail;
      return true;
    } catch (error) {
      if (!isCurrent()) return false;
      this.error = musicErrorMessage(error, translate("music.errors.playlistLoadFailed"));
      this.detail = null;
      return false;
    } finally {
      if (isCurrent()) this.loadRequest = { phase: "idle" };
    }
  }

  clear(): void {
    this.loadRequest = { phase: "idle" };
    this.detail = null;
    this.deleteInspection = null;
    this.error = null;
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
      this.error = musicErrorMessage(error, translate("music.errors.playlistCreateFailed"));
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
      this.error = musicErrorMessage(error, translate("music.errors.playlistSaveFailed"));
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
      this.error = musicErrorMessage(error, translate("music.errors.playlistDuplicateFailed"));
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
      const impact = await getMusicPlaylistDeleteImpact(detail.id);
      if (this.detail?.id !== detail.id) return false;
      this.deleteInspection = { playlistId: detail.id, impact };
      return true;
    } catch (error) {
      if (this.detail?.id !== detail.id) return false;
      this.error = musicErrorMessage(error, translate("music.errors.playlistDeleteFailed"));
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
      this.error = musicErrorMessage(error, translate("music.errors.playlistPlayFailed"));
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
      this.error = musicErrorMessage(error, translate("music.errors.playlistDeleteFailed"));
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
