export type MusicActivityPhase = "focus" | "short-break" | "long-break";
export type MusicAssignmentBehavior =
  | "inherit"
  | "play-automatically"
  | "prepare-silently"
  | "pause-music"
  | "keep-current-music";
export type MusicSoundscapeBehavior =
  | "inherit"
  | "play-selected"
  | "pause-soundscape"
  | "keep-current-soundscape";
export type MusicAssignmentOwnerKind =
  | "project-default"
  | "event-snapshot"
  | "event-override"
  | "work-environment";
export type MusicAssignmentProvenanceKind =
  | "explicit"
  | "copied-project"
  | "work-environment";

export interface MusicContextAssignment {
  ownerKind: MusicAssignmentOwnerKind;
  ownerId: string;
  phase: MusicActivityPhase;
  behavior: MusicAssignmentBehavior;
  playlistId: string | null;
  soundscapeId: string | null;
  soundscapeBehavior: MusicSoundscapeBehavior;
  provenanceKind: MusicAssignmentProvenanceKind;
  provenanceId: string | null;
  updatedAtMs: number;
  version: number;
}

export interface MusicContextAssignmentDraft {
  phase: MusicActivityPhase;
  behavior: MusicAssignmentBehavior;
  playlistId: string | null;
  soundscapeId: string | null;
  soundscapeBehavior: MusicSoundscapeBehavior;
  provenanceKind: MusicAssignmentProvenanceKind;
  provenanceId: string | null;
}

export interface MusicContextAssignmentSet {
  ownerKind: MusicAssignmentOwnerKind;
  ownerId: string;
  assignments: MusicContextAssignmentDraft[];
  updatedAtMs: number;
}

export type MusicAssignmentSource = "event-override" | "work-environment" | "project-snapshot" | "none";


const activityPhases = new Set<MusicActivityPhase>(["focus", "short-break", "long-break"]);
const assignmentBehaviors = new Set<MusicAssignmentBehavior>([
  "inherit",
  "play-automatically",
  "prepare-silently",
  "pause-music",
  "keep-current-music",
]);
const soundscapeBehaviors = new Set<MusicSoundscapeBehavior>([
  "inherit",
  "play-selected",
  "pause-soundscape",
  "keep-current-soundscape",
]);
const assignmentOwnerKinds = new Set<MusicAssignmentOwnerKind>([
  "project-default",
  "event-snapshot",
  "event-override",
  "work-environment",
]);
const assignmentProvenanceKinds = new Set<MusicAssignmentProvenanceKind>([
  "explicit",
  "copied-project",
  "work-environment",
]);

/** Validates context assignment rows received from the Tauri boundary. */
export function parseMusicContextAssignments(value: unknown): MusicContextAssignment[] {
  if (!Array.isArray(value)) throw new Error("music context assignments must be an array");
  return value.map((entry, index) => parseMusicContextAssignment(entry, `music context assignments[${index}]`));
}

function parseMusicContextAssignment(value: unknown, label: string): MusicContextAssignment {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  const row = value as Record<string, unknown>;
  return {
    ownerKind: enumValue(row.ownerKind, assignmentOwnerKinds, `${label}.ownerKind`),
    ownerId: stringValue(row.ownerId, `${label}.ownerId`),
    phase: enumValue(row.phase, activityPhases, `${label}.phase`),
    behavior: enumValue(row.behavior, assignmentBehaviors, `${label}.behavior`),
    playlistId: nullableString(row.playlistId, `${label}.playlistId`),
    soundscapeId: nullableString(row.soundscapeId, `${label}.soundscapeId`),
    soundscapeBehavior: enumValue(row.soundscapeBehavior, soundscapeBehaviors, `${label}.soundscapeBehavior`),
    provenanceKind: enumValue(row.provenanceKind, assignmentProvenanceKinds, `${label}.provenanceKind`),
    provenanceId: nullableString(row.provenanceId, `${label}.provenanceId`),
    updatedAtMs: integerValue(row.updatedAtMs, `${label}.updatedAtMs`),
    version: integerValue(row.version, `${label}.version`),
  };
}

function enumValue<T extends string>(value: unknown, allowed: ReadonlySet<T>, label: string): T {
  if (typeof value !== "string" || !allowed.has(value as T)) throw new Error(`${label} is not supported`);
  return value as T;
}

function stringValue(value: unknown, label: string): string {
  if (typeof value !== "string") throw new Error(`${label} must be a string`);
  return value;
}

function nullableString(value: unknown, label: string): string | null {
  if (value === null) return null;
  return stringValue(value, label);
}

function integerValue(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) throw new Error(`${label} must be a safe integer`);
  return value;
}
