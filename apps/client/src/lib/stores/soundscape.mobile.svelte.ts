import type {
  MusicSoundscapeDefinition,
  MusicSoundscapePreset,
  MusicSoundscapePresetWrite,
  MusicSoundscapeSnapshot,
  MusicSoundscapeState,
  MusicSoundscapeWrite,
} from "$lib/music/soundscape/contracts";
import type { SoundscapeMix } from "./soundscape.svelte";

const UNAVAILABLE = "Soundscapes are unavailable on this platform.";

class MobileSoundscapeStore {
  definitions = $state<MusicSoundscapeDefinition[]>([]);
  presets = $state<MusicSoundscapePreset[]>([]);
  persisted = $state<MusicSoundscapeState | null>(null);
  snapshot = $state<MusicSoundscapeSnapshot>({ status: "idle", sourceId: null, errorCode: null });
  loading = $state(false);
  saving = $state(false);
  playbackPending = $state(false);
  error = $state<string | null>(null);
  catalogReady = $state(false);

  get activePreset(): MusicSoundscapePreset | null {
    return null;
  }

  get playingPreset(): MusicSoundscapePreset | null {
    return null;
  }

  get playingLayerId(): string | null {
    return null;
  }

  presetIsPlayable(_preset: Pick<MusicSoundscapePreset, "sounds">): boolean {
    return false;
  }

  async initialize(): Promise<void> {}
  async ensureCatalog(): Promise<void> {}
  async refreshAcceptedOutput(): Promise<void> {}
  async playPreset(_preset: MusicSoundscapePreset): Promise<void> {}
  async togglePlayback(): Promise<void> {}
  async setPresetLevel(_preset: MusicSoundscapePreset, _level: number): Promise<void> {}
  get previewing(): boolean {
    return false;
  }
  async beginPreview(): Promise<void> {}
  async previewMix(_mix: SoundscapeMix | null): Promise<void> {}
  async endPreview(): Promise<void> {}
  async pause(): Promise<void> {}
  async resume(): Promise<void> {}
  async stop(): Promise<void> {}
  async recover(): Promise<void> {}
  async switchVault(_nextVaultId: string | null): Promise<void> {}
  async saveDefinition(_write: MusicSoundscapeWrite): Promise<void> {}
  async removeDefinition(_definition: MusicSoundscapeDefinition): Promise<void> {}
  async savePreset(_write: MusicSoundscapePresetWrite): Promise<MusicSoundscapePreset> {
    throw new Error(UNAVAILABLE);
  }
  async makePresetFromSound(_definition: MusicSoundscapeDefinition, _name: string): Promise<MusicSoundscapePreset> {
    throw new Error(UNAVAILABLE);
  }
  async playLayer(_definition: MusicSoundscapeDefinition): Promise<void> {}
  async removePreset(_preset: MusicSoundscapePreset): Promise<void> {}
}

let instance: MobileSoundscapeStore | null = null;

/** Return the inert soundscape store used on platforms without a soundscape engine. */
export function getSoundscapeStore(): MobileSoundscapeStore {
  instance ??= new MobileSoundscapeStore();
  return instance;
}
