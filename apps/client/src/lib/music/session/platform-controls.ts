import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { MusicTrayUpdate } from "./platform-controls-contracts";

/** Bring the desktop window forward for context inspection. */
export function focusMusicWindow(): void {
  const appWindow = getCurrentWindow();
  void appWindow.show().then(() => appWindow.setFocus()).catch(() => undefined);
}

/** Publish desktop tray state without coupling the shared player to Tauri windows. */
export async function publishMusicTray(update: MusicTrayUpdate): Promise<void> {
  await invoke("update_music_tray", { update });
}
