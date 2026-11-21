import type {
  ChatChannelId,
  ChatConversationId,
  UtcTimestamp,
} from "./common";
import type {
  ChatConversationMembershipRead,
  ChatWorkAssignmentState,
} from "./organizational";

export interface ChatChannelRead {
  id: ChatChannelId;
  conversationId: ChatConversationId;
  projectId: string;
  name: string;
  topic: string;
  isDefault: boolean;
  memberships: ChatConversationMembershipRead[];
  messageCount: number;
  unreadCount: number;
  /** New replies, after the last visit and not read in their thread, in threads the person took part in. */
  unreadReplyCount: number;
  /** Whether a failed run in one of its threads has not been seen since it failed. */
  unseenFailure: boolean;
  latestPreview: string | null;
  lastActivityAt: UtcTimestamp;
  attentionState: ChatWorkAssignmentState | null;
  revision: number;
  archivedAt: UtcTimestamp | null;
  createdAt: UtcTimestamp;
  updatedAt: UtcTimestamp;
}

export interface CreateChatChannelRequest {
  id: ChatChannelId;
  projectId: string;
  name: string;
  topic: string;
}

export interface UpdateChatChannelDetailsRequest {
  channelId: ChatChannelId;
  name: string;
  topic: string;
  expectedRevision: number;
}
