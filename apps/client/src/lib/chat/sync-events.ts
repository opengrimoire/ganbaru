const CHAT_SYNC_CHANGED_EVENT = "ganbaru-chat-sync-changed";

/** Tells this window's Chat views that sync changed Chat rows outside them. */
export function notifyChatSyncChanged(): void {
  if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent(CHAT_SYNC_CHANGED_EVENT));
}

/** Listens for {@link notifyChatSyncChanged}, and returns a function that stops listening. */
export function onChatSyncChanged(listener: () => void): () => void {
  if (typeof window === "undefined") return () => undefined;
  window.addEventListener(CHAT_SYNC_CHANGED_EVENT, listener);
  return () => window.removeEventListener(CHAT_SYNC_CHANGED_EVENT, listener);
}
