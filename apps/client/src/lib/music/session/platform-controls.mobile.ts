import type { MusicTrayUpdate } from "./platform-controls-contracts";

/** Android is already presenting the active activity when this action is handled. */
export function focusMusicWindow(): void {}

/** Media3 owns Android system playback presentation instead of a desktop tray. */
export function publishMusicTray(_update: MusicTrayUpdate): Promise<void> {
  return Promise.resolve();
}
