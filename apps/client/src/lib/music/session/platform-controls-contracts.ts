import type { PlaybackStatus } from "$lib/music/playback";

export interface MusicTrayUpdate {
  status: PlaybackStatus;
  title: string | null;
  canPlayPause: boolean;
  canPrevious: boolean;
  canNext: boolean;
  contextLabel: string | null;
}
