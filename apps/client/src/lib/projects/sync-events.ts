const PROJECTS_SYNC_CHANGED_EVENT = "ganbaru-projects-sync-changed";

/** Tells this window's Projects store that sync recovery changed rows outside it. */
export function notifyProjectsSyncChanged(): void {
  if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent(PROJECTS_SYNC_CHANGED_EVENT));
}

/** Listens for {@link notifyProjectsSyncChanged}, and returns a function that stops listening. */
export function onProjectsSyncChanged(listener: () => void): () => void {
  if (typeof window === "undefined") return () => undefined;
  window.addEventListener(PROJECTS_SYNC_CHANGED_EVENT, listener);
  return () => window.removeEventListener(PROJECTS_SYNC_CHANGED_EVENT, listener);
}
