import { createVaultChangeChannel } from "$lib/windows/vault-change-channel";

const CHAT_SYNC_CHANGED_EVENT = "ganbaru-chat-sync-changed";
const CHAT_SYNC_CHANGE_KEY = "chat";
const chatSyncChanges = createVaultChangeChannel("chat-sync-changed");

/** Tells this window and the other windows of the vault that Chat rows changed outside their Chat views. */
export function notifyChatSyncChanged(): void {
  if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent(CHAT_SYNC_CHANGED_EVENT));
  chatSyncChanges.publish([CHAT_SYNC_CHANGE_KEY]);
}

/** Listens for {@link notifyChatSyncChanged} in this window, and returns a function that stops listening. */
export function onChatSyncChanged(listener: () => void): () => void {
  if (typeof window === "undefined") return () => undefined;
  window.addEventListener(CHAT_SYNC_CHANGED_EVENT, listener);
  return () => window.removeEventListener(CHAT_SYNC_CHANGED_EVENT, listener);
}

/** Listens for {@link notifyChatSyncChanged} from other windows of the vault, and resolves to a function that stops listening. */
export function listenForChatSyncChanges(onChange: () => void): Promise<() => void> {
  return chatSyncChanges.listen(() => onChange());
}
