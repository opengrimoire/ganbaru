/** Window event asking the Chat header to show the members panel of a channel. */
export const CHAT_OPEN_MEMBERS_EVENT = "ganbaru-ai:chat-open-members";

export interface ChatOpenMembersDetail {
  readonly channelId: string;
  /** Opens the member picker with the panel. */
  readonly addMembers: boolean;
}

/** Validates the untyped detail of a members panel event. */
export function readChatOpenMembersDetail(event: Event): ChatOpenMembersDetail | null {
  if (!(event instanceof CustomEvent) || typeof event.detail !== "object" || event.detail === null) return null;
  const detail: Record<string, unknown> = event.detail;
  if (typeof detail.channelId !== "string") return null;
  return { channelId: detail.channelId, addMembers: detail.addMembers === true };
}
