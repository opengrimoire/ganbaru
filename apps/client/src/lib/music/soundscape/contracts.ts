import { parseProjectIcon, serializeProjectIcon } from "$lib/projects/icons/values";

export type MusicSoundscapeSourceKind = "generated-noise" | "local-loop" | "bundled-loop";
export type MusicGeneratedNoiseKind = "white" | "pink" | "brown";
export type MusicSoundscapeAvailability = "available" | "missing" | "unsupported";
export type MusicSoundscapeStatus = "idle" | "playing" | "paused" | "error";
export const MAX_SOUNDSCAPE_LAYERS = 16;

export interface MusicSoundscapeDefinition {
  id: string;
  sourceKind: MusicSoundscapeSourceKind;
  generatedKind: MusicGeneratedNoiseKind | null;
  bundledIdentity: string | null;
  name: string;
  icon: string;
  availability: MusicSoundscapeAvailability;
  localPath: string | null;
  createdAtMs: number;
  updatedAtMs: number;
  version: number;
}

export interface MusicSoundscapeWrite {
  id: string;
  sourceKind: MusicSoundscapeSourceKind;
  generatedKind: MusicGeneratedNoiseKind | null;
  bundledIdentity: string | null;
  name: string;
  icon: string;
  localPath: string | null;
  expectedVersion: number | null;
  updatedAtMs: number;
}

/** The preset the soundscape plays; output follows the Music volume setting, so no volume is stored here. */
export interface MusicSoundscapeState {
  /** An automatic selection requires fresh native phase admission after restart. */
  automaticIntent: boolean;
  activePresetId: string | null;
  desiredPlaying: boolean;
  updatedAtMs: number;
  version: number;
}

/** Highest preset level: twice the Music volume setting before output clamps at full scale. */
export const MAX_SOUNDSCAPE_PRESET_LEVEL = 2;
/** Level of a new preset: the Music volume setting itself, shown as 0%. */
export const DEFAULT_SOUNDSCAPE_PRESET_LEVEL = 1;

/** One sound in a preset mix with its share of the preset level, from 0 (silent) to 1 (full). */
export interface MusicSoundscapePresetSound {
  soundscapeId: string;
  level: number;
}

/**
 * Named mix of soundscape sounds, the only thing a soundscape plays. Its level multiplies the Music volume setting.
 * Removing a sound can leave a preset empty until it is edited.
 */
export interface MusicSoundscapePreset {
  id: string;
  name: string;
  icon: string;
  level: number;
  sounds: MusicSoundscapePresetSound[];
  createdAtMs: number;
  updatedAtMs: number;
  version: number;
}

export interface MusicSoundscapePresetWrite {
  id: string;
  name: string;
  icon: string;
  level: number;
  sounds: MusicSoundscapePresetSound[];
  expectedVersion: number | null;
  updatedAtMs: number;
}

export interface MusicSoundscapeStateWrite {
  activePresetId: string | null;
  desiredPlaying: boolean;
  expectedVersion: number;
  updatedAtMs: number;
}

export interface MusicSoundscapeSnapshot {
  status: MusicSoundscapeStatus;
  sourceId: string | null;
  errorCode: string | null;
}

export interface MusicSoundscapeStartLayer {
  sourceId: string;
  generatedKind: MusicGeneratedNoiseKind | null;
  localPath: string | null;
  /** Share of the preset level, from 0 to 1. */
  level: number;
}

export interface MusicSoundscapeStartRequest extends MusicSoundscapeStartLayer {
  extraSources: MusicSoundscapeStartLayer[];
  /** Multiplier of the Music volume setting, from 0 to 2. */
  presetLevel: number;
}

const sourceKinds = new Set<MusicSoundscapeSourceKind>(["generated-noise", "local-loop", "bundled-loop"]);
const generatedKinds = new Set<MusicGeneratedNoiseKind>(["white", "pink", "brown"]);
const availability = new Set<MusicSoundscapeAvailability>(["available", "missing", "unsupported"]);
const statuses = new Set<MusicSoundscapeStatus>(["idle", "playing", "paused", "error"]);

function record(value: unknown, label: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error(`${label} must be an object`);
  return value as Record<string, unknown>;
}
function text(value: unknown, label: string): string { if (typeof value !== "string") throw new Error(`${label} must be a string`); return value; }
function iconText(value: unknown, label: string): string {
  const icon = text(value, label);
  if (icon.length > 500 || serializeProjectIcon(parseProjectIcon(icon)) !== icon) throw new Error(`${label} is invalid`);
  return icon;
}
function optionalText(value: unknown, label: string): string | null { return value === null ? null : text(value, label); }
function number(value: unknown, label: string): number { if (typeof value !== "number" || !Number.isFinite(value)) throw new Error(`${label} must be finite`); return value; }
function integer(value: unknown, label: string): number { const result = number(value, label); if (!Number.isSafeInteger(result)) throw new Error(`${label} must be an integer`); return result; }
function boolean(value: unknown, label: string): boolean { if (typeof value !== "boolean") throw new Error(`${label} must be boolean`); return value; }
function enumeration<T extends string>(value: unknown, values: ReadonlySet<T>, label: string): T { if (typeof value !== "string" || !values.has(value as T)) throw new Error(`${label} is unsupported`); return value as T; }

export function parseMusicSoundscape(value: unknown, label = "soundscape"): MusicSoundscapeDefinition {
  const row = record(value, label);
  return {
    id: text(row.id, `${label}.id`),
    sourceKind: enumeration(row.sourceKind, sourceKinds, `${label}.sourceKind`),
    generatedKind: row.generatedKind === null ? null : enumeration(row.generatedKind, generatedKinds, `${label}.generatedKind`),
    bundledIdentity: optionalText(row.bundledIdentity, `${label}.bundledIdentity`),
    name: text(row.name, `${label}.name`),
    icon: iconText(row.icon, `${label}.icon`),
    availability: enumeration(row.availability, availability, `${label}.availability`),
    localPath: optionalText(row.localPath, `${label}.localPath`),
    createdAtMs: integer(row.createdAtMs, `${label}.createdAtMs`),
    updatedAtMs: integer(row.updatedAtMs, `${label}.updatedAtMs`),
    version: integer(row.version, `${label}.version`),
  };
}
export function parseMusicSoundscapes(value: unknown): MusicSoundscapeDefinition[] {
  if (!Array.isArray(value)) throw new Error("soundscapes must be an array");
  return value.map((entry, index) => parseMusicSoundscape(entry, `soundscapes[${index}]`));
}
export function parseMusicSoundscapeState(value: unknown): MusicSoundscapeState {
  const row = record(value, "soundscape state");
  const activePresetId = optionalText(row.activePresetId, "soundscape state.activePresetId");
  const desiredPlaying = boolean(row.desiredPlaying, "soundscape state.desiredPlaying");
  if (desiredPlaying && activePresetId === null) throw new Error("soundscape state cannot play without a preset");
  return {
    automaticIntent: boolean(row.automaticIntent, "soundscape state.automaticIntent"),
    activePresetId,
    desiredPlaying,
    updatedAtMs: integer(row.updatedAtMs, "soundscape state.updatedAtMs"),
    version: integer(row.version, "soundscape state.version"),
  };
}
export function parseMusicSoundscapeSnapshot(value: unknown): MusicSoundscapeSnapshot {
  const row = record(value, "soundscape snapshot");
  return { status: enumeration(row.status, statuses, "soundscape snapshot.status"), sourceId: optionalText(row.sourceId, "soundscape snapshot.sourceId"), errorCode: optionalText(row.errorCode, "soundscape snapshot.errorCode") };
}
function level(value: unknown, max: number, label: string): number {
  const result = number(value, label);
  if (result < 0 || result > max) throw new Error(`${label} is out of range`);
  return result;
}
function presetSounds(value: unknown, label: string): MusicSoundscapePresetSound[] {
  if (!Array.isArray(value) || value.length > MAX_SOUNDSCAPE_LAYERS) throw new Error(`${label} is invalid`);
  const sounds = value.map((entry, index) => {
    const sound = record(entry, `${label}[${index}]`);
    return { soundscapeId: text(sound.soundscapeId, `${label}[${index}].soundscapeId`), level: level(sound.level, 1, `${label}[${index}].level`) };
  });
  if (new Set(sounds.map((sound) => sound.soundscapeId)).size !== sounds.length) throw new Error(`${label} repeats a sound`);
  return sounds;
}
export function parseMusicSoundscapePreset(value: unknown, label = "sound preset"): MusicSoundscapePreset {
  const row = record(value, label);
  return {
    id: text(row.id, `${label}.id`),
    name: text(row.name, `${label}.name`),
    icon: iconText(row.icon, `${label}.icon`),
    level: level(row.level, MAX_SOUNDSCAPE_PRESET_LEVEL, `${label}.level`),
    sounds: presetSounds(row.sounds, `${label}.sounds`),
    createdAtMs: integer(row.createdAtMs, `${label}.createdAtMs`),
    updatedAtMs: integer(row.updatedAtMs, `${label}.updatedAtMs`),
    version: integer(row.version, `${label}.version`),
  };
}
export function parseMusicSoundscapePresets(value: unknown): MusicSoundscapePreset[] {
  if (!Array.isArray(value)) throw new Error("sound presets must be an array");
  return value.map((entry, index) => parseMusicSoundscapePreset(entry, `sound presets[${index}]`));
}
