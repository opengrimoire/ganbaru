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
  groupId: string | null;
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
  groupId: string | null;
  deviceId: string;
  localPath: string | null;
  expectedVersion: number | null;
  updatedAtMs: number;
}

export interface MusicSoundscapeState {
  automaticIntent?: boolean;
  activeSoundscapeId: string | null;
  activeIds: string[];
  multipleEnabled: boolean;
  generatedLevel: number | null;
  localLevel: number | null;
  desiredPlaying: boolean;
  volume: number;
  updatedAtMs: number;
  version: number;
}

export interface MusicSoundscapeGroup {
  id: string;
  name: string;
  icon: string;
  createdAtMs: number;
  updatedAtMs: number;
  version: number;
}

export interface MusicSoundscapeGroupWrite {
  id: string;
  name: string;
  icon: string;
  expectedVersion: number | null;
  updatedAtMs: number;
}

export interface MusicSoundscapeStateWrite {
  activeSoundscapeId: string | null;
  activeIds: string[];
  multipleEnabled: boolean;
  generatedLevel: number | null;
  localLevel: number | null;
  desiredPlaying: boolean;
  volume: number;
  expectedVersion: number;
  updatedAtMs: number;
}

export interface MusicSoundscapeSnapshot {
  status: MusicSoundscapeStatus;
  sourceId: string | null;
  volume: number;
  errorCode: string | null;
}

export interface MusicSoundscapeStartRequest {
  sourceId: string;
  generatedKind: MusicGeneratedNoiseKind | null;
  localPath: string | null;
  level: number;
  extraSources: Array<{ sourceId: string; generatedKind: MusicGeneratedNoiseKind | null; localPath: string | null; level: number }>;
  volume: number;
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
function textArray(value: unknown, label: string): string[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  return value.map((entry, index) => text(entry, `${label}[${index}]`));
}
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
    groupId: optionalText(row.groupId, `${label}.groupId`),
    availability: enumeration(row.availability, availability, `${label}.availability`),
    localPath: optionalText(row.localPath, `${label}.localPath`),
    createdAtMs: integer(row.createdAtMs, `${label}.createdAtMs`),
    updatedAtMs: integer(row.updatedAtMs, `${label}.updatedAtMs`),
    version: integer(row.version, `${label}.version`),
  };
}
export function parseMusicSoundscapeGroup(value: unknown, label = "sound group"): MusicSoundscapeGroup {
  const row = record(value, label);
  return {
    id: text(row.id, `${label}.id`),
    name: text(row.name, `${label}.name`),
    icon: iconText(row.icon, `${label}.icon`),
    createdAtMs: integer(row.createdAtMs, `${label}.createdAtMs`),
    updatedAtMs: integer(row.updatedAtMs, `${label}.updatedAtMs`),
    version: integer(row.version, `${label}.version`),
  };
}
export function parseMusicSoundscapeGroups(value: unknown): MusicSoundscapeGroup[] {
  if (!Array.isArray(value)) throw new Error("sound groups must be an array");
  return value.map((entry, index) => parseMusicSoundscapeGroup(entry, `sound groups[${index}]`));
}
export function parseMusicSoundscapes(value: unknown): MusicSoundscapeDefinition[] {
  if (!Array.isArray(value)) throw new Error("soundscapes must be an array");
  return value.map((entry, index) => parseMusicSoundscape(entry, `soundscapes[${index}]`));
}
export function parseMusicSoundscapeState(value: unknown): MusicSoundscapeState {
  const row = record(value, "soundscape state");
  const activeSoundscapeId = optionalText(row.activeSoundscapeId, "soundscape state.activeSoundscapeId");
  const activeIds = textArray(row.activeIds, "soundscape state.activeIds");
  const multipleEnabled = boolean(row.multipleEnabled, "soundscape state.multipleEnabled");
  const desiredPlaying = boolean(row.desiredPlaying, "soundscape state.desiredPlaying");
  const automaticIntent = row.automaticIntent === undefined ? false : boolean(row.automaticIntent, "soundscape state.automaticIntent");
  const volume = number(row.volume, "soundscape state.volume");
  const generatedLevel = row.generatedLevel === null ? null : number(row.generatedLevel, "soundscape state.generatedLevel");
  const localLevel = row.localLevel === null ? null : number(row.localLevel, "soundscape state.localLevel");
  if (activeSoundscapeId !== (activeIds[0] ?? null) || new Set(activeIds).size !== activeIds.length || activeIds.length > MAX_SOUNDSCAPE_LAYERS) throw new Error("soundscape state has an invalid selection");
  if ((!multipleEnabled && activeIds.length > 1) || (desiredPlaying && activeIds.length === 0)) throw new Error("soundscape state has an invalid playback mode");
  if (volume < 0 || volume > 1) throw new Error("soundscape state.volume is out of range");
  if ([generatedLevel, localLevel].some((level) => level !== null && (level < 0 || level > 2))) throw new Error("soundscape state.section level is out of range");
  return { activeSoundscapeId, activeIds, multipleEnabled, generatedLevel, localLevel, desiredPlaying, automaticIntent, volume, updatedAtMs: integer(row.updatedAtMs, "soundscape state.updatedAtMs"), version: integer(row.version, "soundscape state.version") };
}
export function parseMusicSoundscapeSnapshot(value: unknown): MusicSoundscapeSnapshot {
  const row = record(value, "soundscape snapshot");
  return { status: enumeration(row.status, statuses, "soundscape snapshot.status"), sourceId: optionalText(row.sourceId, "soundscape snapshot.sourceId"), volume: number(row.volume, "soundscape snapshot.volume"), errorCode: optionalText(row.errorCode, "soundscape snapshot.errorCode") };
}
