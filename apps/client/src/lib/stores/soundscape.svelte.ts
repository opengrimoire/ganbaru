import {
  getMusicSoundscapes,
  getMusicSoundscapePresets,
  getSoundscapeSnapshot,
  getMusicSoundscapeState,
  pauseSoundscape,
  recoverSoundscape,
  removeMusicSoundscape,
  removeMusicSoundscapePreset,
  resumeSoundscape,
  saveMusicSoundscapePresetLevel,
  setSoundscapeLevels,
  setSoundscapePresetLevel,
  startSoundscape,
  stopSoundscape,
  updateMusicSoundscapeState,
  upsertMusicSoundscape,
  upsertMusicSoundscapePreset,
} from "$lib/api/soundscape";
import type {
  MusicSoundscapeDefinition,
  MusicSoundscapePreset,
  MusicSoundscapePresetSound,
  MusicSoundscapePresetWrite,
  MusicSoundscapeSnapshot,
  MusicSoundscapeStartRequest,
  MusicSoundscapeState,
  MusicSoundscapeWrite,
} from "$lib/music/soundscape/contracts";
import {
  DEFAULT_SOUNDSCAPE_PRESET_LEVEL,
  MAX_SOUNDSCAPE_LAYERS,
  MAX_SOUNDSCAPE_PRESET_LEVEL,
} from "$lib/music/soundscape/contracts";
import { DEFAULT_PRESET_SOUND_LEVEL } from "$lib/music/soundscape/presets";

/** Delay before a dragged preset level is saved, so a slider gesture writes once. */
const PRESET_LEVEL_SAVE_DELAY_MS = 180;
const IDLE_SNAPSHOT: MusicSoundscapeSnapshot = { status: "idle", sourceId: null, errorCode: null };

/** Sounds and level of a preset or preset draft, which is all playback needs. */
export type SoundscapeMix = Pick<MusicSoundscapePreset, "sounds" | "level">;

/** One available sound of a mix with its share of the preset level. */
interface MixLayer {
  definition: MusicSoundscapeDefinition;
  level: number;
}

/** What the output holds: the chosen preset, or a layer playing on its own outside every preset. */
type OutputSource = { kind: "preset"; presetId: string } | { kind: "layer"; layerId: string };

/** Output state an isolated preview needs to hand playback back unchanged. */
interface SoundscapePreview {
  /** Whether the saved preset was playing when the preview began. */
  resumes: boolean;
  /** Whether the preview replaced the paused output, so resuming it is no longer possible. */
  replacedOutput: boolean;
  /** Layers and level the preview output holds now, in start order. */
  playing: { ids: string[]; levels: number[]; level: number } | null;
}

class SoundscapeStore {
  definitions = $state<MusicSoundscapeDefinition[]>([]);
  presets = $state<MusicSoundscapePreset[]>([]);
  persisted = $state<MusicSoundscapeState | null>(null);
  snapshot = $state<MusicSoundscapeSnapshot>(IDLE_SNAPSHOT);
  loading = $state(false);
  saving = $state(false);
  playbackPending = $state(false);
  error = $state<string | null>(null);
  /** True once the sound catalog has loaded, so edits apply to a known library. */
  catalogReady = $state(false);
  private generation = 0;
  private catalogLoaded = false;
  private stateWriteQueue: Promise<void> = Promise.resolve();
  private levelGeneration = 0;
  private levelQueue: Promise<void> = Promise.resolve();
  private levelSaveTimers = new Map<string, number>();
  private preview = $state<SoundscapePreview | null>(null);
  private previewTarget: SoundscapeMix | null = null;
  private previewQueue: Promise<void> = Promise.resolve();
  /**
   * What the output holds, written together with each snapshot it describes so the two never disagree. A layer
   * playing on its own belongs to no soundscape: it is never saved and stops instead of pausing.
   */
  private source = $state<OutputSource | null>(null);

  /** Preset the soundscape plays or last played. */
  get activePreset(): MusicSoundscapePreset | null {
    const id = this.persisted?.activePresetId;
    return id ? this.presets.find((preset) => preset.id === id) ?? null : null;
  }

  /** Preset playing now; null while output is silent, a layer plays on its own, or an isolated preview plays. */
  get playingPreset(): MusicSoundscapePreset | null {
    if (this.preview || this.snapshot.status !== "playing" || this.source?.kind !== "preset") return null;
    const id = this.source.presetId;
    return this.presets.find((preset) => preset.id === id) ?? null;
  }

  /** Layer playing on its own now, if any; never a layer of a soundscape or of a preview. */
  get playingLayerId(): string | null {
    if (this.preview || this.snapshot.status !== "playing" || this.source?.kind !== "layer") return null;
    return this.source.layerId;
  }

  /** Whether a layer on its own holds the output, playing or still starting. */
  private get layerHoldsOutput(): boolean {
    return this.source?.kind === "layer" && this.snapshot.status !== "idle";
  }

  async initialize(): Promise<void> {
    const generation = ++this.generation;
    this.loading = true;
    this.error = null;
    try {
      const [definitions, persisted, presets, snapshot] = await Promise.all([
        getMusicSoundscapes(),
        getMusicSoundscapeState(),
        getMusicSoundscapePresets(),
        getSoundscapeSnapshot(),
      ]);
      if (generation !== this.generation) return;
      this.catalogReady = true;
      this.definitions = definitions;
      this.presets = presets;
      this.persisted = persisted;
      this.snapshot = snapshot;
      this.source = presetSource(persisted.activePresetId);
      // Like Music, a soundscape the listener started stays chosen but silent after a launch or vault switch.
      if (persisted.desiredPlaying && !persisted.automaticIntent && snapshot.status !== "playing") {
        await this.persistState(persisted.activePresetId, false);
      }
    } catch (error) {
      if (generation === this.generation) this.error = message(error);
    } finally {
      if (generation === this.generation) this.loading = false;
    }
  }

  /**
   * Loads layers and soundscapes for pickers in windows that do not own playback.
   * It never reads or restores playback state, so detached windows cannot start output.
   */
  async ensureCatalog(): Promise<void> {
    if (this.persisted || this.loading || this.catalogLoaded) return;
    const generation = this.generation;
    this.loading = true;
    try {
      const [definitions, presets] = await Promise.all([
        getMusicSoundscapes(),
        getMusicSoundscapePresets(),
      ]);
      if (generation !== this.generation) return;
      this.catalogReady = true;
      this.definitions = definitions;
      this.presets = presets;
      this.catalogLoaded = true;
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
      if (generation !== this.generation || this.playbackPending
        || (this.persisted && this.persisted.version > persisted.version)) return;
      this.persisted = persisted;
      this.snapshot = snapshot;
      // A soundscape started elsewhere, such as by phase automation, replaces a layer playing on its own.
      if (persisted.desiredPlaying || this.source?.kind !== "layer") this.source = presetSource(persisted.activePresetId);
    } catch (error) {
      if (generation === this.generation) this.error = message(error);
    }
  }

  /** Whether at least one sound of a preset can play on this device. */
  presetIsPlayable(preset: Pick<MusicSoundscapePreset, "sounds">): boolean {
    return this.layers(preset.sounds).length > 0;
  }

  /** Plays a preset's available sounds at its level and makes it the listener's choice. */
  async playPreset(preset: MusicSoundscapePreset): Promise<void> {
    if (this.playbackPending) return;
    if (!this.presetIsPlayable(preset)) {
      this.error = "This preset needs to be repaired before it can play.";
      return;
    }
    await this.startMix(preset, { kind: "preset", presetId: preset.id });
  }

  /** Pauses, resumes, or starts the active preset, or stops a layer playing on its own. */
  async togglePlayback(): Promise<void> {
    if (this.playbackPending) return;
    if (this.layerHoldsOutput) await this.stop();
    else if (this.snapshot.status === "playing") await this.pause();
    else if (this.snapshot.status === "paused") await this.resume();
    else if (this.activePreset) await this.playPreset(this.activePreset);
  }

  /**
   * Starts a mix for `source`: a preset is saved as the listener's playing choice, a layer on its own is not, and
   * `null` changes the output of the current source in place.
   */
  private async startMix(mix: SoundscapeMix, source: OutputSource | null): Promise<void> {
    const request = this.startRequest(mix);
    if (!request) return;
    this.playbackPending = true;
    this.error = null;
    let started = false;
    try {
      this.snapshot = await startSoundscape(request);
      if (source) this.source = source;
      started = true;
      if (source?.kind === "preset") await this.persistState(source.presetId, true);
      else if (source?.kind === "layer" && this.persisted?.desiredPlaying) await this.persistState(this.persisted.activePresetId, false);
    } catch (error) {
      this.error = message(error);
      try { this.snapshot = started ? await stopSoundscape() : await getSoundscapeSnapshot(); }
      catch { this.snapshot = { ...this.snapshot, status: "error", sourceId: request.sourceId }; }
    } finally {
      this.playbackPending = false;
    }
  }

  private layers(sounds: readonly MusicSoundscapePresetSound[]): MixLayer[] {
    return sounds
      .flatMap((sound) => {
        const definition = this.definitions.find((entry) => entry.id === sound.soundscapeId && entry.availability === "available");
        return definition ? [{ definition, level: sound.level }] : [];
      })
      .slice(0, MAX_SOUNDSCAPE_LAYERS);
  }

  private startRequest(mix: SoundscapeMix): MusicSoundscapeStartRequest | null {
    const [first, ...extra] = this.layers(mix.sounds);
    if (!first) return null;
    return {
      sourceId: first.definition.id,
      generatedKind: first.definition.generatedKind,
      localPath: first.definition.localPath,
      level: first.level,
      extraSources: extra.map((layer) => ({ sourceId: layer.definition.id, generatedKind: layer.definition.generatedKind, localPath: layer.definition.localPath, level: layer.level })),
      presetLevel: mix.level,
    };
  }

  /**
   * Changes how loud a preset plays relative to the Music volume setting. The playing output follows at once, and
   * the level is saved into the preset after the slider settles.
   */
  async setPresetLevel(preset: MusicSoundscapePreset, level: number): Promise<void> {
    if (!Number.isFinite(level)) return;
    const next = Math.min(MAX_SOUNDSCAPE_PRESET_LEVEL, Math.max(0, level));
    this.presets = this.presets.map((entry) => entry.id === preset.id ? { ...entry, level: next } : entry);
    this.scheduleLevelSave(preset.id, next);
    const live = !this.preview && this.source?.kind === "preset" && this.source.presetId === preset.id
      && (this.snapshot.status === "playing" || this.snapshot.status === "paused");
    if (!live) return;
    const generation = ++this.levelGeneration;
    this.levelQueue = this.levelQueue.catch(() => undefined).then(async () => {
      if (generation !== this.levelGeneration) return;
      try { await setSoundscapePresetLevel(next); }
      catch (error) { if (generation === this.levelGeneration) this.error = message(error); }
    });
    await this.levelQueue;
  }

  private scheduleLevelSave(presetId: string, level: number): void {
    const pending = this.levelSaveTimers.get(presetId);
    if (pending !== undefined) window.clearTimeout(pending);
    const generation = this.generation;
    this.levelSaveTimers.set(presetId, window.setTimeout(() => {
      this.levelSaveTimers.delete(presetId);
      void saveMusicSoundscapePresetLevel(presetId, level, Date.now())
        .then((saved) => {
          if (generation !== this.generation) return;
          // A newer drag keeps its local level; the stored version still advances so later edits do not conflict.
          const newer = this.levelSaveTimers.has(presetId);
          this.presets = this.presets.map((entry) => entry.id !== saved.id ? entry : newer ? { ...entry, version: saved.version, updatedAtMs: saved.updatedAtMs } : saved);
        })
        .catch((error: unknown) => { if (generation === this.generation) this.error = message(error); });
    }, PRESET_LEVEL_SAVE_DELAY_MS));
  }

  private clearLevelSaves(): void {
    for (const timer of this.levelSaveTimers.values()) window.clearTimeout(timer);
    this.levelSaveTimers.clear();
  }

  /**
   * Starts an isolated preview, pausing current output without writing the saved selection. Preview mixes never
   * become the listener's choice, and a restart during a preview restores the original selection.
   */
  beginPreview(): Promise<void> {
    return this.enqueuePreview(async () => {
      if (this.preview) return;
      // A layer playing on its own is never saved, so the preview stops it instead of bringing it back.
      const layer = this.layerHoldsOutput;
      this.preview = { resumes: !layer && this.snapshot.status === "playing", replacedOutput: false, playing: null };
      if (layer) {
        this.snapshot = await stopSoundscape();
        this.source = presetSource(this.persisted?.activePresetId ?? null);
      } else if (this.preview.resumes) this.snapshot = await pauseSoundscape();
      await this.applyPreviewTarget();
    });
  }

  /**
   * Plays this mix inside the preview, or silences it for `null` or a mix without playable sounds. The latest
   * request wins, a request made before the preview begins plays once it does, and level changes to the same
   * sounds adjust the output without restarting it.
   */
  previewMix(mix: SoundscapeMix | null): Promise<void> {
    this.previewTarget = mix ? { sounds: mix.sounds.map((sound) => ({ ...sound })), level: mix.level } : null;
    return this.enqueuePreview(() => this.applyPreviewTarget());
  }

  private async applyPreviewTarget(): Promise<void> {
    const preview = this.preview;
    if (!preview) return;
    const target = this.previewTarget;
    const layers = target ? this.layers(target.sounds) : [];
    const ids = layers.map((layer) => layer.definition.id);
    const levels = layers.map((layer) => layer.level);
    const current = preview.playing;
    if (target && current && sameOrder(ids, current.ids)) {
      if (!sameOrder(levels, current.levels)) await setSoundscapeLevels(levels);
      if (target.level !== current.level) await setSoundscapePresetLevel(target.level);
      preview.playing = { ids, levels, level: target.level };
      return;
    }
    if (!target || layers.length === 0) {
      // The paused output stays resumable until a preview mix actually replaces it.
      if (!current) return;
      this.snapshot = await stopSoundscape();
      preview.playing = null;
      return;
    }
    const request = this.startRequest(target);
    if (!request) return;
    // Layers the draft keeps continue untouched; only the first preview mix starts from silence.
    this.snapshot = await startSoundscape(current ? { ...request, keepPlayingLayers: true } : request);
    preview.replacedOutput = true;
    preview.playing = { ids, levels, level: target.level };
  }

  /** Ends the preview and brings back the saved preset as it was: playing, paused, or silent. */
  endPreview(): Promise<void> {
    return this.enqueuePreview(async () => {
      const preview = this.preview;
      if (!preview) return;
      this.previewTarget = null;
      // The preview ends together with the snapshot that replaces it, so the draft never shows as the saved preset.
      try {
        if (!preview.replacedOutput) {
          if (preview.resumes) this.snapshot = await resumeSoundscape();
          return;
        }
        const request = preview.resumes && this.activePreset ? this.startRequest(this.activePreset) : null;
        this.snapshot = request ? await startSoundscape(request) : await stopSoundscape();
      } finally {
        this.preview = null;
      }
    });
  }

  get previewing(): boolean {
    return this.preview !== null;
  }

  private enqueuePreview(step: () => Promise<void>): Promise<void> {
    this.previewQueue = this.previewQueue.then(step).catch((error: unknown) => { this.error = message(error); });
    return this.previewQueue;
  }

  async pause(): Promise<void> {
    if (this.playbackPending) return;
    if (this.layerHoldsOutput) {
      await this.stop();
      return;
    }
    this.playbackPending = true;
    try { this.snapshot = await pauseSoundscape(); await this.persistState(this.persisted?.activePresetId ?? null, false); }
    catch (error) { this.error = message(error); }
    finally { this.playbackPending = false; }
  }

  async resume(): Promise<void> {
    if (this.playbackPending) return;
    this.playbackPending = true;
    try { this.snapshot = await resumeSoundscape(); await this.persistState(this.persisted?.activePresetId ?? null, true); }
    catch (error) { this.error = message(error); }
    finally { this.playbackPending = false; }
  }

  /** Silences output and keeps the preset chosen, so the next play starts it again. */
  async stop(): Promise<void> {
    if (this.playbackPending) return;
    this.playbackPending = true;
    try {
      this.snapshot = await stopSoundscape();
      this.source = presetSource(this.persisted?.activePresetId ?? null);
      await this.persistState(this.persisted?.activePresetId ?? null, false);
    }
    catch (error) { this.error = message(error); }
    finally { this.playbackPending = false; }
  }

  async recover(): Promise<void> {
    try { this.snapshot = await recoverSoundscape(); this.error = null; }
    catch (error) { this.error = message(error); }
  }

  async switchVault(nextVaultId: string | null): Promise<void> {
    ++this.generation;
    ++this.levelGeneration;
    this.clearLevelSaves();
    this.snapshot = IDLE_SNAPSHOT;
    this.preview = null;
    this.previewTarget = null;
    this.source = null;
    this.definitions = [];
    this.presets = [];
    this.catalogLoaded = false;
    this.catalogReady = false;
    this.persisted = null;
    this.error = null;
    if (nextVaultId) await this.initialize();
  }

  async saveDefinition(write: MusicSoundscapeWrite): Promise<void> {
    if (!this.catalogReady) return;
    this.saving = true;
    this.error = null;
    try {
      const saved = await upsertMusicSoundscape(write);
      this.definitions = [...this.definitions.filter((entry) => entry.id !== saved.id), saved]
        .sort((left, right) => left.createdAtMs - right.createdAtMs || left.name.localeCompare(right.name));
    } catch (error) { this.error = message(error); throw error; }
    finally { this.saving = false; }
  }

  /**
   * Removes a sound from the library and from every preset. When the active preset holds it, output continues with
   * the preset's remaining sounds, or stops when none are left.
   */
  async removeDefinition(definition: MusicSoundscapeDefinition): Promise<void> {
    try {
      const active = this.activePreset;
      const running = this.snapshot.status === "playing" || this.snapshot.status === "paused";
      if (this.source?.kind === "layer") {
        if (this.source.layerId === definition.id && this.layerHoldsOutput) await this.stop();
      } else if (active && running && active.sounds.some((sound) => sound.soundscapeId === definition.id)) {
        const remaining = { ...active, sounds: active.sounds.filter((sound) => sound.soundscapeId !== definition.id) };
        if (this.snapshot.status === "playing" && this.presetIsPlayable(remaining)) await this.startMix(remaining, null);
        else await this.stop();
      }
      await removeMusicSoundscape(definition.id, definition.version);
      this.definitions = this.definitions.filter((entry) => entry.id !== definition.id);
      this.presets = this.presets.map((preset) => preset.sounds.some((sound) => sound.soundscapeId === definition.id)
        ? { ...preset, sounds: preset.sounds.filter((sound) => sound.soundscapeId !== definition.id) }
        : preset);
      await this.refreshState();
    } catch (error) {
      this.error = message(error);
      throw error;
    }
  }

  /** Creates or updates a preset and returns the stored row. */
  async savePreset(write: MusicSoundscapePresetWrite): Promise<MusicSoundscapePreset> {
    this.saving = true;
    this.error = null;
    try {
      const pending = this.levelSaveTimers.get(write.id);
      if (pending !== undefined) { window.clearTimeout(pending); this.levelSaveTimers.delete(write.id); }
      const saved = await upsertMusicSoundscapePreset(write);
      this.presets = sortPresets([...this.presets.filter((entry) => entry.id !== saved.id), saved]);
      return saved;
    } catch (error) { this.error = message(error); throw error; }
    finally { this.saving = false; }
  }

  /**
   * Plays one layer on its own at the default levels, or stops it when it already plays. It replaces any playing
   * soundscape without creating, choosing, or resuming one; the last soundscape stays chosen but silent.
   */
  async playLayer(definition: MusicSoundscapeDefinition): Promise<void> {
    if (this.playbackPending) return;
    if (this.playingLayerId === definition.id) {
      await this.stop();
      return;
    }
    if (definition.availability !== "available") {
      this.error = "This layer needs to be repaired before it can play.";
      return;
    }
    const mix = { sounds: [{ soundscapeId: definition.id, level: DEFAULT_PRESET_SOUND_LEVEL }], level: DEFAULT_SOUNDSCAPE_PRESET_LEVEL };
    await this.startMix(mix, { kind: "layer", layerId: definition.id });
  }

  /**
   * Removes a preset; removing the active one silences its output and clears the choice first. A layer playing on
   * its own keeps playing.
   */
  async removePreset(preset: MusicSoundscapePreset): Promise<void> {
    this.saving = true;
    this.error = null;
    try {
      if (this.source?.kind === "preset" && this.source.presetId === preset.id) {
        if (this.snapshot.status !== "idle") this.snapshot = await stopSoundscape();
        this.source = null;
      }
      if (preset.id === this.persisted?.activePresetId) await this.persistState(null, false);
      const pending = this.levelSaveTimers.get(preset.id);
      if (pending !== undefined) { window.clearTimeout(pending); this.levelSaveTimers.delete(preset.id); }
      await removeMusicSoundscapePreset(preset.id, this.presets.find((entry) => entry.id === preset.id)?.version ?? preset.version);
      this.presets = this.presets.filter((entry) => entry.id !== preset.id);
      await this.refreshState();
    } catch (error) { this.error = message(error); throw error; }
    finally { this.saving = false; }
  }

  /** Rereads the saved selection after removals the native library may have applied to it. */
  private async refreshState(): Promise<void> {
    const generation = this.generation;
    this.stateWriteQueue = this.stateWriteQueue.catch(() => undefined).then(async () => {
      if (generation !== this.generation || !this.persisted) return;
      const persisted = await getMusicSoundscapeState();
      if (generation === this.generation) this.persisted = persisted;
    });
    await this.stateWriteQueue;
  }

  private async persistState(activePresetId: string | null, desiredPlaying: boolean): Promise<void> {
    const generation = this.generation;
    this.stateWriteQueue = this.stateWriteQueue.catch(() => undefined).then(async () => {
      if (generation !== this.generation) return;
      const current = this.persisted;
      if (!current) return;
      const saved = await updateMusicSoundscapeState({
        activePresetId,
        desiredPlaying: desiredPlaying && activePresetId !== null,
        expectedVersion: current.version,
        updatedAtMs: Date.now(),
      });
      if (generation === this.generation) this.persisted = saved;
    });
    await this.stateWriteQueue;
  }
}

function presetSource(presetId: string | null): OutputSource | null {
  return presetId ? { kind: "preset", presetId } : null;
}

function sortPresets(presets: MusicSoundscapePreset[]): MusicSoundscapePreset[] {
  return presets.sort((left, right) => left.name.toLowerCase().localeCompare(right.name.toLowerCase()));
}

function sameOrder<T>(left: readonly T[], right: readonly T[]): boolean {
  return left.length === right.length && left.every((entry, index) => entry === right[index]);
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
