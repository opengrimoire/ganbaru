import type { ChatChannelRead } from "../contracts";
import { CHAT_WORK_ASSIGNMENT_STATES } from "../contracts";
import { parseChatConversationMembership } from "./organizational";
import {
  readArray,
  readBoolean,
  readEnum,
  readIdentifier,
  readNullable,
  readNonNegativeSafeInteger,
  readRecord,
  readString,
  readUtcTimestamp,
} from "./readers";

export function parseChatChannel(value: unknown, label = "Chat channel"): ChatChannelRead {
  const record = readRecord(value, label);
  return {
    id: readIdentifier(record.id, `${label}.id`),
    conversationId: readIdentifier(record.conversationId, `${label}.conversationId`),
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    name: readString(record.name, `${label}.name`),
    topic: readString(record.topic, `${label}.topic`),
    isDefault: readBoolean(record.isDefault, `${label}.isDefault`),
    memberships: readArray(record.memberships, `${label}.memberships`, parseChatConversationMembership),
    messageCount: readNonNegativeSafeInteger(record.messageCount, `${label}.messageCount`),
    unreadCount: readNonNegativeSafeInteger(record.unreadCount, `${label}.unreadCount`),
    unreadReplyCount: readNonNegativeSafeInteger(record.unreadReplyCount, `${label}.unreadReplyCount`),
    unseenFailure: readBoolean(record.unseenFailure, `${label}.unseenFailure`),
    latestPreview: readNullable(record.latestPreview, `${label}.latestPreview`, readString),
    lastActivityAt: readUtcTimestamp(record.lastActivityAt, `${label}.lastActivityAt`),
    attentionState: readNullable(
      record.attentionState,
      `${label}.attentionState`,
      (entry, entryLabel) => readEnum(entry, CHAT_WORK_ASSIGNMENT_STATES, entryLabel),
    ),
    revision: readNonNegativeSafeInteger(record.revision, `${label}.revision`),
    archivedAt: readNullable(record.archivedAt, `${label}.archivedAt`, readUtcTimestamp),
    createdAt: readUtcTimestamp(record.createdAt, `${label}.createdAt`),
    updatedAt: readUtcTimestamp(record.updatedAt, `${label}.updatedAt`),
  };
}

export function parseChatChannels(value: unknown, label = "Chat channels"): ChatChannelRead[] {
  return readArray(value, label, parseChatChannel);
}
