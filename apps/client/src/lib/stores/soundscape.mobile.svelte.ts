import type {
  MusicSoundscapeDefinition,
  MusicSoundscapeGroup,
  MusicSoundscapeGroupWrite,
  MusicSoundscapeSnapshot,
  MusicSoundscapeState,
  MusicSoundscapeWrite,
} from "$lib/music/soundscape/contracts";
import { INITIAL_SOUNDSCAPE_VOLUME } from "$lib/music/soundscape/defaults";

class MobileSoundscapeStore {
  definitions = $state<MusicSoundscapeDefinition[]>([]);
  groups = $state<MusicSoundscapeGroup[]>([]);
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

  get activeDefinition(): MusicSoundscapeDefinition | null {
    return null;
  }

  async initialize(): Promise<void> {}
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
}

let instance: MobileSoundscapeStore | null = null;

/** Return the inert soundscape store used on platforms without a soundscape engine. */
export function getSoundscapeStore(): MobileSoundscapeStore {
  instance ??= new MobileSoundscapeStore();
  return instance;
}
