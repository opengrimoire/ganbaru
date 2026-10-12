import type { Component } from "svelte";

export type MusicBuilderInitialAction = "new-playlist" | "open-playlists" | "open-review" | { kind: "open-review-item"; itemId: string } | { kind: "open-item"; itemId: string } | { kind: "open-issues" } | { kind: "open-soundscapes" };

export interface MusicBuilderComponentProps {
  onOpenPlayer: () => void;
  presentation?: "desktop" | "mobile";
  active?: boolean;
  initialAction?: MusicBuilderInitialAction | null;
  onInitialActionHandled?: () => void;
}

export type MusicBuilderComponent = Component<MusicBuilderComponentProps>;

export interface LazyMusicBuilderLoader {
  load(): Promise<MusicBuilderComponent>;
  peek(): MusicBuilderComponent | null;
}

/** Caches a successful lazy component load while allowing explicit retry after failure. */
export function createLazyMusicBuilderLoader(
  importer: () => Promise<{ default: MusicBuilderComponent }>,
): LazyMusicBuilderLoader {
  let loaded: MusicBuilderComponent | null = null;
  let pending: Promise<MusicBuilderComponent> | null = null;

  return {
    load(): Promise<MusicBuilderComponent> {
      if (loaded) return Promise.resolve(loaded);
      if (pending) return pending;
      pending = importer()
        .then((module) => {
          loaded = module.default;
          return loaded;
        })
        .finally(() => {
          pending = null;
        });
      return pending;
    },
    peek: () => loaded,
  };
}

export const musicBuilderLoader = createLazyMusicBuilderLoader(
  () => import("$lib/components/music/MusicPlaylistBuilderPage.svelte"),
);
