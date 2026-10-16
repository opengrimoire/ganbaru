import { emit } from "@tauri-apps/api/event";

/** Asks the window that runs the browser decoders to come forward with its Music panel open. */
export const MUSIC_HOST_OPEN_EVENT = "music-host-open";

/** Bring forward the window that shows browser-decoded Music video, opening its Music panel. */
export function openMusicInHostWindow(): Promise<void> {
  return emit(MUSIC_HOST_OPEN_EVENT);
}
