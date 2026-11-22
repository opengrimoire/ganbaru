import { getCurrentWindow } from "@tauri-apps/api/window";

/** Bring the desktop window forward for context inspection. */
export function focusMusicWindow(): void {
  const appWindow = getCurrentWindow();
  void appWindow.show().then(() => appWindow.setFocus()).catch(() => undefined);
}
