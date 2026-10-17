import { createMusicBrowserControls } from "./browser-controls";
import type {
  MusicExternalControls,
  MusicExternalControlsContext,
} from "./external-controls-contracts";

/** Owns Android WebView controls while Media3 owns native system controls. */
export function createMusicExternalControls(
  context: MusicExternalControlsContext,
): MusicExternalControls {
  const browser = createMusicBrowserControls(context);
  let initialized = false;

  function init(): void {
    if (initialized) return;
    initialized = true;
    browser.init();
  }

  function destroy(): void {
    browser.destroy();
    initialized = false;
  }

  return {
    init,
    destroy,
    isInitialized: () => initialized,
    update: browser.update,
    updateBrowser: browser.update,
  };
}
