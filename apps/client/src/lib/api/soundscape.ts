import { invoke } from "@tauri-apps/api/core";
import { dbUrl } from "$lib/api/db";
import {
  parseMusicSoundscape,
  parseMusicSoundscapeGroup,
  parseMusicSoundscapeGroups,
  parseMusicSoundscapePreset,
  parseMusicSoundscapePresets,
  parseMusicSoundscapes,
  parseMusicSoundscapeSnapshot,
  parseMusicSoundscapeState,
  type MusicSoundscapeDefinition,
  type MusicSoundscapeGroup,
  type MusicSoundscapeGroupWrite,
  type MusicSoundscapePreset,
  type MusicSoundscapePresetWrite,
  type MusicSoundscapeSnapshot,
  type MusicSoundscapeStartRequest,
  type MusicSoundscapeState,
  type MusicSoundscapeStateWrite,
  type MusicSoundscapeWrite,
} from "$lib/music/soundscape/contracts";

export const getDeviceId = (): Promise<string> => invoke<string>("vault_device_id");
export const getMusicSoundscapes = async (deviceId: string): Promise<MusicSoundscapeDefinition[]> => parseMusicSoundscapes(await invoke<unknown>("music_library_soundscapes", { dbUrl: dbUrl(), deviceId }));
export const getMusicSoundscapeGroups = async (): Promise<MusicSoundscapeGroup[]> => parseMusicSoundscapeGroups(await invoke<unknown>("music_library_soundscape_groups", { dbUrl: dbUrl() }));
export const upsertMusicSoundscapeGroup = async (request: MusicSoundscapeGroupWrite): Promise<MusicSoundscapeGroup> => parseMusicSoundscapeGroup(await invoke<unknown>("music_library_upsert_soundscape_group", { dbUrl: dbUrl(), request }));
export const removeMusicSoundscapeGroup = (groupId: string, expectedVersion: number): Promise<void> => invoke("music_library_remove_soundscape_group", { dbUrl: dbUrl(), groupId, expectedVersion });
export const getMusicSoundscapePresets = async (): Promise<MusicSoundscapePreset[]> => parseMusicSoundscapePresets(await invoke<unknown>("music_library_soundscape_presets", { dbUrl: dbUrl() }));
export const upsertMusicSoundscapePreset = async (request: MusicSoundscapePresetWrite): Promise<MusicSoundscapePreset> => parseMusicSoundscapePreset(await invoke<unknown>("music_library_upsert_soundscape_preset", { dbUrl: dbUrl(), request }));
export const removeMusicSoundscapePreset = (presetId: string, expectedVersion: number): Promise<void> => invoke("music_library_remove_soundscape_preset", { dbUrl: dbUrl(), presetId, expectedVersion });
export const upsertMusicSoundscape = async (request: MusicSoundscapeWrite): Promise<MusicSoundscapeDefinition> => parseMusicSoundscape(await invoke<unknown>("music_library_upsert_soundscape", { dbUrl: dbUrl(), request }));
export const removeMusicSoundscape = (soundscapeId: string, expectedVersion: number): Promise<void> => invoke("music_library_remove_soundscape", { dbUrl: dbUrl(), soundscapeId, expectedVersion });
export const getMusicSoundscapeState = async (): Promise<MusicSoundscapeState> => parseMusicSoundscapeState(await invoke<unknown>("music_library_soundscape_state", { dbUrl: dbUrl() }));
export const updateMusicSoundscapeState = async (request: MusicSoundscapeStateWrite): Promise<MusicSoundscapeState> => parseMusicSoundscapeState(await invoke<unknown>("music_library_update_soundscape_state", { dbUrl: dbUrl(), request }));
export const startSoundscape = async (request: MusicSoundscapeStartRequest): Promise<MusicSoundscapeSnapshot> => parseMusicSoundscapeSnapshot(await invoke<unknown>("music_soundscape_start", { request }));
export const pauseSoundscape = async (): Promise<MusicSoundscapeSnapshot> => parseMusicSoundscapeSnapshot(await invoke<unknown>("music_soundscape_pause"));
export const resumeSoundscape = async (): Promise<MusicSoundscapeSnapshot> => parseMusicSoundscapeSnapshot(await invoke<unknown>("music_soundscape_resume"));
export const stopSoundscape = async (): Promise<MusicSoundscapeSnapshot> => parseMusicSoundscapeSnapshot(await invoke<unknown>("music_soundscape_stop"));
/** Applies a preset level to the output that is playing, without saving it. */
export const setSoundscapePresetLevel = async (level: number): Promise<MusicSoundscapeSnapshot> => parseMusicSoundscapeSnapshot(await invoke<unknown>("music_soundscape_set_preset_level", { level }));
/** Saves a preset level; the latest write wins instead of conflicting, because it follows a slider. */
export const saveMusicSoundscapePresetLevel = async (presetId: string, level: number, updatedAtMs: number): Promise<MusicSoundscapePreset> => parseMusicSoundscapePreset(await invoke<unknown>("music_library_set_soundscape_preset_level", { dbUrl: dbUrl(), presetId, level, updatedAtMs }));
export const setSoundscapeLevels = async (levels: number[]): Promise<MusicSoundscapeSnapshot> => parseMusicSoundscapeSnapshot(await invoke<unknown>("music_soundscape_set_levels", { levels }));
export const recoverSoundscape = async (): Promise<MusicSoundscapeSnapshot> => parseMusicSoundscapeSnapshot(await invoke<unknown>("music_soundscape_recover"));
export const getSoundscapeSnapshot = async (): Promise<MusicSoundscapeSnapshot> => parseMusicSoundscapeSnapshot(await invoke<unknown>("music_soundscape_snapshot"));
