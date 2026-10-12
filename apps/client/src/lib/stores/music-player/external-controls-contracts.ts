import type { MusicSource } from "$lib/music/sources";
import type { PlaybackSnapshot } from "$lib/music/playback";

export interface MusicExternalControlsContext {
  currentSource(): MusicSource | null;
  snapshot(): PlaybackSnapshot;
  title(): string;
  sourceKindLabel(): string;
  artworkUrl(): string | null;
  isBusy(): boolean;
  canPrevious(): boolean;
  canNext(): boolean;
  volume(): number;
  muted(): boolean;
  shuffleEnabled(): boolean;
  play(): Promise<void>;
  pause(): Promise<void>;
  togglePlay(): Promise<void>;
  stop(): Promise<void>;
  previous(): Promise<void>;
  next(): Promise<void>;
  seekBy(deltaMs: number): Promise<void>;
  seekTo(positionMs: number): Promise<void>;
  setVolume(volume: number): Promise<void>;
  setRate(rate: number): Promise<void>;
  toggleShuffle(): void;
  handleWindowMessage(event: MessageEvent<unknown>): void;
  inspectAssignment(): void;
}

export interface MusicExternalControls {
  init(): void;
  destroy(): void;
  isInitialized(): boolean;
  update(): void;
  updateBrowser(): void;
}
