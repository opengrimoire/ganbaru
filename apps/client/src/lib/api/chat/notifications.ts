import { invoke } from "@tauri-apps/api/core";
import type { ChatNotificationText } from "$lib/chat/notifications/delivery";

/** A Chat system notification as the desktop shell shows it. */
export interface ShowChatNotificationRequest extends ChatNotificationText {
  /** Whether the app notification sound plays with it. */
  playSound: boolean;
}

/**
 * Shows a desktop Chat notification. Opening it emits `chat-notification-open` with the target and raises the main
 * window.
 */
export async function showChatNotification(request: ShowChatNotificationRequest): Promise<void> {
  await invoke("show_chat_notification", {
    title: request.title,
    body: request.body,
    openLabel: request.openLabel,
    target: request.target,
    playSound: request.playSound,
  });
}
