import { formatNumber } from "$lib/i18n/formatters";
import type { AppLocale } from "$lib/i18n/locales";
import type { Translate } from "$lib/i18n/translator.svelte";
import {
  MAX_SOUNDSCAPE_LAYERS,
  MAX_SOUNDSCAPE_PRESET_LEVEL,
  type MusicGeneratedNoiseKind,
  type MusicSoundscapeDefinition,
  type MusicSoundscapePreset,
  type MusicSoundscapePresetSound,
} from "./contracts";

/** Seeded one-layer rain soundscapes keyed by id, with the English name they are stored under. */
const SEEDED_RAIN_PRESETS: Readonly<Record<string, { kind: MusicGeneratedNoiseKind; storedName: string }>> = {
  "preset-generated-brown-noise": { kind: "brown", storedName: "Rain (soft)" },
  "preset-generated-pink-noise": { kind: "pink", storedName: "Rain (moderate)" },
  "preset-generated-white-noise": { kind: "white", storedName: "Rain (heavy)" },
};

/** Level a sound joins a preset draft at: the full preset level. */
export const DEFAULT_PRESET_SOUND_LEVEL = 1;

/** Ends of the preset level slider, in percent of the Music volume setting. */
export const PRESET_LEVEL_ADJUSTMENT_RANGE = { min: -100, max: 100 } as const;

/** Signed, localized percent for a preset level slider position, such as +25% or -50%. */
export function formatPresetLevelAdjustment(locale: AppLocale, adjustment: number): string {
  return formatNumber(locale, adjustment / 100, { style: "percent", signDisplay: "exceptZero", maximumFractionDigits: 0 });
}

/** Slider position from -100 to +100 for a preset level, where 0 matches the Music volume setting. */
export function presetLevelAdjustment(level: number): number {
  return Math.round((level - 1) * 100);
}

/** Preset level for a slider position from -100 to +100, clamped to the supported range. */
export function presetLevelFromAdjustment(adjustment: number): number {
  return Math.min(MAX_SOUNDSCAPE_PRESET_LEVEL, Math.max(0, 1 + adjustment / 100));
}

/** Localized name of a seeded rain preset until the listener renames it; other presets keep their stored name. */
export function presetDisplayName(preset: Pick<MusicSoundscapePreset, "id" | "name">, t: Translate): string {
  const seeded = SEEDED_RAIN_PRESETS[preset.id];
  return seeded && preset.name === seeded.storedName ? t(`music.soundscape.rainName.${seeded.kind}`) : preset.name;
}

/** Display name of a layer: generated noise uses its localized noise name. */
export function soundDisplayName(definition: MusicSoundscapeDefinition, t: Translate): string {
  return definition.generatedKind ? t(`music.soundscape.generatedName.${definition.generatedKind}`) : definition.name;
}

/** Add or remove one sound from a preset draft, keeping order and never growing past the layer limit. */
export function togglePresetDraftSound(sounds: readonly MusicSoundscapePresetSound[], id: string): MusicSoundscapePresetSound[] {
  if (sounds.some((sound) => sound.soundscapeId === id)) return sounds.filter((sound) => sound.soundscapeId !== id);
  if (sounds.length >= MAX_SOUNDSCAPE_LAYERS) return [...sounds];
  return [...sounds, { soundscapeId: id, level: DEFAULT_PRESET_SOUND_LEVEL }];
}

/** Change one sound's share of the preset level, clamped from silent to full. */
export function setPresetDraftSoundLevel(sounds: readonly MusicSoundscapePresetSound[], id: string, level: number): MusicSoundscapePresetSound[] {
  const next = Math.min(1, Math.max(0, level));
  return sounds.map((sound) => sound.soundscapeId === id ? { ...sound, level: next } : sound);
}

/** Preset made of exactly one sound at full share, if the listener already has one. */
export function singleSoundPreset(presets: readonly MusicSoundscapePreset[], soundscapeId: string): MusicSoundscapePreset | null {
  return presets.find((preset) => preset.sounds.length === 1 && preset.sounds[0].soundscapeId === soundscapeId) ?? null;
}
