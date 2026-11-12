/** Pane event asking the Chat header to show the notes panel of a channel. */
export const CHAT_OPEN_CHANNEL_NOTES_EVENT = "ganbaru:chat-open-channel-notes";

export interface ChatOpenChannelNotesDetail {
  readonly channelId: string;
}

/** Validates the untyped detail of a notes panel event. */
export function readChatOpenChannelNotesDetail(event: Event): ChatOpenChannelNotesDetail | null {
  if (!(event instanceof CustomEvent) || typeof event.detail !== "object" || event.detail === null) return null;
  const detail: Record<string, unknown> = event.detail;
  return typeof detail.channelId === "string" ? { channelId: detail.channelId } : null;
}
