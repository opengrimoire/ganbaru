import { createMusicBrowserControls } from "./browser-controls";
import type { MusicExternalControls, MusicExternalControlsContext } from "./external-controls-contracts";

/** Owns browser controls; native hardware and tray transport stay native. */
export function createMusicExternalControls(context: MusicExternalControlsContext): MusicExternalControls {
  const browser = createMusicBrowserControls(context);
  let initialized = false;

  function init(): void {
    if (initialized) return;
    initialized = true;
    browser.init();
  }

  function destroy(): void {
    initialized = false;
    browser.destroy();
  }

  return { init, destroy, isInitialized: () => initialized, update: browser.update,
    updateBrowser: browser.update };
}
