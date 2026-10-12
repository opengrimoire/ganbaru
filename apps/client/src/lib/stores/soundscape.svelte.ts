import {
  getDeviceId,
  getMusicSoundscapes,
  getMusicSoundscapeGroups,
  getSoundscapeSnapshot,
  getMusicSoundscapeState,
  pauseSoundscape,
  recoverSoundscape,
  removeMusicSoundscape,
  removeMusicSoundscapeGroup,
  resumeSoundscape,
  setSoundscapeVolume,
  setSoundscapeLevels,
  startSoundscape,
  stopSoundscape,
  updateMusicSoundscapeState,
  upsertMusicSoundscape,
  upsertMusicSoundscapeGroup,
} from "$lib/api/soundscape";
import type {
  MusicSoundscapeDefinition,
  MusicSoundscapeGroup,
  MusicSoundscapeGroupWrite,
  MusicSoundscapeSnapshot,
  MusicSoundscapeState,
  MusicSoundscapeWrite,
} from "$lib/music/soundscape/contracts";
import { MAX_SOUNDSCAPE_LAYERS } from "$lib/music/soundscape/contracts";
import { INITIAL_SOUNDSCAPE_VOLUME } from "$lib/music/soundscape/defaults";
import { soundSelectionAction } from "$lib/music/soundscape/selection";

class SoundscapeStore {
  definitions = $state<MusicSoundscapeDefinition[]>([]);
  groups = $state<MusicSoundscapeGroup[]>([]);
  persisted = $state<MusicSoundscapeState | null>(null);
  sectionLevels = $state<{ generated: number | null; local: number | null }>({ generated: null, local: null });
  snapshot = $state<MusicSoundscapeSnapshot>({ status: "idle", sourceId: null, volume: INITIAL_SOUNDSCAPE_VOLUME, errorCode: null });
  loading = $state(false);
  saving = $state(false);
  playbackPending = $state(false);
  modePending = $state(false);
  error = $state<string | null>(null);
  deviceId = $state<string | null>(null);
  private generation = 0;
  private volumeGeneration = 0;
  private volumePersistTimer: number | null = null;
  private volumeWriteQueue: Promise<void> = Promise.resolve();
  private levelsWriteQueue: Promise<void> = Promise.resolve();
  private levelsPersistTimer: number | null = null;
  private levelsGeneration = 0;
  private stateWriteQueue: Promise<void> = Promise.resolve();

  get activeDefinition(): MusicSoundscapeDefinition | null {
    return this.definitions.find((definition) => definition.id === this.persisted?.activeSoundscapeId) ?? null;
  }

  async initialize(): Promise<void> {
    const generation = ++this.generation;
    this.loading = true;
    this.error = null;
    try {
      const deviceId = await getDeviceId();
      const [definitions, persisted, groups, snapshot] = await Promise.all([
        getMusicSoundscapes(deviceId),
        getMusicSoundscapeState(),
        getMusicSoundscapeGroups(),
        getSoundscapeSnapshot(),
      ]);
      if (generation !== this.generation) return;
      this.deviceId = deviceId;
      this.definitions = definitions;
      this.groups = groups;
      this.persisted = persisted;
      this.sectionLevels = { generated: persisted.generatedLevel, local: persisted.localLevel };
      this.snapshot = snapshot;
      if (persisted.desiredPlaying && !persisted.automaticIntent && snapshot.status !== "playing") {
        const available = persisted.activeIds.filter((id) => definitions.some((definition) => definition.id === id && definition.availability === "available"));
        if (available.length > 0) await this.startSelection(available, available.length !== persisted.activeIds.length);
        else {
          this.error = "The selected background sound is unavailable.";
          await this.persistState(persisted.activeIds, false, persisted.volume);
        }
      }
    } catch (error) {
      if (generation === this.generation) this.error = message(error);
    } finally {
      if (generation === this.generation) this.loading = false;
    }
  }

  /** Refreshes presentation after an accepted native effect without issuing playback commands. */
  async refreshAcceptedOutput(): Promise<void> {
    const generation = this.generation;
    try {
      const [persisted, snapshot] = await Promise.all([
        getMusicSoundscapeState(),
        getSoundscapeSnapshot(),
      ]);
      if (generation !== this.generation || this.playbackPending || this.modePending
        || (this.persisted && this.persisted.version > persisted.version)) return;
      this.persisted = persisted;
      this.snapshot = snapshot;
      this.sectionLevels = { generated: persisted.generatedLevel, local: persisted.localLevel };
    } catch (error) {
      if (generation === this.generation) this.error = message(error);
    }
  }

  async play(id: string, persist = true): Promise<void> {
    await this.startSelection([id], persist);
  }

  async togglePlayback(): Promise<void> {
    if (this.playbackPending || this.modePending) return;
    if (this.snapshot.status === "playing") await this.pause();
    else if (this.snapshot.status === "paused") await this.resume();
    else if (this.persisted?.activeIds.length) await this.startSelection(this.persisted.activeIds);
  }

  async toggleSelection(id: string): Promise<void> {
    if (this.playbackPending || this.modePending) return;
    const action = soundSelectionAction(this.persisted?.activeIds ?? [], id, this.persisted?.multipleEnabled ?? false, this.snapshot.status);
    if (action.kind === "pause") await this.pause();
    else if (action.kind === "resume") await this.resume();
    else if (action.kind === "stop") await this.stop();
    else if (action.ids.length > MAX_SOUNDSCAPE_LAYERS) this.error = "Too many background sounds are selected.";
    else await this.startSelection(action.ids);
  }

  async setMultipleEnabled(enabled: boolean): Promise<void> {
    const current = this.persisted;
    if (!current || current.multipleEnabled === enabled || this.playbackPending || this.modePending) return;
    this.modePending = true;
    try {
      const ids = enabled ? current.activeIds : current.activeIds.slice(0, 1);
      if (!enabled && current.activeIds.length > 1 && current.desiredPlaying && ids[0]) {
        await this.startSelection(ids, true, false);
      } else if (!enabled && current.activeIds.length > 1) {
        this.snapshot = await stopSoundscape();
        await this.persistState(ids, false, current.volume, false);
      } else {
        await this.persistState(ids, current.desiredPlaying, current.volume, enabled);
      }
    } catch (error) {
      this.error = message(error);
    } finally {
      this.modePending = false;
    }
  }

  private async startSelection(ids: string[], persist = true, multipleEnabled = this.persisted?.multipleEnabled ?? false): Promise<void> {
    if (this.playbackPending) return;
    if (!multipleEnabled && ids.length > 1) {
      this.error = "Multiple background sounds are disabled.";
      return;
    }
    const definitions = ids.map((id) => this.definitions.find((entry) => entry.id === id));
    if (definitions.some((entry) => !entry || entry.availability !== "available") || ids.length === 0) {
      this.error = "This background sound needs to be repaired before it can play.";
      return;
    }
    const [first, ...extra] = definitions as MusicSoundscapeDefinition[];
    this.playbackPending = true;
    this.error = null;
    let started = false;
    try {
      this.snapshot = await startSoundscape({
        sourceId: first.id,
        generatedKind: first.generatedKind,
        localPath: first.localPath,
        level: this.levelFor(first),
        extraSources: extra.map((entry) => ({ sourceId: entry.id, generatedKind: entry.generatedKind, localPath: entry.localPath, level: this.levelFor(entry) })),
        volume: this.snapshot.volume,
      });
      started = true;
      if (persist) await this.persistState(ids, true, this.snapshot.volume, multipleEnabled);
    } catch (error) {
      this.error = message(error);
      try { this.snapshot = started ? await stopSoundscape() : await getSoundscapeSnapshot(); }
      catch { this.snapshot = { ...this.snapshot, status: "error", sourceId: first.id }; }
    } finally {
      this.playbackPending = false;
    }
  }

  async pause(): Promise<void> {
    if (this.playbackPending) return;
    this.playbackPending = true;
    try { this.snapshot = await pauseSoundscape(); await this.persistState(this.persisted?.activeIds ?? [], false, this.snapshot.volume); }
    catch (error) { this.error = message(error); }
    finally { this.playbackPending = false; }
  }

  async resume(): Promise<void> {
    if (this.playbackPending) return;
    this.playbackPending = true;
    try { this.snapshot = await resumeSoundscape(); await this.persistState(this.persisted?.activeIds ?? [], true, this.snapshot.volume); }
    catch (error) { this.error = message(error); }
    finally { this.playbackPending = false; }
  }

  async stop(): Promise<void> {
    if (this.playbackPending) return;
    this.playbackPending = true;
    try { this.snapshot = await stopSoundscape(); await this.persistState([], false, this.snapshot.volume); }
    catch (error) { this.error = message(error); }
    finally { this.playbackPending = false; }
  }

  async setVolume(volume: number): Promise<void> {
    const next = Math.min(1, Math.max(0, volume));
    const generation = ++this.volumeGeneration;
    this.snapshot = { ...this.snapshot, volume: next };
    this.volumeWriteQueue = this.volumeWriteQueue.catch(() => undefined).then(async () => {
      if (generation !== this.volumeGeneration) return;
      try {
        const snapshot = await setSoundscapeVolume(next);
        if (generation !== this.volumeGeneration) return;
        this.snapshot = { ...this.snapshot, volume: snapshot.volume };
        if (this.volumePersistTimer !== null) window.clearTimeout(this.volumePersistTimer);
        this.volumePersistTimer = window.setTimeout(() => {
          this.volumePersistTimer = null;
          void this.persistState(this.persisted?.activeIds ?? [], this.persisted?.desiredPlaying ?? false, next)
            .catch((error) => { this.error = message(error); });
        }, 180);
      } catch (error) {
        if (generation === this.volumeGeneration) this.error = message(error);
      }
    });
    await this.volumeWriteQueue;
  }

  private levelFor(definition: MusicSoundscapeDefinition): number {
    return (definition.sourceKind === "generated-noise" ? this.sectionLevels.generated : this.sectionLevels.local) ?? 1;
  }

  async setSectionLevel(section: "generated" | "local", level: number | null): Promise<void> {
    if (level !== null && (!Number.isFinite(level) || level < 0 || level > 2)) return;
    this.sectionLevels = { ...this.sectionLevels, [section]: level };
    const generation = ++this.levelsGeneration;
    this.levelsWriteQueue = this.levelsWriteQueue.catch(() => undefined).then(async () => {
      if (generation !== this.levelsGeneration) return;
      try {
        if (this.snapshot.status === "playing" || this.snapshot.status === "paused") {
          const levels = (this.persisted?.activeIds ?? []).map((id) => this.definitions.find((entry) => entry.id === id)).filter((entry): entry is MusicSoundscapeDefinition => entry !== undefined).map((entry) => this.levelFor(entry));
          if (levels.length) await setSoundscapeLevels(levels);
        }
      } catch (error) { if (generation === this.levelsGeneration) this.error = message(error); }
      if (generation !== this.levelsGeneration) return;
      if (this.levelsPersistTimer !== null) window.clearTimeout(this.levelsPersistTimer);
      this.levelsPersistTimer = window.setTimeout(() => {
        this.levelsPersistTimer = null;
        void this.persistState(this.persisted?.activeIds ?? [], this.persisted?.desiredPlaying ?? false, this.snapshot.volume)
          .catch((error) => { this.error = message(error); });
      }, 180);
    });
    await this.levelsWriteQueue;
  }

  async recover(): Promise<void> {
    try { this.snapshot = await recoverSoundscape(); this.error = null; }
    catch (error) { this.error = message(error); }
  }

  async switchVault(nextVaultId: string | null): Promise<void> {
    ++this.generation;
    ++this.volumeGeneration;
    ++this.levelsGeneration;
    if (this.volumePersistTimer !== null) {
      window.clearTimeout(this.volumePersistTimer);
      this.volumePersistTimer = null;
    }
    if (this.levelsPersistTimer !== null) { window.clearTimeout(this.levelsPersistTimer); this.levelsPersistTimer = null; }
    this.snapshot = { status: "idle", sourceId: null, volume: INITIAL_SOUNDSCAPE_VOLUME, errorCode: null };
    this.definitions = [];
    this.groups = [];
    this.persisted = null;
    this.sectionLevels = { generated: null, local: null };
    this.error = null;
    if (nextVaultId) await this.initialize();
  }

  async saveDefinition(write: Omit<MusicSoundscapeWrite, "deviceId">): Promise<void> {
    if (!this.deviceId) return;
    this.saving = true;
    this.error = null;
    try {
      const saved = await upsertMusicSoundscape({ ...write, deviceId: this.deviceId });
      this.definitions = [...this.definitions.filter((entry) => entry.id !== saved.id), saved]
        .sort((left, right) => left.createdAtMs - right.createdAtMs || left.name.localeCompare(right.name));
    } catch (error) { this.error = message(error); throw error; }
    finally { this.saving = false; }
  }

  async removeDefinition(definition: MusicSoundscapeDefinition): Promise<void> {
    try {
      if (this.persisted?.activeIds.includes(definition.id)) {
        if (this.persisted.multipleEnabled) await this.toggleSelection(definition.id);
        else await this.stop();
      }
      await removeMusicSoundscape(definition.id, definition.version);
      this.definitions = this.definitions.filter((entry) => entry.id !== definition.id);
    } catch (error) {
      this.error = message(error);
      throw error;
    }
  }

  async saveGroup(write: MusicSoundscapeGroupWrite): Promise<void> {
    this.saving = true;
    this.error = null;
    try {
      const saved = await upsertMusicSoundscapeGroup(write);
      this.groups = [...this.groups.filter((entry) => entry.id !== saved.id), saved]
        .sort((left, right) => left.name.localeCompare(right.name));
    } catch (error) { this.error = message(error); throw error; }
    finally { this.saving = false; }
  }

  async removeGroup(group: MusicSoundscapeGroup): Promise<void> {
    this.saving = true;
    this.error = null;
    try {
      await removeMusicSoundscapeGroup(group.id, group.version);
      this.groups = this.groups.filter((entry) => entry.id !== group.id);
      this.definitions = this.definitions.map((entry) => entry.groupId === group.id ? { ...entry, groupId: null } : entry);
    } catch (error) { this.error = message(error); throw error; }
    finally { this.saving = false; }
  }

  private async persistState(activeIds: string[], desiredPlaying: boolean, volume: number, multipleEnabled = this.persisted?.multipleEnabled ?? false): Promise<void> {
    const generation = this.generation;
    this.stateWriteQueue = this.stateWriteQueue.catch(() => undefined).then(async () => {
      if (generation !== this.generation) return;
      const current = this.persisted;
      if (!current) return;
      const saved = await updateMusicSoundscapeState({
        activeSoundscapeId: activeIds[0] ?? null,
        activeIds,
        multipleEnabled,
        generatedLevel: this.sectionLevels.generated,
        localLevel: this.sectionLevels.local,
        desiredPlaying,
        volume,
        expectedVersion: current.version,
        updatedAtMs: Date.now(),
      });
      if (generation === this.generation) this.persisted = saved;
    });
    await this.stateWriteQueue;
  }
}

function message(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error && typeof error.message === "string") return error.message;
  return error instanceof Error ? error.message : String(error);
}

let instance: SoundscapeStore | null = null;
export function getSoundscapeStore(): SoundscapeStore {
  instance ??= new SoundscapeStore();
  return instance;
}
