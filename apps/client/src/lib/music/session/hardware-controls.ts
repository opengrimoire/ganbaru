export type MusicHardwareControlAction =
  | "play"
  | "pause"
  | "playPause"
  | "stop"
  | "previousTrack"
  | "nextTrack"
  | "seekBy"
  | "seekTo"
  | "setVolume"
  | "setRate"
  | "setShuffle";

export interface MusicHardwareControlPayload {
  action: MusicHardwareControlAction;
  deltaMs?: number;
  positionMs?: number;
  volume?: number;
  rate?: number;
  shuffleEnabled?: boolean;
}

interface HardwareKeyInput {
  key: string;
  code: string;
}

const hardwareKeyActions: Readonly<Record<string, MusicHardwareControlAction>> = Object.freeze({
  AudioNext: "nextTrack",
  AudioPause: "pause",
  AudioPlay: "play",
  AudioPlayPause: "playPause",
  AudioPrev: "previousTrack",
  AudioPrevious: "previousTrack",
  AudioStop: "stop",
  MediaNextTrack: "nextTrack",
  MediaPause: "pause",
  MediaPlay: "play",
  MediaPlayPause: "playPause",
  MediaPreviousTrack: "previousTrack",
  MediaStop: "stop",
  MediaTrackNext: "nextTrack",
  MediaTrackPrevious: "previousTrack",
});

export function musicHardwareActionFromKey(event: HardwareKeyInput): MusicHardwareControlAction | null {
  return hardwareKeyActions[event.key] ?? hardwareKeyActions[event.code] ?? null;
}

export function parseMusicHardwareControlPayload(value: unknown): MusicHardwareControlPayload | null {
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  if (!isMusicHardwareControlAction(record.action)) return null;

  const payload: MusicHardwareControlPayload = { action: record.action };
  const deltaMs = optionalFiniteNumber(record.deltaMs);
  const positionMs = optionalFiniteNumber(record.positionMs);
  const volume = optionalFiniteNumber(record.volume);
  const rate = optionalFiniteNumber(record.rate);

  if (deltaMs !== undefined) payload.deltaMs = Math.round(deltaMs);
  if (positionMs !== undefined) payload.positionMs = Math.max(0, Math.round(positionMs));
  if (volume !== undefined) payload.volume = volume;
  if (rate !== undefined) payload.rate = rate;
  if (typeof record.shuffleEnabled === "boolean") payload.shuffleEnabled = record.shuffleEnabled;

  return payload;
}

function isMusicHardwareControlAction(value: unknown): value is MusicHardwareControlAction {
  return value === "play"
    || value === "pause"
    || value === "playPause"
    || value === "stop"
    || value === "previousTrack"
    || value === "nextTrack"
    || value === "seekBy"
    || value === "seekTo"
    || value === "setVolume"
    || value === "setRate"
    || value === "setShuffle";
}

function optionalFiniteNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}
