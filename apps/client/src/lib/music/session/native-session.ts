import type { MusicItemAvailability, MusicPlaybackMode, MusicRepeatMode, MusicWeight } from "$lib/music/library/contracts";
import type { PlaybackStatus } from "$lib/music/playback";
import type { MusicSource } from "$lib/music/sources";
import type { MusicActivityPhase, MusicAssignmentBehavior, MusicAssignmentSource } from "$lib/music/context-assignment";

export type NativeMusicBackend = "native-audio" | "browser";
export type NativeMusicOwner = "manual" | "review" | "calendar-event" | "pomodoro";
export type NativeMusicIssue = "browser-host-unavailable" | "no-eligible-items" | "source-failure" | "persistence-failure" | "interrupted" | "restored";

export interface NativeMusicQueueEntry {
  itemId: string | null;
  membershipId: string | null;
  source: MusicSource;
  backend: NativeMusicBackend;
  availability: MusicItemAvailability;
  enabled: boolean;
  weight: MusicWeight;
  snoozedUntil: number | null;
  snoozedIndefinitely: boolean;
  embeddingBlocked: boolean;
  bound: boolean;
  phaseAllowed: boolean;
  skipRanges: { startMs: number; endMs: number }[];
  volume: number | null;
  rate: number | null;
}

export interface NativeMusicSnapshot {
  soundscapeVersion?: number | null;
  sessionId: string;
  revision: number;
  generation: number;
  queueRevision: number;
  currentIndex: number | null;
  currentSource: MusicSource | null;
  backend: NativeMusicBackend | null;
  status: PlaybackStatus;
  positionMs: number;
  durationMs: number | null;
  volume: number;
  muted: boolean;
  rate: number;
  order: MusicPlaybackMode;
  repeatMode: MusicRepeatMode;
  owner: NativeMusicOwner;
  context: NativeMusicContext | null;
  playlistId: string | null;
  queueName: string;
  canPrevious: boolean;
  canNext: boolean;
  issue: NativeMusicIssue | null;
  error: string | null;
  reviewCheckpointId: string | null;
  queue: NativeMusicQueueEntry[] | null;
}

export interface NativeMusicContext {
  activationKey: string; eventId: string; eventTitle: string; phase: MusicActivityPhase;
  behavior: MusicAssignmentBehavior; assignmentSource: MusicAssignmentSource; playlistId: string | null;
  state: "playing" | "prepared" | "paused" | "kept" | "unavailable" | "overridden";
  issue: "missing-playlist" | "no-eligible-items" | "offline-only" | "deleted-soundscape" | "activation-failed" | null;
}

function parseContext(value: unknown): NativeMusicContext {
  const raw = record(value);
  return { activationKey: text(raw.activationKey), eventId: text(raw.eventId), eventTitle: text(raw.eventTitle),
    phase: choice(raw.phase, ["focus", "short-break", "long-break"] as const),
    behavior: choice(raw.behavior, ["inherit", "play-automatically", "prepare-silently", "pause-music", "keep-current-music"] as const),
    assignmentSource: choice(raw.assignmentSource, ["event-override", "work-environment", "project-snapshot", "none"] as const),
    playlistId: optional(raw.playlistId, text), state: choice(raw.state, ["playing", "prepared", "paused", "kept", "unavailable", "overridden"] as const),
    issue: optional(raw.issue, (value) => choice(value, ["missing-playlist", "no-eligible-items", "offline-only", "deleted-soundscape", "activation-failed"] as const)) };
}

export type NativeMusicIntent =
  | { kind: "play" | "pause" | "toggle" | "stop" | "next" | "previous" | "refresh" | "retry-context" | "suspend-review" }
  | { kind: "select"; index: number }
  | { kind: "seek"; positionMs: number }
  | { kind: "seek-by"; deltaMs: number }
  | { kind: "volume"; volume: number }
  | { kind: "muted"; muted: boolean }
  | { kind: "rate"; rate: number }
  | { kind: "order"; order: MusicPlaybackMode }
  | { kind: "online"; online: boolean }
  | { kind: "browser-host"; available: boolean }
  | { kind: "restore-review"; checkpointId: string };

export type NativeMusicQueueIntent =
  | { kind: "saved-playlist"; playlistId: string; explicitItemId: string | null; avoidItemId: string | null }
  | { kind: "library-items"; itemIds: string[]; selectedItemId: string | null; name: string }
  | { kind: "sources"; sources: MusicSource[]; selectedIndex: number | null; name: string }
  | { kind: "review-item"; itemId: string };

export interface NativeMusicStart {
  actionId: string;
  queue: NativeMusicQueueIntent;
  autoplay: boolean;
  resume: boolean;
  order: MusicPlaybackMode;
  volume: number;
  muted: boolean;
  rate: number;
}

export interface NativeMusicCommand { actionId: string; sessionId: string | null; intent: NativeMusicIntent }
export interface NativeMusicObservation {
  sessionId: string; generation: number; sequence: number; sourceIdentity: string;
  status: PlaybackStatus; positionMs: number; durationMs: number | null; error: string | null;
}

export type NativeMusicEffect =
  | { kind: "load"; sessionId: string; generation: number; source: MusicSource; backend: NativeMusicBackend; positionMs: number; autoplay: boolean; volume: number; muted: boolean; rate: number }
  | { kind: "play" | "pause" | "stop"; generation: number }
  | { kind: "seek"; generation: number; positionMs: number }
  | { kind: "settings"; generation: number; volume: number; muted: boolean; rate: number };

export interface NativeMusicFrame {
  subscriptionId: string;
  sequence: number;
  effectsExpireAtMs: number;
  snapshot: NativeMusicSnapshot;
  effects: NativeMusicEffect[];
}

const MAX_NATIVE_MUSIC_EFFECTS = 32;
const MAX_NATIVE_MUSIC_SUBSCRIPTION_ID_LENGTH = 128;

export type NativeMusicNotice = { kind: "update"; subscriptionId: string; sequence: number }
  | { kind: "invalidate"; subscriptionId: string };

/** Validates small notices before requesting a frame or revoking the previous context. */
export function parseNativeMusicNotice(value: unknown): NativeMusicNotice {
  const raw = record(value);
  const subscriptionId = text(raw.subscriptionId);
  if (!subscriptionId || subscriptionId.length > MAX_NATIVE_MUSIC_SUBSCRIPTION_ID_LENGTH) throw new Error("Invalid native Music notice");
  const kind = choice(raw.kind, ["update", "invalidate"] as const);
  if (kind === "invalidate") return { kind, subscriptionId };
  const sequence = integer(raw.sequence);
  if (sequence === 0) throw new Error("Invalid native Music notice");
  return { kind, subscriptionId, sequence };
}

const STATUSES = ["idle", "loading", "ready", "playing", "paused", "ended", "error"] as const;
const BACKENDS = ["native-audio", "browser"] as const;
const ORDERS = ["in-order", "shuffle", "mix"] as const;

function record(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("Invalid native Music record");
  return value as Record<string, unknown>;
}
function text(value: unknown): string {
  if (typeof value !== "string" || value.length > 16_384) throw new Error("Invalid native Music text");
  return value;
}
function number(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value)) throw new Error("Invalid native Music number");
  return value;
}
function integer(value: unknown): number {
  const result = number(value);
  if (!Number.isSafeInteger(result) || result < 0) throw new Error("Invalid native Music integer");
  return result;
}
function boolean(value: unknown): boolean {
  if (typeof value !== "boolean") throw new Error("Invalid native Music flag");
  return value;
}
function volume(value: unknown): number {
  const result = number(value);
  if (result < 0 || result > 1) throw new Error("Invalid native Music volume");
  return result;
}
function rate(value: unknown): number {
  const result = number(value);
  if (result < 0.25 || result > 2) throw new Error("Invalid native Music rate");
  return result;
}
function optional<T>(value: unknown, parse: (input: unknown) => T): T | null { return value === null || value === undefined ? null : parse(value); }
function choice<const T extends readonly string[]>(value: unknown, values: T): T[number] {
  if (typeof value !== "string" || !values.includes(value)) throw new Error("Invalid native Music enum");
  return value as T[number];
}

/** Validates the platform source contract without accepting arbitrary response casts. */
export function parseNativeMusicSource(value: unknown): MusicSource {
  const raw = record(value);
  const common = { identity: text(raw.identity), originalInput: text(raw.originalInput), title: text(raw.title), startMs: optional(raw.startMs, integer), endMs: optional(raw.endMs, integer) };
  const kind = choice(raw.kind, ["local-file", "youtube-video", "youtube-playlist"] as const);
  if (kind === "local-file") return { ...common, kind, path: optional(raw.path, text) ?? "", artworkPath: optional(raw.artworkPath, text) };
  if (kind === "youtube-video") return { ...common, kind, videoId: text(raw.videoId), playlistId: optional(raw.playlistId, text) };
  return { ...common, kind, playlistId: text(raw.playlistId), videoId: optional(raw.videoId, text) };
}

function parseEntry(value: unknown): NativeMusicQueueEntry {
  const raw = record(value);
  if (!Array.isArray(raw.skipRanges) || raw.skipRanges.length > 100_000) throw new Error("Invalid native Music skip ranges");
  return {
    itemId: optional(raw.itemId, text), membershipId: optional(raw.membershipId, text), source: parseNativeMusicSource(raw.source),
    backend: choice(raw.backend, BACKENDS), availability: choice(raw.availability, ["available", "missing", "unavailable", "ambiguous", "unknown"] as const),
    enabled: boolean(raw.enabled), weight: choice(raw.weight, ["rarely", "less-often", "normal", "more-often", "much-more-often"] as const),
    snoozedUntil: optional(raw.snoozedUntil, integer), snoozedIndefinitely: boolean(raw.snoozedIndefinitely),
    embeddingBlocked: boolean(raw.embeddingBlocked), bound: boolean(raw.bound), phaseAllowed: boolean(raw.phaseAllowed),
    skipRanges: raw.skipRanges.map((value) => { const range = record(value); return { startMs: integer(range.startMs), endMs: integer(range.endMs) }; }),
    volume: optional(raw.volume, volume), rate: optional(raw.rate, rate),
  };
}

/** Parses a bounded canonical projection; absent queue rows preserve the existing projection. */
export function parseNativeMusicSnapshot(value: unknown): NativeMusicSnapshot {
  const raw = record(value);
  const queue = optional(raw.queue, (value) => {
    if (!Array.isArray(value) || value.length > 10_000) throw new Error("Invalid native Music queue");
    const entries = value.map(parseEntry);
    if (entries.reduce((sum, entry) => sum + entry.skipRanges.length, 0) > 100_000) throw new Error("Invalid native Music skip ranges");
    return entries;
  });
  const currentIndex = optional(raw.currentIndex, integer);
  if (queue && currentIndex !== null && currentIndex >= queue.length) throw new Error("Invalid native Music selection");
  return {
    sessionId: text(raw.sessionId), revision: integer(raw.revision), generation: integer(raw.generation), queueRevision: integer(raw.queueRevision),
    currentIndex, currentSource: optional(raw.currentSource, parseNativeMusicSource),
    backend: optional(raw.backend, (value) => choice(value, BACKENDS)), status: choice(raw.status, STATUSES),
    positionMs: integer(raw.positionMs), durationMs: optional(raw.durationMs, integer), volume: volume(raw.volume), muted: boolean(raw.muted), rate: rate(raw.rate),
    order: choice(raw.order, ORDERS), repeatMode: choice(raw.repeatMode, ["off", "one", "all"] as const),
    soundscapeVersion: optional(raw.soundscapeVersion, integer),
    owner: choice(raw.owner, ["manual", "review", "calendar-event", "pomodoro"] as const), context: optional(raw.context, parseContext), playlistId: optional(raw.playlistId, text), queueName: text(raw.queueName),
    canPrevious: boolean(raw.canPrevious), canNext: boolean(raw.canNext),
    issue: optional(raw.issue, (value) => choice(value, ["browser-host-unavailable", "no-eligible-items", "source-failure", "persistence-failure", "interrupted", "restored"] as const)),
    error: optional(raw.error, text), reviewCheckpointId: optional(raw.reviewCheckpointId, text), queue,
  };
}

/** Parses the narrow browser effect protocol emitted after native persistence succeeds. */
export function parseNativeMusicEffect(value: unknown): NativeMusicEffect {
  const raw = record(value);
  const generation = integer(raw.generation);
  const kind = choice(raw.kind, ["load", "play", "pause", "stop", "seek", "settings"] as const);
  if (kind === "load") return { kind, generation, sessionId: text(raw.sessionId), source: parseNativeMusicSource(raw.source), backend: choice(raw.backend, BACKENDS), positionMs: integer(raw.positionMs), autoplay: boolean(raw.autoplay), volume: volume(raw.volume), muted: boolean(raw.muted), rate: rate(raw.rate) };
  if (kind === "seek") return { kind, generation, positionMs: integer(raw.positionMs) };
  if (kind === "settings") return { kind, generation, volume: volume(raw.volume), muted: boolean(raw.muted), rate: rate(raw.rate) };
  return { kind, generation };
}

/** Validates bounded stream frames before presentation or browser effects run. */
export function parseNativeMusicFrame(value: unknown): NativeMusicFrame {
  const raw = record(value);
  if (!Array.isArray(raw.effects) || raw.effects.length > MAX_NATIVE_MUSIC_EFFECTS) throw new Error("Invalid native Music effects");
  const sequence = integer(raw.sequence);
  if (sequence === 0) throw new Error("Invalid native Music sequence");
  return { subscriptionId: text(raw.subscriptionId), sequence, effectsExpireAtMs: integer(raw.effectsExpireAtMs),
    snapshot: parseNativeMusicSnapshot(raw.snapshot), effects: raw.effects.map(parseNativeMusicEffect) };
}
