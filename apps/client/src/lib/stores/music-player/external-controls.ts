import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { createMusicBrowserControls } from "./browser-controls";
import type { MusicExternalControls, MusicExternalControlsContext } from "./external-controls-contracts";

interface DesktopMusicExternalControlsContext extends MusicExternalControlsContext {
  listen?: typeof listen;
}

/** Owns browser controls and presentation requests; native hardware and tray transport stay native. */
export function createMusicExternalControls(context: DesktopMusicExternalControlsContext): MusicExternalControls {
  const browser = createMusicBrowserControls(context);
  let initialized = false;
  let unsubscribe: UnlistenFn | null = null;

  function init(): void {
    if (initialized) return;
    initialized = true;
    browser.init();
    void (context.listen ?? listen)("tray-music-inspect-assignment", () => context.inspectAssignment())
      .then((unlisten) => { if (initialized) unsubscribe = unlisten; else unlisten(); })
      .catch((error: unknown) => console.error("Music assignment listener failed:", error));
  }

  function destroy(): void {
    initialized = false;
    browser.destroy();
    unsubscribe?.();
    unsubscribe = null;
  }

  return { init, destroy, isInitialized: () => initialized, update: browser.update,
    updateBrowser: browser.update };
}
