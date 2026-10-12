const MUSIC_LIBRARY_CHANGED_EVENT = "ganbaru-ai-music-library-changed";

export function notifyMusicLibraryChanged(): void {
  if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent(MUSIC_LIBRARY_CHANGED_EVENT));
}

export function onMusicLibraryChanged(listener: () => void): () => void {
  if (typeof window === "undefined") return () => undefined;
  window.addEventListener(MUSIC_LIBRARY_CHANGED_EVENT, listener);
  return () => window.removeEventListener(MUSIC_LIBRARY_CHANGED_EVENT, listener);
}
