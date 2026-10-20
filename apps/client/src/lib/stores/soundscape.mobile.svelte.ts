import type {
  MusicSoundscapeDefinition,
  MusicSoundscapeGroup,
  MusicSoundscapeGroupWrite,
  MusicSoundscapePreset,
  MusicSoundscapePresetWrite,
  MusicSoundscapeSnapshot,
  MusicSoundscapeState,
  MusicSoundscapeWrite,
} from "$lib/music/soundscape/contracts";
import { INITIAL_SOUNDSCAPE_VOLUME } from "$lib/music/soundscape/defaults";

class MobileSoundscapeStore {
  definitions = $state<MusicSoundscapeDefinition[]>([]);
  groups = $state<MusicSoundscapeGroup[]>([]);
  presets = $state<MusicSoundscapePreset[]>([]);
  persisted = $state<MusicSoundscapeState | null>(null);
  sectionLevels = $state<{ generated: number | null; local: number | null }>({ generated: null, local: null });
  snapshot = $state<MusicSoundscapeSnapshot>({
    status: "idle",
    sourceId: null,
    volume: INITIAL_SOUNDSCAPE_VOLUME,
    errorCode: null,
  });
  loading = $state(false);
  saving = $state(false);
  playbackPending = $state(false);
  modePending = $state(false);
  error = $state<string | null>(null);
  deviceId = $state<string | null>(null);

  get activePreset(): MusicSoundscapePreset | null {
    return null;
  }

  presetIsPlayable(_preset: MusicSoundscapePreset): boolean {
    return false;
  }

  get activeDefinition(): MusicSoundscapeDefinition | null {
    return null;
  }

  async initialize(): Promise<void> {}
  async ensureCatalog(): Promise<void> {}
  async play(_id: string, _persist = true): Promise<void> {}
  async togglePlayback(): Promise<void> {}
  async toggleSelection(_id: string): Promise<void> {}
  async setMultipleEnabled(_enabled: boolean): Promise<void> {}
  async pause(): Promise<void> {}
  async resume(): Promise<void> {}
  async stop(): Promise<void> {}
  async setVolume(_volume: number): Promise<void> {}
  async setSectionLevel(_section: "generated" | "local", _level: number | null): Promise<void> {}
  async recover(): Promise<void> {}
  async switchVault(_nextVaultId: string | null): Promise<void> {}
  async saveDefinition(_write: Omit<MusicSoundscapeWrite, "deviceId">): Promise<void> {}
  async removeDefinition(_definition: MusicSoundscapeDefinition): Promise<void> {}
  async saveGroup(_write: MusicSoundscapeGroupWrite): Promise<void> {}
  async removeGroup(_group: MusicSoundscapeGroup): Promise<void> {}
  async playPreset(_preset: Pick<MusicSoundscapePreset, "soundscapeIds">): Promise<void> {}
  get previewing(): boolean {
    return false;
  }
  async beginPreview(): Promise<void> {}
  async previewSounds(_ids: readonly string[]): Promise<void> {}
  async endPreview(): Promise<void> {}
  async savePreset(_write: MusicSoundscapePresetWrite): Promise<MusicSoundscapePreset> {
    throw new Error("Background sounds are unavailable on this platform.");
  }
  async removePreset(_preset: MusicSoundscapePreset): Promise<void> {}
}

let instance: MobileSoundscapeStore | null = null;

/** Return the inert soundscape store used on platforms without a soundscape engine. */
export function getSoundscapeStore(): MobileSoundscapeStore {
  instance ??= new MobileSoundscapeStore();
  return instance;
}
